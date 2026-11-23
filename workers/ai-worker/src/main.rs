//! AI worker executable: publishes evidence-bound briefings from durable AI jobs.
//!
//! The worker owns no scheduling of its own. It claims one durable job at a time,
//! assembles owner-scoped evidence, runs the configured route, validates the model
//! output, and atomically publishes the briefing while completing the job.

use std::io::{self, BufRead, Write};
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use chrono::Utc;
use thiserror::Error;
use tm_trading_agent_protocol::{SupervisorCommand, WORKER_CONFIGURATION_EXIT_CODE, WorkerMessage};
use tm_trading_ai_worker::{
    AiCapabilityProbeError, AiJobFailure, AiJobOutcome, AiJobProcessor, AiProviderResolver,
    AiRouteError, AiRouteExecutor, BriefingEvidenceAssembler, EnvironmentAiProviderResolver,
    probe_research_explanation_capability,
};
use tm_trading_storage::{
    MongoAiJobRepository, MongoAiModelRepository, MongoAiProviderProfileRepository,
    MongoAiTaskRouteRepository, MongoConnection, MongoNewsItemRepository,
    MongoResearchEventRepository, RepositoryError,
};
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};

const DEFAULT_DATABASE_NAME: &str = "tm-trading-rust";
/// Idle delay between two claim attempts.
const DEFAULT_CLAIM_INTERVAL: Duration = Duration::from_secs(5);
/// Delay between two heartbeats.
const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
/// Minimum delay between failed billable readiness probes.
const DEFAULT_CAPABILITY_REVALIDATION_INTERVAL: Duration = Duration::from_secs(60);
/// Complete worker startup budget, including MongoDB setup and the first readiness probe.
const WORKER_STARTUP_BUDGET: Duration = Duration::from_secs(110);
/// Complete budget for a later readiness revalidation.
const READINESS_PROBE_BUDGET: Duration = Duration::from_secs(90);
/// Maximum time allowed for the MongoDB transaction and briefing-sink checks.
const READINESS_STORAGE_TIMEOUT: Duration = Duration::from_secs(15);
/// Maximum time allowed for the complete route fallback chain.
const READINESS_ROUTE_TIMEOUT: Duration = Duration::from_secs(75);
/// Lease taken on a claimed job. It is longer than the longest route attempt chain.
const DEFAULT_LEASE: Duration = Duration::from_secs(300);

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(RunError::Configuration(error)) => {
            eprintln!("[ai-worker] fatal configuration error: {error}");
            ExitCode::from(u8::try_from(WORKER_CONFIGURATION_EXIT_CODE).unwrap_or(78))
        }
        Err(error) => {
            eprintln!("[ai-worker] fatal: {error}");
            ExitCode::from(2)
        }
    }
}

