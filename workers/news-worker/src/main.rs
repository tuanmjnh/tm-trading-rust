use std::collections::VecDeque;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use chrono::Utc;
use thiserror::Error;
use tm_trading_agent_protocol::{SupervisorCommand, WORKER_CONFIGURATION_EXIT_CODE, WorkerMessage};
use tm_trading_domain::{
    NewNewsSource, NewsLicenseStatus, NewsRights, NewsTransport, NewsTrustTier,
};
use tm_trading_news_providers::HttpFeedFetcher;
use tm_trading_news_worker::{
    CycleResult, NewsWorker, NewsWorkerConfig, NewsWorkerError, run_news_cycle,
};
use tm_trading_storage::{
    MongoConnection, MongoNewsClusterRepository, MongoNewsItemRepository,
    MongoNewsSourceRepository, NewsSourceRepository, RepositoryError,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const DEFAULT_DATABASE_NAME: &str = "tm-trading-rust";
const MAX_PENDING_OPERATIONS: usize = 16;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(RunError::Configuration(error)) => {
            eprintln!("[news-worker] fatal configuration error: {error}");
            ExitCode::from(u8::try_from(WORKER_CONFIGURATION_EXIT_CODE).unwrap_or(78))
        }
        Err(error) => {
            eprintln!("[news-worker] fatal: {error}");
            ExitCode::from(2)
        }
    }
}

#[derive(Debug, Error)]
enum RunError {
    #[error("{0}")]
    Configuration(String),
    #[error("news worker I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

async fn run() -> Result<(), RunError> {
    let config =
        NewsWorkerConfig::from_env().map_err(|error| RunError::Configuration(error.to_string()))?;
    validate_startup(&config)?;

    let uri =
        std::env::var("TM_DATABASE_URI").unwrap_or_else(|_| "mongodb://127.0.0.1:27017".to_owned());
    let database_name =
        std::env::var("TM_DATABASE_NAME").unwrap_or_else(|_| DEFAULT_DATABASE_NAME.to_owned());
    let pid = std::process::id();
    let instance_id = std::env::var("TM_SERVICE_INSTANCE_ID")
        .unwrap_or_else(|_| format!("news-worker@local-agent:{pid}"));

    let connection = MongoConnection::connect(&uri, &database_name)
        .await
        .map_err(classify_repository_error)?;
    let sources = Arc::new(MongoNewsSourceRepository::new(connection.database()).await?);
    if let Ok(list) = sources.list().await
        && list.is_empty()
    {
        let seed = NewNewsSource {
            source_id: "coindesk-rss".to_owned(),
            name: "CoinDesk RSS".to_owned(),
            provider: Some("coindesk".to_owned()),
            transport: NewsTransport::Rss,
            feed_url: Some("https://www.coindesk.com/arc/outboundfeeds/rss/".to_owned()),
            trust_tier: NewsTrustTier::T1LicensedNews,
            market_groups: vec!["crypto".to_owned()],
            languages: vec!["en".to_owned()],
            license_status: NewsLicenseStatus::Ok,
            rights: NewsRights {
                allowed_store_metadata: true,
                allowed_display: true,
                allowed_summarize: true,
                allowed_training: false,
            },
            enabled: true,
            poll_interval_sec: 300,
            retention_days: 30,
            created_at: Utc::now(),
        };
        let _ = sources.upsert_seed(seed).await;
    }
    let items = Arc::new(MongoNewsItemRepository::new(connection.database()).await?);
    let clusters = Arc::new(MongoNewsClusterRepository::new(connection.database()).await?);
    let fetcher = Arc::new(
        HttpFeedFetcher::new(config.cycle_timeout)
            .map_err(|error| RunError::Configuration(error.to_string()))?,
    );

    let (command_tx, mut command_rx) = mpsc::unbounded_channel();
    thread::spawn(move || read_commands(command_tx));
    let (cycle_result_tx, mut cycle_result_rx) = mpsc::unbounded_channel();

    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut worker = NewsWorker::new(&instance_id, config);

    send(&mut output, &worker.hello(pid, Utc::now()))?;
    worker.record_dependencies_reachable(true);
    send(&mut output, &worker.heartbeat(Utc::now()))?;

    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + worker.config().heartbeat_interval,
        worker.config().heartbeat_interval,
    );
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut next_cycle = Instant::now();
    let mut active_cycle: Option<JoinHandle<()>> = None;
    let mut pending_operations: VecDeque<String> = VecDeque::new();

