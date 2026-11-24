use std::collections::VecDeque;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use thiserror::Error;
use tm_trading_agent_protocol::{SupervisorCommand, WORKER_CONFIGURATION_EXIT_CODE, WorkerMessage};
use tm_trading_domain::{ProviderHealthState, ProviderInstance};
use tm_trading_market_providers::build_provider;
use tm_trading_market_worker::{
    CycleResult, MarketWorker, MarketWorkerConfig, MarketWorkerError, collect_closed_candles,
};
use tm_trading_storage::{
    CandleRepository, MongoCandleRepository, MongoConnection, MongoProviderInstanceRepository,
    ProviderInstanceRepository, RepositoryError,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const DEFAULT_DATABASE_NAME: &str = "tm-trading-rust";
const MAX_PROVIDER_ATTEMPTS: usize = 3;
const MAX_PENDING_OPERATIONS: usize = 16;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(RunError::Configuration(error)) => {
            eprintln!("[market-worker] fatal configuration error: {error}");
            ExitCode::from(u8::try_from(WORKER_CONFIGURATION_EXIT_CODE).unwrap_or(78))
        }
        Err(error) => {
            eprintln!("[market-worker] fatal: {error}");
            ExitCode::from(2)
        }
    }
}

#[derive(Debug, Error)]
enum RunError {
    #[error("{0}")]
    Configuration(String),
    #[error("market worker I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

async fn run() -> Result<(), RunError> {
    let config = MarketWorkerConfig::from_env()
        .map_err(|error| RunError::Configuration(error.to_string()))?;
    validate_startup(&config)?;

    let uri =
        std::env::var("TM_DATABASE_URI").unwrap_or_else(|_| "mongodb://127.0.0.1:27017".to_owned());
    let database_name =
        std::env::var("TM_DATABASE_NAME").unwrap_or_else(|_| DEFAULT_DATABASE_NAME.to_owned());
    let pid = std::process::id();
    let instance_id = std::env::var("TM_SERVICE_INSTANCE_ID")
        .unwrap_or_else(|_| format!("market-worker@local-agent:{pid}"));

    let connection = MongoConnection::connect(&uri, &database_name)
        .await
        .map_err(classify_repository_error)?;
    let candles = MongoCandleRepository::new(connection.database()).await?;
    let providers = MongoProviderInstanceRepository::new(connection.database()).await?;
    let mut instances = providers.list().await?;
    if instances.is_empty()
        && let Ok(instance) = providers
            .create(
                "binance",
                "Binance Public Market",
                "https://api.binance.com",
                5000,
                Utc::now(),
            )
            .await
    {
        instances.push(instance);
    }
    let candidates = provider_candidates(&instances, config.provider_definition_id.as_deref())
        .map_err(|error| RunError::Configuration(error.to_string()))?;
    if candidates.is_empty() {
        return Err(RunError::Configuration(
            "no enabled provider instance matches the market worker configuration".to_owned(),
        ));
    }

    let (command_tx, mut command_rx) = mpsc::unbounded_channel();
    thread::spawn(move || read_commands(command_tx));
    let (cycle_result_tx, mut cycle_result_rx) = mpsc::unbounded_channel();

    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut worker = MarketWorker::new(&instance_id, config);

    send(&mut output, &worker.hello(pid, Utc::now()))?;
    let verified = verify_provider(&providers, &candidates).await;
    worker.record_capability(verified);
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
                    providers.clone(),
                    candles.clone(),
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
                            let result = CycleResult::provider_failure(
                                "too many market operations are already pending".to_owned(),
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

fn validate_startup(config: &MarketWorkerConfig) -> Result<(), RunError> {
    if !config.source_of_truth {
        return Err(RunError::Configuration(
            "TM_TRADING_MARKET_SOURCE_OF_TRUTH must be enabled before the market worker can write"
                .to_owned(),
        ));
    }
    if config.symbols.is_empty() {
        return Err(RunError::Configuration(
            "TM_TRADING_MARKET_SYMBOLS must list at least one symbol".to_owned(),
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
    config: &MarketWorkerConfig,
    providers: MongoProviderInstanceRepository,
    candles: MongoCandleRepository,
    operation_id: Option<String>,
    result_tx: mpsc::UnboundedSender<(Option<String>, CycleResult)>,
) -> JoinHandle<()> {
    let config = config.clone();
    tokio::spawn(async move {
        let result = run_cycle(&config, &providers, &candles).await;
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

/// Probes the selected providers before the worker claims its capability.
///
/// A configured URL that only builds an HTTP client is not proof that the provider is
/// reachable. The bounded health request is the smallest harmless verification.
async fn verify_provider(
    providers: &dyn ProviderInstanceRepository,
    candidates: &[&ProviderInstance],
) -> bool {
    for instance in candidates {
        let provider = match build_provider(
            instance.provider_definition_id(),
            instance.base_url(),
            Duration::from_millis(instance.timeout_ms()),
        ) {
            Ok(provider) => provider,
            Err(error) => {
                record_health(providers, instance.id(), false, None, Some(error.code())).await;
                continue;
            }
        };
        match provider.health().await {
            Ok(health) => {
                record_health(
                    providers,
                    instance.id(),
                    true,
                    Some(health.latency_ms),
                    None,
                )
                .await;
                return true;
            }
            Err(error) => {
                record_health(providers, instance.id(), false, None, Some(error.code())).await;
            }
        }
    }
    false
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
enum ProviderSelectionError {
    #[error("no enabled provider instance is configured")]
    Missing,
    #[error("the enabled provider set is ambiguous; configure TM_TRADING_MARKET_PROVIDER_ID")]
    Ambiguous,
}

/// Selects a bounded provider fallback chain.
///
/// The repository list order is the configured priority within one health class. Healthy
/// instances are tried first, then unknown instances, then unhealthy instances. The chain
/// never crosses provider definitions because Binance and OKX symbols are not interchangeable.
fn provider_candidates<'a>(
    instances: &'a [ProviderInstance],
    provider_definition_id: Option<&str>,
) -> Result<Vec<&'a ProviderInstance>, ProviderSelectionError> {
    let mut enabled: Vec<&ProviderInstance> = instances
        .iter()
        .filter(|instance| instance.enabled())
        .collect();
    if let Some(wanted) = provider_definition_id {
        enabled.retain(|instance| instance.provider_definition_id() == wanted);
        if enabled.is_empty() {
            return Err(ProviderSelectionError::Missing);
        }
    } else {
        let Some(first) = enabled.first() else {
            return Err(ProviderSelectionError::Missing);
        };
        if enabled
            .iter()
            .any(|instance| instance.provider_definition_id() != first.provider_definition_id())
        {
            return Err(ProviderSelectionError::Ambiguous);
        }
    }

    enabled.sort_by_key(|instance| health_priority(instance.health_state()));
    enabled.truncate(MAX_PROVIDER_ATTEMPTS);
    Ok(enabled)
}

const fn health_priority(state: ProviderHealthState) -> u8 {
    match state {
        ProviderHealthState::Healthy => 0,
        ProviderHealthState::Unknown => 1,
        ProviderHealthState::Unhealthy => 2,
    }
}

/// Runs one bounded collection cycle and records the provider health it observed.
async fn run_cycle(
    config: &MarketWorkerConfig,
    providers: &dyn ProviderInstanceRepository,
    candles: &dyn CandleRepository,
) -> CycleResult {
    if !config.source_of_truth {
        return CycleResult::skipped(
            "the market worker is not the source of truth for candle writes",
        );
    }
    if config.symbols.is_empty() {
        return CycleResult::skipped("no market symbols are configured");
    }

    match tokio::time::timeout(
        config.cycle_timeout,
        run_cycle_inner(config, providers, candles),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => CycleResult::timeout(format!(
            "market collection cycle exceeded its {} ms deadline",
            u64::try_from(config.cycle_timeout.as_millis()).unwrap_or(u64::MAX)
        )),
    }
}

async fn run_cycle_inner(
    config: &MarketWorkerConfig,
    providers: &dyn ProviderInstanceRepository,
    candles: &dyn CandleRepository,
) -> CycleResult {
    let instances = match providers.list().await {
        Ok(instances) => instances,
        Err(error) => return CycleResult::storage_failure(error.to_string()),
    };
    let candidates = match provider_candidates(&instances, config.provider_definition_id.as_deref())
    {
        Ok(candidates) => candidates,
        Err(error) => return CycleResult::skipped(&error.to_string()),
    };
    if candidates.is_empty() {
        return CycleResult::skipped("no enabled provider instance is configured");
    }

    let mut last_provider_error = None;
    let mut empty_response = false;
    for (attempt, instance) in candidates.iter().enumerate() {
        let provider = match build_provider(
            instance.provider_definition_id(),
            instance.base_url(),
            Duration::from_millis(instance.timeout_ms()),
        ) {
            Ok(provider) => provider,
            Err(error) => {
                last_provider_error = Some(error.to_string());
                record_health(providers, instance.id(), false, None, Some(error.code())).await;
                continue;
            }
        };

        match collect_closed_candles(
            provider.as_ref(),
            candles,
            config,
            Utc::now().timestamp_millis(),
        )
        .await
        {
            Ok(outcome) => {
                record_health(
                    providers,
                    instance.id(),
                    true,
                    Some(outcome.provider_ms),
                    None,
                )
                .await;
                let mut result = CycleResult::success(outcome);
                let fallback = if attempt == 0 {
                    String::new()
                } else {
                    format!(" after {attempt} fallback attempt(s)")
                };
                result.summary = Some(format!(
                    "stored {} bar(s) using provider instance '{}'{fallback}",
                    result.output_count.unwrap_or_default(),
                    instance.id()
                ));
                return result;
            }
            Err(MarketWorkerError::Provider(error)) => {
                last_provider_error = Some(error.to_string());
                record_health(providers, instance.id(), false, None, Some(error.code())).await;
            }
            Err(MarketWorkerError::EmptyResponse { .. }) => {
                empty_response = true;
                record_health(
                    providers,
                    instance.id(),
                    false,
                    None,
                    Some("invalid_payload"),
                )
                .await;
            }
            Err(MarketWorkerError::Storage(error)) => {
                return CycleResult::storage_failure(error.to_string());
            }
            Err(MarketWorkerError::WriteAuthorityDisabled) => {
                return CycleResult::skipped(
                    "the market worker is not the source of truth for candle writes",
                );
            }
        }
    }

    if empty_response && last_provider_error.is_none() {
        CycleResult::skipped("the provider returned no closed candles")
    } else {
        CycleResult::provider_failure(
            last_provider_error.unwrap_or_else(|| "all provider instances failed".to_owned()),
        )
    }
}

/// Writes the observed provider health; a failed health write is logged, not fatal.
async fn record_health(
    providers: &dyn ProviderInstanceRepository,
    provider_instance_id: &str,
    healthy: bool,
    latency_ms: Option<u64>,
    error_code: Option<&str>,
) {
    if let Err(error) = providers
        .record_health(
            provider_instance_id,
            Utc::now(),
            healthy,
            latency_ms,
            error_code,
        )
        .await
    {
        eprintln!("[market-worker] failed to record provider health: {error}");
    }
}

fn read_commands(command_tx: mpsc::UnboundedSender<SupervisorCommand>) {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("[market-worker] failed to read control command: {error}");
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
                eprintln!("[market-worker] ignored invalid control command: {error}");
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
    use chrono::{TimeZone, Utc};
    use tm_trading_domain::NewProviderInstance;
    use tm_trading_storage::{InMemoryCandleRepository, InMemoryProviderInstanceRepository};

    fn spawn_provider(response: String) -> (String, thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind provider");
        let address = listener.local_addr().expect("read provider address");
        let server = thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut stream, _) = listener.accept().expect("accept provider request");
            let mut request = [0_u8; 2_048];
            let _ = stream.read(&mut request);
            stream
                .write_all(response.as_bytes())
                .expect("write provider response");
        });
        (format!("http://{address}"), server)
    }

    fn instance(id: &str, definition: &str, enabled: bool) -> ProviderInstance {
        let at = Utc.with_ymd_and_hms(2026, 11, 20, 0, 0, 0).unwrap();
        ProviderInstance::new(NewProviderInstance {
            id: id.to_owned(),
            provider_definition_id: definition.to_owned(),
            display_name: definition.to_owned(),
            base_url: "https://api.binance.com".to_owned(),
            timeout_ms: 5_000,
            created_at: at,
        })
        .expect("valid provider")
        .with_settings(definition, "https://api.binance.com", 5_000, enabled, at)
        .expect("valid settings")
    }

    fn config() -> MarketWorkerConfig {
        MarketWorkerConfig {
            symbols: vec!["BTCUSDT".to_owned()],
            timeframe: "1m".to_owned(),
            bars: 100,
            cycle_interval: Duration::from_secs(60),
            heartbeat_interval: Duration::from_secs(5),
            cycle_timeout: Duration::from_secs(30),
            provider_definition_id: None,
            source_of_truth: true,
        }
    }

    #[test]
    fn an_explicit_definition_selects_only_matching_instances() {
        let instances = vec![
            instance("one", "binance", true),
            instance("two", "okx", true),
        ];

        let selected = provider_candidates(&instances, Some("okx")).expect("selected");

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id(), "two");
    }

    #[test]
    fn an_ambiguous_provider_set_is_not_selected_implicitly() {
        let instances = vec![
            instance("one", "binance", true),
            instance("two", "okx", true),
        ];

        assert_eq!(
            provider_candidates(&instances, None),
            Err(ProviderSelectionError::Ambiguous)
        );
    }

    #[test]
    fn disabled_instances_are_ignored() {
        let instances = vec![
            instance("one", "binance", false),
            instance("two", "binance", true),
        ];

        let selected = provider_candidates(&instances, None).expect("selected");

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id(), "two");
    }

    #[test]
    fn healthy_instances_are_tried_before_unhealthy_instances() {
        let unhealthy = instance("one", "binance", true).with_health(
            Utc.with_ymd_and_hms(2026, 11, 20, 0, 0, 0).unwrap(),
            false,
            None,
            Some("timeout"),
        );
        let healthy = instance("two", "binance", true).with_health(
            Utc.with_ymd_and_hms(2026, 11, 20, 0, 1, 0).unwrap(),
            true,
            Some(10),
            None,
        );

        let instances = [unhealthy, healthy];
        let selected = provider_candidates(&instances, Some("binance")).expect("selected");

        assert_eq!(selected[0].id(), "two");
    }

    #[test]
    fn startup_rejects_missing_write_authority_and_symbols() {
        let mut settings = config();
        settings.source_of_truth = false;
        assert!(matches!(
            validate_startup(&settings),
            Err(RunError::Configuration(_))
        ));

        let mut settings = config();
        settings.symbols.clear();
        assert!(matches!(
            validate_startup(&settings),
            Err(RunError::Configuration(_))
        ));
    }

    #[test]
    fn provider_fallback_is_bounded_and_preserves_configured_order() {
        let instances = (0..5)
            .map(|index| instance(&format!("instance-{index}"), "binance", true))
            .collect::<Vec<_>>();

        let selected = provider_candidates(&instances, Some("binance")).expect("selected");

        assert_eq!(selected.len(), MAX_PROVIDER_ATTEMPTS);
        assert_eq!(selected[0].id(), "instance-0");
        assert_eq!(selected[1].id(), "instance-1");
        assert_eq!(selected[2].id(), "instance-2");
    }

    #[tokio::test]
    async fn a_failed_provider_falls_back_to_the_next_configured_instance() {
        let (failed_url, failed_server) = spawn_provider(
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n".to_owned(),
        );
        let (healthy_url, healthy_server) = spawn_provider(
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n[[\"1700000000000\",\"100.5\",\"110.0\",\"90.0\",\"105.25\",\"12.5\",\"1700000059999\",\"0\",\"42\",\"0\",\"0\",\"0\"]]"
                .to_owned(),
        );
        let providers = InMemoryProviderInstanceRepository::new();
        let failed = providers
            .create("binance", "Failed Binance", &failed_url, 5_000, Utc::now())
            .await
            .unwrap();
        let healthy = providers
            .create(
                "binance",
                "Healthy Binance",
                &healthy_url,
                5_000,
                Utc::now(),
            )
            .await
            .unwrap();
        let candles = InMemoryCandleRepository::new();
        let mut settings = config();
        settings.provider_definition_id = Some("binance".to_owned());

        let result = run_cycle(&settings, &providers, &candles).await;
        failed_server.join().expect("join failed provider");
        healthy_server.join().expect("join healthy provider");

        assert!(result.ok, "fallback cycle should succeed: {result:?}");
        assert!(
            result.summary.as_deref().is_some_and(
                |summary| summary.contains(healthy.id()) && summary.contains("fallback")
            )
        );
        assert_eq!(candles.recent("BTCUSDT", "1m", 10).await.unwrap().len(), 1);
        assert_eq!(
            providers
                .get(failed.id())
                .await
                .unwrap()
                .unwrap()
                .health_state(),
            ProviderHealthState::Unhealthy
        );
        assert_eq!(
            providers
                .get(healthy.id())
                .await
                .unwrap()
                .unwrap()
                .health_state(),
            ProviderHealthState::Healthy
        );
    }

    #[tokio::test]
    async fn an_overdue_cycle_is_cancelled_and_reported_as_not_ready() {
        use std::io::Write;
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind delayed provider");
        let address = listener.local_addr().expect("read provider address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept provider request");
            thread::sleep(Duration::from_millis(200));
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]");
        });
        let providers = InMemoryProviderInstanceRepository::new();
        providers
            .create(
                "binance",
                "Delayed Binance",
                &format!("http://{address}"),
                5_000,
                Utc::now(),
            )
            .await
            .unwrap();
        let candles = InMemoryCandleRepository::new();
        let mut settings = config();
        settings.provider_definition_id = Some("binance".to_owned());
        settings.cycle_timeout = Duration::from_millis(25);

        let result = run_cycle(&settings, &providers, &candles).await;
        server.join().expect("join delayed provider");

        assert!(!result.ok);
        assert!(
            result
                .error
                .as_deref()
                .is_some_and(|error| error.contains("deadline"))
        );
        assert_eq!(result.dependencies_reachable, Some(false));
        assert_eq!(result.sink_writable, Some(false));
    }

    #[tokio::test]
    async fn aborting_an_active_cycle_is_not_reported_as_drained() {
        let mut active = Some(tokio::spawn(std::future::pending::<()>()));

        assert!(!stop_active_cycle(&mut active).await);
        assert!(active.is_none());
        assert!(stop_active_cycle(&mut None).await);
    }

    #[tokio::test]
    async fn disabled_write_authority_does_not_touch_candles_or_provider_health() {
        let providers = InMemoryProviderInstanceRepository::new();
        let created = providers
            .create(
                "binance",
                "Binance Public",
                "https://api.binance.com",
                5_000,
                Utc::now(),
            )
            .await
            .unwrap();
        let candles = InMemoryCandleRepository::new();
        let mut settings = config();
        settings.source_of_truth = false;

        let result = run_cycle(&settings, &providers, &candles).await;

        assert!(!result.ok);
        assert!(result.skipped.is_some());
        assert!(
            candles
                .recent("BTCUSDT", "1m", 10)
                .await
                .unwrap()
                .is_empty()
        );
        let stored = providers.get(created.id()).await.unwrap().unwrap();
        assert_eq!(stored.health_state(), ProviderHealthState::Unknown);
    }
}