#[derive(Debug, Error)]
enum RunError {
    #[error("{0}")]
    Configuration(String),
    #[error("AI worker startup budget exceeded")]
    StartupTimeout,
    #[error("ai worker I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

async fn run() -> Result<(), RunError> {
    let startup_deadline = tokio::time::Instant::now() + WORKER_STARTUP_BUDGET;
    let uri = std::env::var("TM_DATABASE_URI").map_err(|_| {
        RunError::Configuration("TM_DATABASE_URI must be set for the AI worker".to_owned())
    })?;
    let database_name =
        std::env::var("TM_DATABASE_NAME").unwrap_or_else(|_| DEFAULT_DATABASE_NAME.to_owned());
    if !enabled("TM_TRADING_AI_WORKER_ENABLED") {
        return Err(RunError::Configuration(
            "TM_TRADING_AI_WORKER_ENABLED must be enabled before the AI worker can write briefings"
                .to_owned(),
        ));
    }
    let claim_interval = duration_from_env("TM_TRADING_AI_CLAIM_MS", DEFAULT_CLAIM_INTERVAL)?;
    let heartbeat_interval =
        duration_from_env("TM_TRADING_AI_HEARTBEAT_MS", DEFAULT_HEARTBEAT_INTERVAL)?;
    let lease = duration_from_env("TM_TRADING_AI_LEASE_MS", DEFAULT_LEASE)?;

    let pid = std::process::id();
    let instance_id = std::env::var("TM_SERVICE_INSTANCE_ID")
        .unwrap_or_else(|_| format!("ai-worker@local-agent:{pid}"));

    let (connection, jobs, research, news, routes, providers, models) =
        run_with_startup_deadline(startup_deadline, async {
            let connection = Arc::new(MongoConnection::connect(&uri, &database_name).await?);
            let database = connection.database();

            let jobs = Arc::new(MongoAiJobRepository::new(database).await?);
            let research = Arc::new(MongoResearchEventRepository::new(database).await?);
            let news = Arc::new(MongoNewsItemRepository::new(database).await?);
            let routes = Arc::new(MongoAiTaskRouteRepository::new(database).await?);
            let providers = Arc::new(MongoAiProviderProfileRepository::new(database).await?);
            let models = Arc::new(MongoAiModelRepository::new(database).await?);

            Ok::<_, RepositoryError>((connection, jobs, research, news, routes, providers, models))
        })
        .await?;

    let resolver = Arc::new(EnvironmentAiProviderResolver);
    let executor = AiRouteExecutor::new(
        Arc::clone(&routes) as Arc<dyn tm_trading_storage::AiTaskRouteRepository>,
        Arc::clone(&providers) as Arc<dyn tm_trading_storage::AiProviderProfileRepository>,
        Arc::clone(&models) as Arc<dyn tm_trading_storage::AiModelRepository>,
        Arc::clone(&resolver) as Arc<dyn AiProviderResolver>,
    );
    let assembler = BriefingEvidenceAssembler::new(research, news);
    let processor = AiJobProcessor::new(
        Arc::clone(&jobs) as Arc<dyn tm_trading_storage::AiJobRepository>,
        assembler,
        executor.clone(),
        tm_trading_ai_worker::DEFAULT_EVIDENCE_MAX_AGE,
    );
    let (job_result_tx, mut job_result_rx) = mpsc::unbounded_channel();

    let (command_tx, mut command_rx) = tokio::sync::mpsc::unbounded_channel();
    thread::spawn(move || read_commands(command_tx));

    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut worker = Worker::new(&instance_id);

    send(&mut output, &worker.hello(pid))?;
    send(&mut output, &worker.heartbeat())?;

    let mut probes = JoinSet::new();
    probes.spawn(probe_capability(
        Arc::clone(&connection),
        Arc::clone(&jobs),
        executor.clone(),
        startup_deadline,
    ));

    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + heartbeat_interval,
        heartbeat_interval,
    );
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut claim =
        tokio::time::interval_at(tokio::time::Instant::now() + claim_interval, claim_interval);
    claim.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut capability_retry = tokio::time::interval_at(
        tokio::time::Instant::now() + DEFAULT_CAPABILITY_REVALIDATION_INTERVAL,
        DEFAULT_CAPABILITY_REVALIDATION_INTERVAL,
    );
    capability_retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut active_job: Option<JoinHandle<()>> = None;