    loop {
        if active_cycle.is_none() {
            let operation_id = pending_operations.pop_front();
            if operation_id.is_some() || Instant::now() >= next_cycle {
                active_cycle = Some(spawn_cycle(
                    worker.config(),
                    Arc::clone(&sources),
                    Arc::clone(&items),
                    Arc::clone(&clusters),
                    Arc::clone(&fetcher),
                    operation_id,
                    cycle_result_tx.clone(),
                ));
                continue;
            }
        }

        tokio::select! {
            biased;
            Some((operation_id, result)) = cycle_result_rx.recv() => {
                if let Some(handle) = active_cycle.take() {
                    let _ = handle.await;
                }
                let succeeded = result.ok && result.skipped.is_none();
                if let Some(operation_id) = operation_id {
                    send(
                        &mut output,
                        &worker.operation_result(&operation_id, Utc::now(), result),
                    )?;
                } else {
                    send(&mut output, &worker.cycle(Utc::now(), result))?;
                }
                if succeeded
                    && let Some(ready) = worker.ready_after_success(Utc::now())
                {
                    send(&mut output, &ready)?;
                }
                next_cycle = Instant::now() + worker.config().cycle_interval;
            }
            command = command_rx.recv() => {
                match command {
                    Some(SupervisorCommand::Shutdown { reason, .. }) => {
                        send(&mut output, &worker.stopping(Utc::now(), &reason))?;
                        let drained = stop_active_cycle(&mut active_cycle).await && pending_operations.is_empty();
                        send(&mut output, &worker.stopped(Utc::now(), &reason, drained))?;
                        return Ok(());
                    }
                    Some(SupervisorCommand::RunOnce { operation_id }) => {
                        if pending_operations.len() >= MAX_PENDING_OPERATIONS {
                            let result = CycleResult::failure(
                                "too many news operations are already pending".to_owned(),
                            );
                            send(
                                &mut output,
                                &worker.operation_result(&operation_id, Utc::now(), result),
                            )?;
                        } else {
                            pending_operations.push_back(operation_id);
                        }
                    }
                    None => {
                        send(
                            &mut output,
                            &WorkerMessage::Fatal {
                                instance_id: instance_id.clone(),
                                at: Utc::now(),
                                error: "supervisor control channel closed".to_owned(),
                            },
                        )?;
                        stop_active_cycle(&mut active_cycle).await;
                        return Ok(());
                    }
                }
            }
            _ = heartbeat.tick() => {
                send(&mut output, &worker.heartbeat(Utc::now()))?;
            }
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(next_cycle)), if active_cycle.is_none() => {}
        }
    }
}

fn validate_startup(config: &NewsWorkerConfig) -> Result<(), RunError> {
    if !config.source_of_truth {
        return Err(RunError::Configuration(
            "TM_TRADING_NEWS_SOURCE_OF_TRUTH must be enabled before the news worker can write"
                .to_owned(),
        ));
    }
    Ok(())
}

fn classify_repository_error(error: RepositoryError) -> RunError {
    match &error {
        RepositoryError::InvalidDatabaseName
        | RepositoryError::TlsRequired
        | RepositoryError::InsecureTls => RunError::Configuration(error.to_string()),
        _ => RunError::Repository(error),
    }
}

fn spawn_cycle(
    config: &NewsWorkerConfig,
    sources: Arc<MongoNewsSourceRepository>,
    items: Arc<MongoNewsItemRepository>,
    clusters: Arc<MongoNewsClusterRepository>,
    fetcher: Arc<HttpFeedFetcher>,
    operation_id: Option<String>,
    result_tx: mpsc::UnboundedSender<(Option<String>, CycleResult)>,
) -> JoinHandle<()> {
    let config = config.clone();
    tokio::spawn(async move {
        let started = Instant::now();
        let result = match tokio::time::timeout(
            config.cycle_timeout,
            run_news_cycle(
                sources.as_ref(),
                items.as_ref(),
                clusters.as_ref(),
                fetcher.as_ref(),
                &config,
                Utc::now(),
            ),
        )
        .await
        {
            Ok(Ok(outcome)) => CycleResult::success(
                outcome,
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            ),
            Ok(Err(NewsWorkerError::NoEligibleSources)) => {
                CycleResult::skipped("no enabled RSS source is due for collection")
            }
            Ok(Err(error)) => CycleResult::failure(error.to_string()),
            Err(_) => {
                CycleResult::timeout("news collection cycle exceeded its deadline".to_owned())
            }
        };
        let _ = result_tx.send((operation_id, result));
    })
}

async fn stop_active_cycle(active_cycle: &mut Option<JoinHandle<()>>) -> bool {
    let Some(handle) = active_cycle.take() else {
        return true;
    };
    handle.abort();
    let _ = handle.await;
    false
}

fn read_commands(command_tx: mpsc::UnboundedSender<SupervisorCommand>) {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("[news-worker] failed to read control command: {error}");
                break;
            }
        };
        match serde_json::from_str::<SupervisorCommand>(&line) {
            Ok(command) => {
                if command_tx.send(command).is_err() {
                    break;
                }
            }
            Err(error) => {
                eprintln!("[news-worker] ignored invalid control command: {error}");
            }
        }
    }
}

fn send(output: &mut impl Write, message: &WorkerMessage) -> io::Result<()> {
    let line = serde_json::to_string(message)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    writeln!(output, "{line}")?;
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn aborting_an_active_cycle_is_not_reported_as_drained() {
        let mut active = Some(tokio::spawn(std::future::pending::<()>()));

        assert!(!stop_active_cycle(&mut active).await);
        assert!(active.is_none());
        assert!(stop_active_cycle(&mut None).await);
    }
}
