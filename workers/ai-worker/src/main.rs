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
    AiJobOutcome, AiJobProcessor, AiProviderResolver, AiRouteExecutor, BriefingEvidenceAssembler,
    EnvironmentAiProviderResolver,
};
use tm_trading_domain::AiTaskKind;
use tm_trading_storage::{
    MongoAiJobRepository, MongoAiModelRepository, MongoAiProviderProfileRepository,
    MongoAiTaskRouteRepository, MongoConnection, MongoNewsItemRepository,
    MongoResearchEventRepository, RepositoryError,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const DEFAULT_DATABASE_NAME: &str = "tm-trading-rust";
/// Idle delay between two claim attempts.
const DEFAULT_CLAIM_INTERVAL: Duration = Duration::from_secs(5);
/// Delay between two heartbeats.
const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
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
    #[error("ai worker I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

async fn run() -> Result<(), RunError> {
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

    let connection = MongoConnection::connect(&uri, &database_name)
        .await
        .map_err(classify_repository_error)?;
    let database = connection.database();

    let jobs = Arc::new(MongoAiJobRepository::new(database).await?);
    let research = Arc::new(MongoResearchEventRepository::new(database).await?);
    let news = Arc::new(MongoNewsItemRepository::new(database).await?);
    let routes = Arc::new(MongoAiTaskRouteRepository::new(database).await?);
    let providers = Arc::new(MongoAiProviderProfileRepository::new(database).await?);
    let models = Arc::new(MongoAiModelRepository::new(database).await?);

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
        executor,
        tm_trading_ai_worker::DEFAULT_EVIDENCE_MAX_AGE,
    );
    let (job_result_tx, mut job_result_rx) = mpsc::unbounded_channel();

    let probe = probe_capability(
        &connection,
        routes.as_ref(),
        providers.as_ref(),
        models.as_ref(),
        resolver.as_ref(),
    )
    .await;

    let (command_tx, mut command_rx) = tokio::sync::mpsc::unbounded_channel();
    thread::spawn(move || read_commands(command_tx));

    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut worker = Worker::new(&instance_id);

    send(&mut output, &worker.hello(pid))?;
    match probe {
        Ok(()) => {
            worker.record_capability(true);
            send(&mut output, &worker.ready())?;
            send(&mut output, &worker.heartbeat())?;
        }
        Err(error) => {
            worker.record_probe_failure(&error);
            eprintln!("[ai-worker] readiness probe failed: {error}");
            send(&mut output, &worker.heartbeat())?;
        }
    }

    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + heartbeat_interval,
        heartbeat_interval,
    );
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut claim =
        tokio::time::interval_at(tokio::time::Instant::now() + claim_interval, claim_interval);
    claim.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
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
                    Ok(AiJobOutcome::Failed { job_id, error_code }) => {
                        worker.record_job_failure();
                        send(
                            &mut output,
                            &worker.failed(&job_id.to_string(), error_code),
                        )?;
                    }
                    Ok(AiJobOutcome::OutcomeUnknown { job_id }) => {
                        send(&mut output, &worker.unknown(&job_id.to_string()))?;
                    }
                    Err(error) => {
                        worker.record_dependency_failure();
                        eprintln!("[ai-worker] job outcome is unknown: {error}");
                    }
                }
            }
            command = command_rx.recv() => {
                match command {
                    Some(SupervisorCommand::Shutdown { reason, .. }) => {
                        send(&mut output, &worker.stopping(&reason))?;
                        let drained = stop_active_job(&mut active_job).await;
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
                        stop_active_job(&mut active_job).await;
                        return Ok(());
                    }
                }
            }
            _ = claim.tick() => {
                if active_job.is_some() {
                    continue;
                }
                if !worker.capability_verified() {
                    match probe_capability(
                        &connection,
                        routes.as_ref(),
                        providers.as_ref(),
                        models.as_ref(),
                        resolver.as_ref(),
                    )
                    .await
                    {
                        Ok(()) => {
                            worker.record_capability(true);
                            send(&mut output, &worker.ready())?;
                        }
                        Err(error) => {
                            worker.record_probe_failure(&error);
                            eprintln!("[ai-worker] readiness probe failed: {error}");
                            continue;
                        }
                    }
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

#[derive(Debug, Error)]
enum CapabilityProbeError {
    #[error("dependency probe failed: {0}")]
    Dependency(String),
    #[error("{0}")]
    Capability(String),
}

/// Verifies that the configured route can run before the worker reports readiness.
///
/// The probe resolves the provider and its credential but never calls the provider,
/// so readiness never costs a billable request.
async fn probe_capability(
    connection: &MongoConnection,
    routes: &dyn tm_trading_storage::AiTaskRouteRepository,
    providers: &dyn tm_trading_storage::AiProviderProfileRepository,
    models: &dyn tm_trading_storage::AiModelRepository,
    resolver: &dyn AiProviderResolver,
) -> Result<(), CapabilityProbeError> {
    let transactions_supported = connection
        .transactions_supported()
        .await
        .map_err(|error| CapabilityProbeError::Dependency(error.to_string()))?;
    ensure_transaction_support(transactions_supported)?;

    let route = routes
        .get(AiTaskKind::ResearchExplanation.as_str())
        .await
        .map_err(|error| CapabilityProbeError::Dependency(error.to_string()))?
        .ok_or_else(|| {
            CapabilityProbeError::Capability(
                "no research_explanation route is configured".to_owned(),
            )
        })?;
    if !route.enabled() {
        return Err(CapabilityProbeError::Capability(
            "the research_explanation route is disabled".to_owned(),
        ));
    }
    let reference = tm_trading_domain::AiTaskModelRef::parse(route.primary())
        .map_err(|error| CapabilityProbeError::Capability(error.to_string()))?;
    let profile = providers
        .get(reference.provider_id())
        .await
        .map_err(|error| CapabilityProbeError::Dependency(error.to_string()))?
        .ok_or_else(|| {
            CapabilityProbeError::Capability(format!(
                "provider '{}' is not registered",
                reference.provider_id()
            ))
        })?;
    if !profile.enabled() {
        return Err(CapabilityProbeError::Capability(format!(
            "provider '{}' is disabled",
            profile.id()
        )));
    }
    let model = models
        .get(reference.model_registry_id())
        .await
        .map_err(|error| CapabilityProbeError::Dependency(error.to_string()))?
        .ok_or_else(|| {
            CapabilityProbeError::Capability(format!(
                "model '{}' is not registered",
                reference.model_registry_id()
            ))
        })?;
    if !model.enabled() {
        return Err(CapabilityProbeError::Capability(format!(
            "model '{}' is disabled",
            model.id()
        )));
    }
    if !model.supports_json_mode() {
        return Err(CapabilityProbeError::Capability(format!(
            "model '{}' does not support JSON mode",
            model.id()
        )));
    }
    let timeout = Duration::from_millis(route.timeout_ms().min(profile.timeout_ms()));
    resolver
        .resolve(&profile, &model, timeout)
        .map(|_| ())
        .map_err(|error| CapabilityProbeError::Capability(error.code().to_owned()))
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
            self.evidence.capability_verified = Some(false);
        }
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

    fn record_job_failure(&mut self) {
        self.evidence.last_job_validated = Some(false);
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
    fn a_standalone_mongodb_topology_blocks_readiness() {
        let error = ensure_transaction_support(false).unwrap_err();

        assert!(matches!(error, CapabilityProbeError::Dependency(_)));
        assert!(ensure_transaction_support(true).is_ok());
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
    fn a_job_failure_does_not_invalidate_capability_readiness() {
        let mut worker = Worker::new("ai-worker-test");
        worker.record_capability(true);

        worker.record_job_failure();

        assert_eq!(worker.evidence.capability_verified, Some(true));
        assert_eq!(worker.evidence.dependencies_reachable, Some(true));
        assert_eq!(worker.evidence.last_job_validated, Some(false));
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
}