    loop {
        tokio::select! {
            biased;
            Some(result) = job_result_rx.recv() => {
                if let Some(handle) = active_job.take() {
                    let _ = handle.await;
                }
                match result {
                    Ok(AiJobOutcome::Idle) => {}
                    Ok(AiJobOutcome::Published { job_id, briefing_id }) => {
                        worker.record_success();
                        send(
                            &mut output,
                            &worker.published(&job_id.to_string(), &briefing_id),
                        )?;
                    }
                    Ok(AiJobOutcome::Failed { job_id, failure }) => {
                        worker.record_job_failure(failure);
                        send(
                            &mut output,
                            &worker.failed(&job_id.to_string(), failure.code()),
                        )?;
                    }
                    Ok(AiJobOutcome::OutcomeUnknown {
                        job_id,
                        observed_failure,
                    }) => {
                        worker.record_unknown_outcome(observed_failure);
                        send(&mut output, &worker.unknown(&job_id.to_string()))?;
                    }
                    Err(error) => {
                        worker.record_dependency_failure();
                        eprintln!("[ai-worker] job outcome is unknown: {error}");
                    }
                }
            }
            Some(result) = probes.join_next() => {
                match result {
                    Ok(Ok(())) => {
                        worker.record_capability(true);
                        send(&mut output, &worker.ready())?;
                    }
                    Ok(Err(error)) => {
                        schedule_capability_retry(
                            &mut capability_retry,
                            tokio::time::Instant::now(),
                        );
                        worker.record_probe_failure(&error);
                        eprintln!("[ai-worker] readiness probe failed: {error}");
                        send(&mut output, &worker.heartbeat())?;
                    }
                    Err(error) => {
                        schedule_capability_retry(
                            &mut capability_retry,
                            tokio::time::Instant::now(),
                        );
                        worker.record_dependency_failure();
                        eprintln!("[ai-worker] readiness probe task failed: {error}");
                        send(&mut output, &worker.heartbeat())?;
                    }
                }
            }
            command = command_rx.recv() => {
                match command {
                    Some(SupervisorCommand::Shutdown { reason, .. }) => {
                        send(&mut output, &worker.stopping(&reason))?;
                        let drained = stop_active_work(&mut active_job, &mut probes).await;
                        send(&mut output, &worker.stopped(&reason, drained))?;
                        return Ok(());
                    }
                    Some(SupervisorCommand::RunOnce { .. }) => {
                        eprintln!("[ai-worker] run-once is not supported; jobs are claimed continuously");
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
                        let drained = stop_active_work(&mut active_job, &mut probes).await;
                        if !drained {
                            eprintln!("[ai-worker] shutdown ended with an unobserved outcome");
                        }
                        return Ok(());
                    }
                }
            }
            _ = capability_retry.tick() => {
                if active_job.is_none()
                    && !worker.capability_verified()
                    && probes.is_empty()
                {
                    probes.spawn(probe_capability(
                        Arc::clone(&connection),
                        Arc::clone(&jobs),
                        executor.clone(),
                        tokio::time::Instant::now() + READINESS_PROBE_BUDGET,
                    ));
                }
            }
            _ = claim.tick() => {
                if active_job.is_some() {
                    continue;
                }
                if !worker.capability_verified() {
                    continue;
                }
                active_job = Some(spawn_job(
                    processor.clone(),
                    instance_id.clone(),
                    lease,
                    job_result_tx.clone(),
                ));
            }
            _ = heartbeat.tick() => {
                send(&mut output, &worker.heartbeat())?;
            }
        }
    }
}

async fn run_with_startup_deadline<T>(
    deadline: tokio::time::Instant,
    operation: impl std::future::Future<Output = Result<T, RepositoryError>>,
) -> Result<T, RunError> {
    match tokio::time::timeout_at(deadline, operation).await {
        Ok(result) => result.map_err(classify_repository_error),
        Err(_) => Err(RunError::StartupTimeout),
    }
}

fn stage_deadline(
    overall_deadline: tokio::time::Instant,
    stage_started_at: tokio::time::Instant,
    budget: Duration,
) -> tokio::time::Instant {
    std::cmp::min(overall_deadline, stage_started_at + budget)
}

fn schedule_capability_retry(
    retry: &mut tokio::time::Interval,
    completed_at: tokio::time::Instant,
) {
    retry.reset_at(completed_at + DEFAULT_CAPABILITY_REVALIDATION_INTERVAL);
}

#[derive(Debug, Error)]
enum CapabilityProbeError {
    #[error("dependency probe failed: {0}")]
    Dependency(String),
    #[error("{0}")]
    Capability(String),
}

/// Verifies that the configured route can produce a schema-valid response.
///
/// This is one bounded billable route execution. It runs in a cancellable task so
/// heartbeat and shutdown handling remain responsive while MongoDB or the provider is slow.
async fn probe_capability(
    connection: Arc<MongoConnection>,
    jobs: Arc<MongoAiJobRepository>,
    executor: AiRouteExecutor,
    deadline: tokio::time::Instant,
) -> Result<(), CapabilityProbeError> {
    let storage_deadline = stage_deadline(
        deadline,
        tokio::time::Instant::now(),
        READINESS_STORAGE_TIMEOUT,
    );
    tokio::time::timeout_at(
        storage_deadline,
        probe_storage_capabilities(connection.as_ref(), jobs.as_ref()),
    )
    .await
    .map_err(|_| {
        CapabilityProbeError::Dependency("MongoDB readiness probe timed out".to_owned())
    })??;

    let route_deadline = stage_deadline(
        deadline,
        tokio::time::Instant::now(),
        READINESS_ROUTE_TIMEOUT,
    );
    run_bounded_probe(
        route_deadline,
        probe_research_explanation_capability(&executor),
    )
    .await
    .map_err(classify_capability_probe_error)
}

async fn probe_storage_capabilities(
    connection: &MongoConnection,
    jobs: &MongoAiJobRepository,
) -> Result<(), CapabilityProbeError> {
    let transactions_supported = connection
        .transactions_supported()
        .await
        .map_err(|error| CapabilityProbeError::Dependency(error.to_string()))?;
    ensure_transaction_support(transactions_supported)?;
    jobs.probe_briefing_sink()
        .await
        .map_err(|error| CapabilityProbeError::Dependency(error.to_string()))
}

