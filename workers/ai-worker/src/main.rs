//! AI worker executable: publishes evidence-bound briefings from durable AI jobs.
//!
//! The worker owns no scheduling of its own. It claims one durable job at a time,
//! assembles owner-scoped evidence, runs the configured route, validates the model
//! output, publishes the briefing, and only then completes the job.

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
    MongoAiBriefingRepository, MongoAiJobRepository, MongoAiModelRepository,
    MongoAiProviderProfileRepository, MongoAiTaskRouteRepository, MongoConnection,
    MongoNewsItemRepository, MongoResearchEventRepository, RepositoryError,
};

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
    let briefings = Arc::new(MongoAiBriefingRepository::new(database).await?);
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
        Arc::clone(&briefings) as Arc<dyn tm_trading_storage::AiBriefingRepository>,
        assembler,
        executor,
        tm_trading_ai_worker::DEFAULT_EVIDENCE_MAX_AGE,
    );

    let probe = probe_capability(
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
    match &probe {
        Ok(()) => {
            worker.record_ready();
            send(&mut output, &worker.ready())?;
            send(&mut output, &worker.heartbeat())?;
        }
        Err(error) => {
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

    loop {
        tokio::select! {
            biased;
            command = command_rx.recv() => {
                match command {
                    Some(SupervisorCommand::Shutdown { reason, .. }) => {
                        send(&mut output, &worker.stopping(&reason))?;
                        send(&mut output, &worker.stopped(&reason))?;
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
                        return Ok(());
                    }
                }
            }
            _ = claim.tick() => {
                if probe.is_err() {
                    continue;
                }
                match processor
                    .process_next(&instance_id, Utc::now(), lease)
                    .await
                {
                    Ok(AiJobOutcome::Idle) => {}
                    Ok(AiJobOutcome::Published { job_id, briefing_id }) => {
                        worker.record_success();
                        send(
                            &mut output,
                            &worker.published(&job_id.to_string(), &briefing_id),
                        )?;
                    }
                    Ok(AiJobOutcome::Failed { job_id, error_code }) => {
                        worker.record_failure();
                        send(
                            &mut output,
                            &worker.failed(&job_id.to_string(), error_code),
                        )?;
                    }
                    Ok(AiJobOutcome::OutcomeUnknown { job_id }) => {
                        worker.record_failure();
                        send(
                            &mut output,
                            &worker.failed(&job_id.to_string(), "lease_lapsed"),
                        )?;
                    }
                    Err(error) => {
                        worker.record_failure();
                        eprintln!("[ai-worker] job processing failed: {error}");
                    }
                }
            }
            _ = heartbeat.tick() => {
                send(&mut output, &worker.heartbeat())?;
            }
        }
    }
}

/// Verifies that the configured route can run before the worker reports readiness.
///
/// The probe resolves the provider and its credential but never calls the provider,
/// so readiness never costs a billable request.
async fn probe_capability(
    routes: &dyn tm_trading_storage::AiTaskRouteRepository,
    providers: &dyn tm_trading_storage::AiProviderProfileRepository,
    models: &dyn tm_trading_storage::AiModelRepository,
    resolver: &dyn AiProviderResolver,
) -> Result<(), String> {
    let route = routes
        .get(AiTaskKind::ResearchExplanation.as_str())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "no research_explanation route is configured".to_owned())?;
    if !route.enabled() {
        return Err("the research_explanation route is disabled".to_owned());
    }
    let reference = tm_trading_domain::AiTaskModelRef::parse(route.primary())
        .map_err(|error| error.to_string())?;
    let profile = providers
        .get(reference.provider_id())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("provider '{}' is not registered", reference.provider_id()))?;
    if !profile.enabled() {
        return Err(format!("provider '{}' is disabled", profile.id()));
    }
    let model = models
        .get(reference.model_registry_id())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            format!(
                "model '{}' is not registered",
                reference.model_registry_id()
            )
        })?;
    if !model.enabled() {
        return Err(format!("model '{}' is disabled", model.id()));
    }
    if !model.supports_json_mode() {
        return Err(format!("model '{}' does not support JSON mode", model.id()));
    }
    let timeout = Duration::from_millis(route.timeout_ms().min(profile.timeout_ms()));
    resolver
        .resolve(&profile, &model, timeout)
        .map(|_| ())
        .map_err(|error| error.code().to_owned())
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
    ready: bool,
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
            ready: false,
        }
    }

    fn record_ready(&mut self) {
        self.ready = true;
        self.evidence.dependencies_reachable = Some(true);
        self.evidence.sink_writable = Some(true);
        self.evidence.capability_verified = Some(true);
    }

    fn record_success(&mut self) {
        self.evidence.last_success_at = Some(Utc::now());
        self.evidence.last_output_count = Some(1);
        self.evidence.last_event_at = Some(Utc::now());
    }

    fn record_failure(&mut self) {
        self.evidence.capability_verified = Some(false);
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

    fn stopping(&self, reason: &str) -> WorkerMessage {
        WorkerMessage::Stopping {
            instance_id: self.instance_id.clone(),
            at: Utc::now(),
            reason: reason.to_owned(),
        }
    }

    fn stopped(&self, reason: &str) -> WorkerMessage {
        WorkerMessage::Stopped {
            instance_id: self.instance_id.clone(),
            at: Utc::now(),
            reason: reason.to_owned(),
            drained: true,
        }
    }
}
