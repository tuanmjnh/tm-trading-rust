use std::io::{self, BufRead, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use chrono::Utc;
use thiserror::Error;
use tm_trading_agent_protocol::{SupervisorCommand, WORKER_CONFIGURATION_EXIT_CODE, WorkerMessage};
use tm_trading_notification_worker::{
    DEFAULT_CLAIM_INTERVAL, DEFAULT_OUTBOX_LEASE, NotificationDrainer, build_worker_evidence,
    hello_message,
};
use tm_trading_storage::{
    MongoConnection, MongoNotificationLogRepository, MongoNotificationOutboxRepository,
    RepositoryError,
};
use tm_trading_telegram_client::{TelegramBotToken, TelegramClient};
use tokio::sync::mpsc;

const DEFAULT_DATABASE_NAME: &str = "tm-trading-rust";
const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(RunError::Configuration(error)) => {
            eprintln!("[notification-worker] fatal configuration error: {error}");
            ExitCode::from(u8::try_from(WORKER_CONFIGURATION_EXIT_CODE).unwrap_or(78))
        }
        Err(error) => {
            eprintln!("[notification-worker] fatal: {error}");
            ExitCode::from(2)
        }
    }
}

#[derive(Debug, Error)]
enum RunError {
    #[error("{0}")]
    Configuration(String),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

async fn run() -> Result<(), RunError> {
    let uri =
        std::env::var("TM_DATABASE_URI").unwrap_or_else(|_| "mongodb://127.0.0.1:27017".to_owned());
    let database_name =
        std::env::var("TM_DATABASE_NAME").unwrap_or_else(|_| DEFAULT_DATABASE_NAME.to_owned());

    if !is_env_enabled("TM_TRADING_NOTIFICATION_WORKER_ENABLED") {
        return Err(RunError::Configuration(
            "TM_TRADING_NOTIFICATION_WORKER_ENABLED must be enabled before the notification worker can run".to_owned(),
        ));
    }

    let pid = std::process::id();
    let instance_id = std::env::var("TM_SERVICE_INSTANCE_ID")
        .unwrap_or_else(|_| format!("notification-worker@local-agent:{pid}"));

    let connection = Arc::new(MongoConnection::connect(&uri, &database_name).await?);
    let outbox_repo =
        Arc::new(MongoNotificationOutboxRepository::new(connection.database()).await?);
    let log_repo = Arc::new(MongoNotificationLogRepository::new(connection.database()).await?);

    let telegram_token_str = std::env::var("TM_TRADING_TELEGRAM_BOT_TOKEN")
        .or_else(|_| std::env::var("TM_TELEGRAM_BOT_TOKEN"))
        .ok();
    let telegram_chat_id = std::env::var("TM_TRADING_TELEGRAM_CHAT_ID")
        .or_else(|_| std::env::var("TM_TELEGRAM_CHAT_ID"))
        .ok();
    let telegram_base_url = std::env::var("TM_TRADING_TELEGRAM_BASE_URL")
        .or_else(|_| std::env::var("TM_TELEGRAM_BASE_URL"))
        .ok();

    let (telegram_client, telegram_configured) = match telegram_token_str {
        Some(token_str) => match TelegramBotToken::new(token_str) {
            Ok(token) => match TelegramClient::new(token, telegram_base_url.as_deref(), None) {
                Ok(client) => {
                    let has_chat = telegram_chat_id.is_some();
                    (Some(client), has_chat)
                }
                Err(e) => {
                    eprintln!("[notification-worker] invalid telegram client config: {e}");
                    (None, false)
                }
            },
            Err(e) => {
                eprintln!("[notification-worker] invalid telegram bot token: {e}");
                (None, false)
            }
        },
        None => (None, false),
    };

    let drainer =
        NotificationDrainer::new(outbox_repo, log_repo, telegram_client, telegram_chat_id);

    // Initial Hello message
    let exe_path = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "tm-trading-notification-worker".to_owned());

    let hello = hello_message(
        &instance_id,
        Utc::now(),
        pid,
        &exe_path,
        env!("CARGO_PKG_VERSION"),
    );
    send_message(&hello)?;

    let initial_evidence = build_worker_evidence(None, 0, true, telegram_configured);
    let heartbeat = WorkerMessage::Heartbeat {
        instance_id: instance_id.clone(),
        at: Utc::now(),
        evidence: initial_evidence.clone(),
    };
    send_message(&heartbeat)?;

    let ready = WorkerMessage::Ready {
        instance_id: instance_id.clone(),
        at: Utc::now(),
        evidence: initial_evidence,
    };
    send_message(&ready)?;

    // Spawn stdin reader for Agent commands
    let (command_tx, mut command_rx) = mpsc::unbounded_channel::<SupervisorCommand>();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(text) => {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(cmd) = serde_json::from_str::<SupervisorCommand>(trimmed)
                        && command_tx.send(cmd).is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut claim_interval = tokio::time::interval(DEFAULT_CLAIM_INTERVAL);
    let mut heartbeat_interval = tokio::time::interval(DEFAULT_HEARTBEAT_INTERVAL);

    let mut last_success_at: Option<chrono::DateTime<Utc>> = None;
    let mut processed_count = 0usize;

    loop {
        tokio::select! {
            cmd = command_rx.recv() => {
                match cmd {
                    Some(SupervisorCommand::Shutdown { reason, .. }) => {
                        eprintln!("[notification-worker] shutdown requested: {reason}");
                        break;
                    }
                    Some(SupervisorCommand::RunOnce { .. }) => {}
                    None => {
                        // Stdin closed -> agent exited
                        break;
                    }
                }
            }
            _ = claim_interval.tick() => {
                let now = Utc::now();
                match drainer.drain_one(&instance_id, now, DEFAULT_OUTBOX_LEASE).await {
                    Ok(Some(outcome)) => {
                        if outcome.accepted || outcome.state == tm_trading_domain::NotificationState::Sent {
                            last_success_at = Some(now);
                            processed_count = processed_count.saturating_add(1);
                        }
                    }
                    Ok(None) => {
                        // Idle
                    }
                    Err(e) => {
                        eprintln!("[notification-worker] drain error: {e}");
                    }
                }
            }
            _ = heartbeat_interval.tick() => {
                let evidence = build_worker_evidence(
                    last_success_at,
                    processed_count,
                    true,
                    telegram_configured,
                );
                let heartbeat = WorkerMessage::Heartbeat {
                    instance_id: instance_id.clone(),
                    at: Utc::now(),
                    evidence,
                };
                let _ = send_message(&heartbeat);
            }
        }
    }

    let stopped = WorkerMessage::Stopped {
        instance_id: instance_id.clone(),
        at: Utc::now(),
        reason: "notification worker graceful stop".to_owned(),
        drained: true,
    };
    let _ = send_message(&stopped);

    Ok(())
}

fn send_message(msg: &WorkerMessage) -> io::Result<()> {
    let json = serde_json::to_string(msg).map_err(io::Error::other)?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{json}")?;
    stdout.flush()
}

fn is_env_enabled(key: &str) -> bool {
    std::env::var(key).is_ok_and(|val| {
        let v = val.trim().to_ascii_lowercase();
        v == "1" || v == "true" || v == "yes" || v == "on"
    })
}