async fn run_bounded_probe(
    deadline: tokio::time::Instant,
    probe: impl std::future::Future<Output = Result<(), AiCapabilityProbeError>>,
) -> Result<(), AiCapabilityProbeError> {
    match tokio::time::timeout_at(deadline, probe).await {
        Ok(result) => result,
        Err(_) => Err(AiCapabilityProbeError::Timeout),
    }
}

fn classify_capability_probe_error(error: AiCapabilityProbeError) -> CapabilityProbeError {
    match error {
        AiCapabilityProbeError::Route(AiRouteError::Repository(error)) => {
            CapabilityProbeError::Dependency(error.to_string())
        }
        AiCapabilityProbeError::Route(AiRouteError::Provider(error)) => {
            CapabilityProbeError::Capability(error.code().to_owned())
        }
        AiCapabilityProbeError::Route(error) => CapabilityProbeError::Capability(error.to_string()),
        AiCapabilityProbeError::InvalidResponse => {
            CapabilityProbeError::Capability("invalid_model_output".to_owned())
        }
        AiCapabilityProbeError::Timeout => {
            CapabilityProbeError::Dependency("route readiness probe timed out".to_owned())
        }
    }
}

fn ensure_transaction_support(supported: bool) -> Result<(), CapabilityProbeError> {
    if supported {
        Ok(())
    } else {
        Err(CapabilityProbeError::Dependency(
            "MongoDB deployment must support multi-document transactions".to_owned(),
        ))
    }
}

fn spawn_job(
    processor: AiJobProcessor,
    worker_id: String,
    lease: Duration,
    result_tx: mpsc::UnboundedSender<Result<AiJobOutcome, RepositoryError>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let result = processor.process_next(&worker_id, Utc::now(), lease).await;
        let _ = result_tx.send(result);
    })
}

/// Aborts an in-flight job when the supervisor asks the worker to stop.
///
/// The aborted lease is left to expire as an unknown outcome; reporting it as
/// drained would claim that the provider action was observed when it was not.
async fn stop_active_job(active_job: &mut Option<JoinHandle<()>>) -> bool {
    let Some(handle) = active_job.take() else {
        return true;
    };
    handle.abort();
    let _ = handle.await;
    false
}

/// Cancels any in-flight readiness probe before the worker exits.
async fn stop_active_probe(probes: &mut JoinSet<Result<(), CapabilityProbeError>>) -> bool {
    let mut drained = true;
    probes.abort_all();
    while let Some(result) = probes.join_next().await {
        if result.is_err() {
            drained = false;
        }
    }
    drained
}

async fn stop_active_work(
    active_job: &mut Option<JoinHandle<()>>,
    probes: &mut JoinSet<Result<(), CapabilityProbeError>>,
) -> bool {
    let job_drained = stop_active_job(active_job).await;
    let probe_drained = stop_active_probe(probes).await;
    job_drained && probe_drained
}

