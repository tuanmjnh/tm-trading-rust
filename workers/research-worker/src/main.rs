use std::collections::VecDeque;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use chrono::Utc;
use thiserror::Error;
use tm_trading_agent_protocol::{SupervisorCommand, WORKER_CONFIGURATION_EXIT_CODE, WorkerMessage};
use tm_trading_research_worker::{
    CycleResult, ResearchWorker, ResearchWorkerConfig, run_research_cycle,
};
use tm_trading_storage::{
    MongoCandleRepository, MongoConnection, MongoResearchEventRepository, RepositoryError,
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
            eprintln!("[research-worker] fatal configuration error: {error}");
            ExitCode::from(u8::try_from(WORKER_CONFIGURATION_EXIT_CODE).unwrap_or(78))
        }
        Err(error) => {
            eprintln!("[research-worker] fatal: {error}");
            ExitCode::from(2)
        }
    }
}

#[derive(Debug, Error)]
enum RunError {
    #[error("{0}")]
    Configuration(String),
    #[error("research worker I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

async fn run() -> Result<(), RunError> {
    let config = ResearchWorkerConfig::from_env()
        .map_err(|error| RunError::Configuration(error.to_string()))?;
    validate_startup(&config)?;

    let uri = std::env::var("TM_DATABASE_URI").map_err(|_| {
        RunError::Configuration("TM_DATABASE_URI must be set for the research worker".to_owned())
    })?;
    let database_name =
        std::env::var("TM_DATABASE_NAME").unwrap_or_else(|_| DEFAULT_DATABASE_NAME.to_owned());
    let pid = std::process::id();
    let instance_id = std::env::var("TM_SERVICE_INSTANCE_ID")
        .unwrap_or_else(|_| format!("research-worker@local-agent:{pid}"));

    let connection = MongoConnection::connect(&uri, &database_name)
        .await
        .map_err(classify_repository_error)?;
    let candles = Arc::new(MongoCandleRepository::new(connection.database()).await?);
    let events = Arc::new(MongoResearchEventRepository::new(connection.database()).await?);

    let (command_tx, mut command_rx) = mpsc::unbounded_channel();
    thread::spawn(move || read_commands(command_tx));
    let (cycle_result_tx, mut cycle_result_rx) = mpsc::unbounded_channel();

    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut worker = ResearchWorker::new(&instance_id, config);

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
                    Arc::clone(&candles),
                    Arc::clone(&events),
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
                                "too many research operations are already pending".to_owned(),
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

fn validate_startup(config: &ResearchWorkerConfig) -> Result<(), RunError> {
    if !config.source_of_truth {
        return Err(RunError::Configuration(
            "TM_TRADING_RESEARCH_SOURCE_OF_TRUTH must be enabled before the research worker can write"
                .to_owned(),
        ));
    }
    if config.symbols.is_empty() {
        return Err(RunError::Configuration(
            "TM_TRADING_RESEARCH_SYMBOLS must list at least one symbol".to_owned(),
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
    config: &ResearchWorkerConfig,
    candles: Arc<MongoCandleRepository>,
    events: Arc<MongoResearchEventRepository>,
    operation_id: Option<String>,
    result_tx: mpsc::UnboundedSender<(Option<String>, CycleResult)>,
) -> JoinHandle<()> {
    let config = config.clone();
    tokio::spawn(async move {
        let result =
            run_research_cycle(candles.as_ref(), events.as_ref(), &config, Utc::now()).await;
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
                eprintln!("[research-worker] failed to read control command: {error}");
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
                eprintln!("[research-worker] ignored invalid control command: {error}");
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