fn enabled(name: &str) -> bool {
    matches!(
        std::env::var(name)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn duration_from_env(name: &str, default: Duration) -> Result<Duration, RunError> {
    let value = std::env::var(name).unwrap_or_default();
    let value = value.trim();
    if value.is_empty() {
        return Ok(default);
    }
    let millis = value.parse::<u64>().map_err(|_| {
        RunError::Configuration(format!("{name} must be a whole number of milliseconds"))
    })?;
    if millis == 0 {
        return Err(RunError::Configuration(format!(
            "{name} must be greater than zero"
        )));
    }
    Ok(Duration::from_millis(millis))
}

fn classify_repository_error(error: RepositoryError) -> RunError {
    match &error {
        RepositoryError::InvalidDatabaseName
        | RepositoryError::TlsRequired
        | RepositoryError::InsecureTls => RunError::Configuration(error.to_string()),
        _ => RunError::Repository(error),
    }
}

fn read_commands(command_tx: tokio::sync::mpsc::UnboundedSender<SupervisorCommand>) {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                eprintln!("[ai-worker] failed to read control command: {error}");
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
                eprintln!("[ai-worker] ignored invalid control command: {error}");
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

/// Protocol state of the AI worker.
struct Worker {
    instance_id: String,
    executable_path: String,
    evidence: tm_trading_agent_protocol::WorkerEvidence,
}

impl Worker {
    fn new(instance_id: &str) -> Self {
        let executable_path = std::env::current_exe()
            .ok()
            .and_then(|path| std::fs::canonicalize(path).ok())
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            instance_id: instance_id.to_owned(),
            executable_path,
            evidence: tm_trading_agent_protocol::WorkerEvidence::default(),
        }
    }

    fn capability_verified(&self) -> bool {
        self.evidence.capability_verified == Some(true)
    }

    fn record_capability(&mut self, verified: bool) {
        self.evidence.capability_verified = Some(verified);
        if verified {
            self.evidence.dependencies_reachable = Some(true);
            self.evidence.sink_writable = Some(true);
        }
    }

    fn record_probe_failure(&mut self, error: &CapabilityProbeError) {
        if matches!(error, CapabilityProbeError::Dependency(_)) {
            self.record_dependency_failure();
        } else {
            self.record_capability_failure();
        }
    }

    fn record_capability_failure(&mut self) {
        self.evidence.capability_verified = Some(false);
    }

    fn record_dependency_failure(&mut self) {
        self.evidence.dependencies_reachable = Some(false);
        self.evidence.sink_writable = Some(false);
        self.evidence.capability_verified = Some(false);
    }

    fn record_success(&mut self) {
        let now = Utc::now();
        self.evidence.last_success_at = Some(now);
        self.evidence.last_output_count = Some(1);
        self.evidence.last_event_at = Some(now);
        self.evidence.last_job_validated = Some(true);
    }

    fn record_job_failure(&mut self, failure: AiJobFailure) {
        self.evidence.last_job_validated = Some(false);
        match failure {
            AiJobFailure::ProviderFailed
            | AiJobFailure::RouteUnavailable
            | AiJobFailure::InvalidModelOutput
            | AiJobFailure::InvalidBriefing => self.record_capability_failure(),
            AiJobFailure::DependencyUnavailable | AiJobFailure::PublishFailed => {
                self.record_dependency_failure();
            }
            _ => {}
        }
    }

    fn record_unknown_outcome(&mut self, observed_failure: Option<AiJobFailure>) {
        if let Some(failure) = observed_failure {
            self.record_job_failure(failure);
        }
    }

    fn hello(&self, pid: u32) -> WorkerMessage {
        WorkerMessage::Hello {
            instance_id: self.instance_id.clone(),
            at: Utc::now(),
            pid,
            executable_path: self.executable_path.clone(),
            executable_version: env!("CARGO_PKG_VERSION").to_owned(),
            schema_version: tm_trading_agent_protocol::WORKER_SCHEMA_VERSION.to_owned(),
        }
    }

    fn ready(&self) -> WorkerMessage {
        WorkerMessage::Ready {
            instance_id: self.instance_id.clone(),
            at: Utc::now(),
            evidence: self.evidence.clone(),
        }
    }

    fn heartbeat(&self) -> WorkerMessage {
        WorkerMessage::Heartbeat {
            instance_id: self.instance_id.clone(),
            at: Utc::now(),
            evidence: self.evidence.clone(),
        }
    }

    fn published(&self, job_id: &str, briefing_id: &str) -> WorkerMessage {
        WorkerMessage::Cycle {
            instance_id: self.instance_id.clone(),
            at: Utc::now(),
            ok: true,
            skipped: None,
            duration_ms: 0,
            output_count: Some(1),
            evidence: self.evidence.clone(),
            error: None,
            summary: Some(format!("published briefing {briefing_id} for job {job_id}")),
        }
    }

    fn failed(&self, job_id: &str, error_code: &str) -> WorkerMessage {
        WorkerMessage::Cycle {
            instance_id: self.instance_id.clone(),
            at: Utc::now(),
            ok: false,
            skipped: None,
            duration_ms: 0,
            output_count: Some(0),
            evidence: self.evidence.clone(),
            error: Some(format!("job {job_id} failed: {error_code}")),
            summary: None,
        }
    }

    fn unknown(&self, job_id: &str) -> WorkerMessage {
        WorkerMessage::Cycle {
            instance_id: self.instance_id.clone(),
            at: Utc::now(),
            ok: false,
            skipped: Some("outcome_unknown".to_owned()),
            duration_ms: 0,
            output_count: Some(0),
            evidence: self.evidence.clone(),
            error: None,
            summary: Some(format!("job {job_id} outcome unknown")),
        }
    }

    fn stopping(&self, reason: &str) -> WorkerMessage {
        WorkerMessage::Stopping {
            instance_id: self.instance_id.clone(),
            at: Utc::now(),
            reason: reason.to_owned(),
        }
    }

    fn stopped(&self, reason: &str, drained: bool) -> WorkerMessage {
        WorkerMessage::Stopped {
            instance_id: self.instance_id.clone(),
            at: Utc::now(),
            reason: reason.to_owned(),
            drained,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_startup_path_uses_one_agent_safe_deadline() {
        const AGENT_STARTUP_DEADLINE: Duration = Duration::from_secs(120);
        let started = tokio::time::Instant::now();
        let startup_deadline = started + WORKER_STARTUP_BUDGET;
        let setup_finished = started + Duration::from_secs(31);

        let storage_deadline =
            stage_deadline(startup_deadline, setup_finished, READINESS_STORAGE_TIMEOUT);
        let route_deadline =
            stage_deadline(startup_deadline, storage_deadline, READINESS_ROUTE_TIMEOUT);

        assert_eq!(route_deadline, startup_deadline);
        assert!(WORKER_STARTUP_BUDGET < AGENT_STARTUP_DEADLINE);
    }

    #[tokio::test]
    async fn connection_and_repository_initialization_are_bounded() {
        let result = run_with_startup_deadline(
            tokio::time::Instant::now() + Duration::from_millis(1),
            std::future::pending::<Result<(), RepositoryError>>(),
        )
        .await;

        assert!(matches!(result, Err(RunError::StartupTimeout)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_probe_is_retried_only_after_the_cooldown_from_completion() {
        let started = tokio::time::Instant::now();
        let mut retry = tokio::time::interval_at(
            started + DEFAULT_CAPABILITY_REVALIDATION_INTERVAL,
            DEFAULT_CAPABILITY_REVALIDATION_INTERVAL,
        );
        retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let completed_at = started + Duration::from_secs(45);

        schedule_capability_retry(&mut retry, completed_at);

        assert_eq!(
            retry.tick().await,
            completed_at + DEFAULT_CAPABILITY_REVALIDATION_INTERVAL
        );
    }

    #[tokio::test]
    async fn a_readiness_probe_that_does_not_finish_times_out() {
        let result = run_bounded_probe(
            tokio::time::Instant::now() + Duration::from_millis(1),
            std::future::pending::<Result<(), AiCapabilityProbeError>>(),
        )
        .await;

        assert!(matches!(result, Err(AiCapabilityProbeError::Timeout)));
    }

    #[test]
    fn a_standalone_mongodb_topology_blocks_readiness() {
        let error = ensure_transaction_support(false).unwrap_err();

        assert!(matches!(error, CapabilityProbeError::Dependency(_)));
        assert!(ensure_transaction_support(true).is_ok());
    }

    #[test]
    fn capability_probe_errors_distinguish_dependencies_from_capability_failures() {
        assert!(matches!(
            classify_capability_probe_error(AiCapabilityProbeError::Route(
                AiRouteError::Repository(RepositoryError::LockPoisoned)
            )),
            CapabilityProbeError::Dependency(_)
        ));
        assert!(matches!(
            classify_capability_probe_error(AiCapabilityProbeError::Route(
                AiRouteError::Provider(tm_trading_ai_providers::AiProviderError::HttpStatus(401))
            )),
            CapabilityProbeError::Capability(code) if code == "auth_failed"
        ));
        assert!(matches!(
            classify_capability_probe_error(AiCapabilityProbeError::InvalidResponse),
            CapabilityProbeError::Capability(code) if code == "invalid_model_output"
        ));
        assert!(matches!(
            classify_capability_probe_error(AiCapabilityProbeError::Timeout),
            CapabilityProbeError::Dependency(_)
        ));
    }

    #[test]
    fn an_unknown_cycle_does_not_invalidate_capability_readiness() {
        let mut worker = Worker::new("ai-worker-test");
        worker.record_capability(true);

        let WorkerMessage::Cycle {
            skipped,
            error,
            evidence,
            ..
        } = worker.unknown("aijob1_test")
        else {
            panic!("expected a cycle message");
        };

        assert_eq!(skipped.as_deref(), Some("outcome_unknown"));
        assert!(error.is_none());
        assert_eq!(evidence.capability_verified, Some(true));
        assert_eq!(worker.evidence.capability_verified, Some(true));
    }

    #[test]
    fn an_unknown_job_with_an_observed_capability_failure_invalidates_readiness() {
        let mut worker = Worker::new("ai-worker-test");
        worker.record_capability(true);

        worker.record_unknown_outcome(Some(AiJobFailure::InvalidModelOutput));

        assert_eq!(worker.evidence.capability_verified, Some(false));
        assert_eq!(worker.evidence.last_job_validated, Some(false));
    }

    #[test]
    fn an_evidence_job_failure_does_not_invalidate_capability_readiness() {
        let mut worker = Worker::new("ai-worker-test");
        worker.record_capability(true);

        worker.record_job_failure(AiJobFailure::NoUsableEvidence);

        assert_eq!(worker.evidence.capability_verified, Some(true));
        assert_eq!(worker.evidence.dependencies_reachable, Some(true));
        assert_eq!(worker.evidence.last_job_validated, Some(false));
    }

    #[test]
    fn provider_route_and_model_output_job_failures_invalidate_capability_readiness() {
        for failure in [
            AiJobFailure::ProviderFailed,
            AiJobFailure::RouteUnavailable,
            AiJobFailure::InvalidModelOutput,
        ] {
            let mut worker = Worker::new("ai-worker-test");
            worker.record_capability(true);

            worker.record_job_failure(failure);

            assert_eq!(worker.evidence.capability_verified, Some(false));
            assert_eq!(worker.evidence.dependencies_reachable, Some(true));
            assert_eq!(worker.evidence.last_job_validated, Some(false));
        }
    }

    #[test]
    fn final_schema_and_publication_failures_invalidate_readiness() {
        let mut worker = Worker::new("ai-worker-test");
        worker.record_capability(true);

        worker.record_job_failure(AiJobFailure::InvalidBriefing);

        assert_eq!(worker.evidence.capability_verified, Some(false));
        assert_eq!(worker.evidence.dependencies_reachable, Some(true));

        let mut worker = Worker::new("ai-worker-test");
        worker.record_capability(true);

        worker.record_job_failure(AiJobFailure::PublishFailed);

        assert_eq!(worker.evidence.capability_verified, Some(false));
        assert_eq!(worker.evidence.dependencies_reachable, Some(false));
        assert_eq!(worker.evidence.sink_writable, Some(false));
    }

    #[test]
    fn a_dependency_job_failure_marks_the_worker_not_ready() {
        let mut worker = Worker::new("ai-worker-test");
        worker.record_capability(true);

        worker.record_job_failure(AiJobFailure::DependencyUnavailable);

        assert_eq!(worker.evidence.capability_verified, Some(false));
        assert_eq!(worker.evidence.dependencies_reachable, Some(false));
        assert_eq!(worker.evidence.sink_writable, Some(false));
    }

    #[test]
    fn a_dependency_failure_marks_the_worker_not_ready_until_revalidated() {
        let mut worker = Worker::new("ai-worker-test");
        worker.record_capability(true);

        worker.record_dependency_failure();

        assert_eq!(worker.evidence.capability_verified, Some(false));
        assert_eq!(worker.evidence.dependencies_reachable, Some(false));
        assert_eq!(worker.evidence.sink_writable, Some(false));

        worker.record_capability(true);

        assert_eq!(worker.evidence.capability_verified, Some(true));
        assert_eq!(worker.evidence.dependencies_reachable, Some(true));
        assert_eq!(worker.evidence.sink_writable, Some(true));
    }

    #[tokio::test]
    async fn aborting_an_active_job_is_not_reported_as_drained() {
        let mut active = Some(tokio::spawn(std::future::pending::<()>()));

        assert!(!stop_active_job(&mut active).await);
        assert!(active.is_none());
        assert!(stop_active_job(&mut None).await);
    }

    #[tokio::test]
    async fn an_in_flight_probe_is_cancelled_and_reported_as_not_drained() {
        let mut probes = tokio::task::JoinSet::new();
        probes.spawn(std::future::pending::<Result<(), CapabilityProbeError>>());

        let drained = stop_active_work(&mut None, &mut probes).await;
        let WorkerMessage::Stopped {
            drained: reported_drained,
            ..
        } = Worker::new("ai-worker-test").stopped("test", drained)
        else {
            panic!("expected a stopped message");
        };

        assert!(!reported_drained);
        assert!(probes.is_empty());
    }
}
