mod api;
mod state;

use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use process_wrap::tokio::{ChildWrapper, CommandWrap};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use tracing::{error, info, warn};
use uuid::Uuid;

use tm_trading_agent_protocol::{SupervisorCommand, WORKER_CONFIGURATION_EXIT_CODE, WorkerMessage};
use tm_trading_contracts::{
    AgentCommandDto, DesiredStateDto, ObservedStateDto, OperationActionDto, ReadinessDto,
    ServiceSnapshot,
};
use tm_trading_domain::{DesiredState, ObservedState, Readiness};
use tm_trading_supervisor_core::{
    MARKET_WORKER_ID, NEWS_WORKER_ID, RESEARCH_WORKER_ID, ServiceState, SupervisorError,
    WorkerDefinition, WorkerProcessIdentity, WorkerRegistry,
};

pub use api::ManagementApiClient;

#[cfg(windows)]
use process_wrap::tokio::{CreationFlags, JobObject, KillOnDrop};
#[cfg(unix)]
use process_wrap::tokio::{KillOnDrop, ProcessGroup};
#[cfg(windows)]
use windows::Win32::System::Threading::DETACHED_PROCESS;

const AGENT_LOCK_FILE_NAME: &str = "agent.lock";
const MAX_WORKER_EVENTS_PER_BATCH: usize = 128;

/// Environment handed to the market worker only.
const MARKET_WORKER_ENVIRONMENT: &[&str] = &[
    "TM_DATABASE_URI",
    "TM_DATABASE_NAME",
    "TM_TRADING_MARKET_SYMBOLS",
    "TM_TRADING_MARKET_TIMEFRAME",
    "TM_TRADING_MARKET_BARS",
    "TM_TRADING_MARKET_CYCLE_MS",
    "TM_TRADING_MARKET_CYCLE_TIMEOUT_MS",
    "TM_TRADING_MARKET_HEARTBEAT_MS",
    "TM_TRADING_MARKET_PROVIDER_ID",
    "TM_TRADING_MARKET_SOURCE_OF_TRUTH",
];
/// Environment handed to the research worker only.
const RESEARCH_WORKER_ENVIRONMENT: &[&str] = &[
    "TM_DATABASE_URI",
    "TM_DATABASE_NAME",
    "TM_TRADING_RESEARCH_OWNER_SCOPE",
    "TM_TRADING_RESEARCH_SYMBOLS",
    "TM_TRADING_RESEARCH_TIMEFRAME",
    "TM_TRADING_RESEARCH_BARS",
    "TM_TRADING_RESEARCH_CYCLE_MS",
    "TM_TRADING_RESEARCH_CYCLE_TIMEOUT_MS",
    "TM_TRADING_RESEARCH_HEARTBEAT_MS",
    "TM_TRADING_RESEARCH_SOURCE_OF_TRUTH",
];
/// Environment handed to the news worker only.
const NEWS_WORKER_ENVIRONMENT: &[&str] = &[
    "TM_DATABASE_URI",
    "TM_DATABASE_NAME",
    "TM_TRADING_NEWS_MAX_ITEMS_PER_SOURCE",
    "TM_TRADING_NEWS_MAX_SOURCES_PER_CYCLE",
    "TM_TRADING_NEWS_CYCLE_MS",
    "TM_TRADING_NEWS_CYCLE_TIMEOUT_MS",
    "TM_TRADING_NEWS_HEARTBEAT_MS",
    "TM_TRADING_NEWS_SOURCE_OF_TRUTH",
];
const FORCED_TERMINATION_TIMEOUT: Duration = Duration::from_secs(5);
const FINAL_EVENT_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent ID must contain only letters, digits, '-', '_' or '.'")]
    InvalidAgentId,
    #[error("worker directory does not exist: {0}")]
    WorkerDirectoryNotFound(PathBuf),
    #[error("unknown worker service: {0}")]
    UnknownWorker(String),
    #[error("worker is already running: {0}")]
    AlreadyRunning(String),
    #[error("worker is not running: {0}")]
    NotRunning(String),
    #[error("worker exited before becoming ready with code {0:?}")]
    ExitedBeforeReady(Option<i32>),
    #[error("worker did not become ready before the startup timeout")]
    StartupTimeout,
    #[error("worker control command failed: {0}")]
    Command(#[source] io::Error),
    #[error("worker protocol error: {0}")]
    Protocol(#[from] SupervisorError),
    #[error("invalid worker protocol message: {0}")]
    InvalidProtocol(String),
    #[error("worker protocol stream failed: {0}")]
    ProtocolStream(String),
    #[error("retry block can only be cleared while desired state is RUNNING: {0}")]
    RetryBlockRequiresRunningDesiredState(String),
    #[error("worker identity mismatch: {0}")]
    WorkerIdentityMismatch(String),
    #[error("worker termination failed: {0}")]
    WorkerTermination(String),
    #[error("Agent shutdown was requested")]
    ShutdownRequested,
    #[error("another TM Trading Agent already owns {0}")]
    AgentAlreadyRunning(PathBuf),
    #[error("Agent state is invalid: {0}")]
    InvalidState(String),
    #[error("Agent state I/O failed: {0}")]
    StateIo(#[source] io::Error),
    #[error("management API request failed: {0}")]
    Api(String),
}

impl AgentError {
    fn is_deterministic_worker_rejection(&self) -> bool {
        matches!(
            self,
            Self::Protocol(_) | Self::InvalidProtocol(_) | Self::WorkerIdentityMismatch(_)
        ) || matches!(
            self,
            Self::ExitedBeforeReady(Some(exit_code))
                if *exit_code == WORKER_CONFIGURATION_EXIT_CODE
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopOutcome {
    pub drained: bool,
    pub forced: bool,
    pub exit_code: Option<i32>,
}

#[derive(Debug)]
enum WorkerEvent {
    Message(WorkerMessage),
    Log(String),
    ProtocolError(String),
    ProtocolStreamError(String),
}

#[derive(Debug)]
struct ManagedWorker {
    child: Box<dyn ChildWrapper>,
    stdin: ChildStdin,
    events: mpsc::Receiver<WorkerEvent>,
    definition: WorkerDefinition,
    state: ServiceState,
    drained: Option<bool>,
    expected_pid: u32,
    expected_executable_path: String,
}

#[derive(Debug)]
pub struct Agent {
    agent_id: String,
    worker_dir: PathBuf,
    state_dir: PathBuf,
    state_file: PathBuf,
    registry: WorkerRegistry,
    workers: HashMap<String, ManagedWorker>,
    stopped_states: HashMap<String, ServiceState>,
    crash_after_ms: Option<u64>,
    dirty: bool,
    shutdown_tx: watch::Sender<()>,
    shutdown_rx: watch::Receiver<()>,
    shutdown_listener: Option<tokio::task::JoinHandle<()>>,
    _lock: fs::File,
}

impl Agent {
    pub fn new(
        agent_id: &str,
        worker_dir: impl AsRef<Path>,
        state_dir: impl AsRef<Path>,
        registry: WorkerRegistry,
    ) -> Result<Self, AgentError> {
        if agent_id.is_empty()
            || !agent_id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
        {
            return Err(AgentError::InvalidAgentId);
        }
        let worker_dir = worker_dir.as_ref().to_path_buf();
        if !worker_dir.is_dir() {
            return Err(AgentError::WorkerDirectoryNotFound(worker_dir));
        }
        let state_dir = state_dir.as_ref().to_path_buf();
        fs::create_dir_all(&state_dir).map_err(AgentError::StateIo)?;
        let lock_path = state_dir.join(AGENT_LOCK_FILE_NAME);
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(AgentError::StateIo)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(AgentError::AgentAlreadyRunning(lock_path));
            }
            Err(std::fs::TryLockError::Error(error)) => {
                return Err(AgentError::StateIo(error));
            }
        }

        let state_file = state_dir.join(state::STATE_FILE_NAME);
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let mut agent = Self {
            agent_id: agent_id.to_owned(),
            worker_dir,
            state_dir,
            state_file,
            registry,
            workers: HashMap::new(),
            stopped_states: HashMap::new(),
            crash_after_ms: None,
            dirty: false,
            shutdown_tx,
            shutdown_rx,
            shutdown_listener: None,
            _lock: lock,
        };
        let persisted = state::load(
            &agent.state_file,
            &agent.agent_id,
            &agent.registry,
            Utc::now(),
        )
        .map_err(AgentError::InvalidState)?;
        for service_state in persisted {
            agent
                .stopped_states
                .insert(service_state.service_definition_id.clone(), service_state);
        }
        Ok(agent)
    }

    #[must_use]
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    #[must_use]
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub fn request_shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    pub fn spawn_shutdown_listener(&mut self) {
        if let Some(listener) = self.shutdown_listener.take() {
            listener.abort();
        }
        let shutdown_tx = self.shutdown_tx.clone();
        self.shutdown_listener = Some(tokio::spawn(async move {
            if let Err(error) = shutdown_signal().await {
                error!(%error, "failed to listen for an Agent shutdown signal");
            } else {
                let _ = shutdown_tx.send(());
            }
        }));
    }

    pub async fn shutdown(&mut self, reason: &str) -> Result<(), AgentError> {
        self.request_shutdown();
        self.stop_all(reason).await
    }

    pub fn enable_crash_drill(&mut self, delay_ms: u64) {
        self.crash_after_ms = Some(delay_ms);
    }

    #[must_use]
    pub fn service_state(&self, service_definition_id: &str) -> Option<&ServiceState> {
        self.workers
            .get(service_definition_id)
            .map(|worker| &worker.state)
            .or_else(|| self.stopped_states.get(service_definition_id))
    }

    #[must_use]
    pub fn service_states(&self) -> Vec<ServiceState> {
        let mut states = self
            .workers
            .values()
            .map(|worker| worker.state.clone())
            .chain(self.stopped_states.values().cloned())
            .collect::<Vec<_>>();
        states.sort_by(|left, right| left.service_definition_id.cmp(&right.service_definition_id));
        states
    }

    pub async fn start_service_for_launch(
        &mut self,
        service_definition_id: &str,
    ) -> Result<bool, AgentError> {
        if let Some(state) = self.stopped_states.get(service_definition_id) {
            if state.desired_state == DesiredState::Stopped {
                return Ok(false);
            }
            if state.retry_blocked {
                return Ok(false);
            }
            if state.restart_count
                > self
                    .registry
                    .get(service_definition_id)
                    .map(|definition| definition.max_restart_attempts)
                    .unwrap_or(0)
            {
                return Ok(false);
            }
            if state
                .next_restart_at
                .is_some_and(|restart_at| restart_at > Utc::now())
            {
                return Ok(false);
            }
        }
        self.start_service(service_definition_id).await?;
        Ok(true)
    }

    pub async fn start_service(&mut self, service_definition_id: &str) -> Result<(), AgentError> {
        if self.workers.contains_key(service_definition_id) {
            return Err(AgentError::AlreadyRunning(service_definition_id.to_owned()));
        }
        let definition = self
            .registry
            .get(service_definition_id)
            .ok_or_else(|| AgentError::UnknownWorker(service_definition_id.to_owned()))?
            .clone();
        let instance_id = format!("{}@{}:{}", definition.id, self.agent_id, Uuid::new_v4());
        let mut service_state = self
            .stopped_states
            .remove(service_definition_id)
            .unwrap_or_else(|| ServiceState::new(definition.id, &instance_id));
        service_state.service_instance_id = instance_id.clone();
        service_state.request_start();
        self.stopped_states
            .insert(service_definition_id.to_owned(), service_state.clone());
        self.dirty = true;
        self.persist_state()?;

        let mut worker = match self
            .spawn_worker(&definition, &instance_id, service_state.clone())
            .await
        {
            Ok(worker) => worker,
            Err(error) => {
                service_state.apply_exit(&definition, -1, Utc::now(), jitter_percent());
                self.stopped_states
                    .insert(service_definition_id.to_owned(), service_state);
                self.dirty = true;
                self.persist_state()?;
                return Err(error);
            }
        };
        let mut shutdown = self.shutdown_rx.clone();
        if let Err(error) = worker.wait_until_ready(&definition, &mut shutdown).await {
            let shutdown_requested = matches!(error, AgentError::ShutdownRequested);
            let exit_code = match &error {
                AgentError::ExitedBeforeReady(exit_code) => *exit_code,
                _ => worker.terminate().await.unwrap_or(None),
            };
            if error.is_deterministic_worker_rejection() {
                worker.state.quarantine(exit_code);
            } else if shutdown_requested {
                worker
                    .state
                    .apply_agent_shutdown_exit(exit_code.unwrap_or(-1));
            } else {
                worker.state.apply_exit(
                    &definition,
                    exit_code.unwrap_or(-1),
                    Utc::now(),
                    jitter_percent(),
                );
            }
            self.stopped_states
                .insert(service_definition_id.to_owned(), worker.state);
            self.dirty = true;
            self.persist_state()?;
            return Err(error);
        }
        self.stopped_states.remove(service_definition_id);
        self.workers
            .insert(service_definition_id.to_owned(), worker);
        self.dirty = true;
        self.persist_state()?;
        info!(service_definition_id, instance_id, "worker became ready");
        Ok(())
    }

    pub async fn stop_service(
        &mut self,
        service_definition_id: &str,
        reason: &str,
    ) -> Result<StopOutcome, AgentError> {
        let definition = self
            .registry
            .get(service_definition_id)
            .ok_or_else(|| AgentError::UnknownWorker(service_definition_id.to_owned()))?
            .clone();
        if let Some(mut worker) = self.workers.remove(service_definition_id) {
            worker.state.request_stop();
            let outcome = match worker.stop(&definition, reason).await {
                Ok(outcome) => outcome,
                Err(error) => {
                    self.workers
                        .insert(service_definition_id.to_owned(), worker);
                    self.dirty = true;
                    if let Err(state_error) = self.persist_state() {
                        error!(%state_error, "failed to persist a stopped worker state");
                    }
                    return Err(error);
                }
            };
            worker.state.apply_exit(
                &definition,
                outcome.exit_code.unwrap_or(-1),
                Utc::now(),
                jitter_percent(),
            );
            self.stopped_states
                .insert(service_definition_id.to_owned(), worker.state);
            self.dirty = true;
            self.persist_state()?;
            return Ok(outcome);
        }

        if let Some(mut service_state) = self.stopped_states.remove(service_definition_id) {
            let was_running = service_state.desired_state == DesiredState::Running;
            service_state.request_stop();
            self.stopped_states
                .insert(service_definition_id.to_owned(), service_state);
            self.dirty = true;
            self.persist_state()?;
            if was_running {
                info!(service_definition_id, "automatic restart cancelled");
            }
            return Ok(StopOutcome {
                drained: false,
                forced: false,
                exit_code: None,
            });
        }
        Err(AgentError::NotRunning(service_definition_id.to_owned()))
    }

    pub async fn restart_service(
        &mut self,
        service_definition_id: &str,
        reason: &str,
    ) -> Result<(), AgentError> {
        self.stop_service(service_definition_id, reason).await?;
        self.start_service(service_definition_id).await
    }

    pub fn clear_retry_block(&mut self, service_definition_id: &str) -> Result<(), AgentError> {
        let Some(state) = self.stopped_states.get_mut(service_definition_id) else {
            return Err(AgentError::NotRunning(service_definition_id.to_owned()));
        };
        if state.desired_state != DesiredState::Running {
            return Err(AgentError::RetryBlockRequiresRunningDesiredState(
                service_definition_id.to_owned(),
            ));
        }
        state.retry_blocked = false;
        state.next_restart_at = Some(Utc::now());
        self.dirty = true;
        self.persist_state()
    }

    pub async fn poll_once(&mut self) -> Result<(), AgentError> {
        let service_ids = self.workers.keys().cloned().collect::<Vec<_>>();
        for service_id in service_ids {
            let Some(mut worker) = self.workers.remove(&service_id) else {
                continue;
            };
            let definition = self
                .registry
                .get(&service_id)
                .ok_or_else(|| AgentError::UnknownWorker(service_id.clone()))?
                .clone();
            let previous_state = worker.state.clone();
            let mut event_error = None;
            for _ in 0..MAX_WORKER_EVENTS_PER_BATCH {
                match worker.events.try_recv() {
                    Ok(event) => {
                        if let Err(error) = worker.handle_event(event) {
                            event_error = Some(error);
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            if let Some(error) = event_error {
                self.quarantine_worker(service_id, worker, &definition, &error)
                    .await;
                continue;
            }
            worker.state.refresh_readiness(&definition, Utc::now());
            match worker.child.try_wait() {
                Ok(Some(status)) => {
                    worker.state.apply_exit(
                        &definition,
                        status.code().unwrap_or(-1),
                        Utc::now(),
                        jitter_percent(),
                    );
                    self.stopped_states.insert(service_id.clone(), worker.state);
                    self.dirty = true;
                    warn!(service_id, exit_code = status.code(), "worker exited");
                }
                Ok(None) => {
                    if worker.state != previous_state {
                        self.dirty = true;
                    }
                    self.workers.insert(service_id, worker);
                }
                Err(error) => {
                    let error = AgentError::Command(error);
                    self.quarantine_worker(service_id, worker, &definition, &error)
                        .await;
                }
            }
        }

        self.start_due_restarts().await?;
        if self.dirty {
            self.persist_state()?;
        }
        Ok(())
    }

    async fn quarantine_worker(
        &mut self,
        service_id: String,
        mut worker: ManagedWorker,
        definition: &WorkerDefinition,
        error: &AgentError,
    ) {
        error!(service_id, %error, "worker rejected; isolating the failed service");
        worker
            .state
            .record_log(Utc::now(), format!("worker rejected: {error}"));
        let exit_code = match &error {
            AgentError::ExitedBeforeReady(exit_code) => *exit_code,
            _ => worker.terminate().await.unwrap_or(None),
        };
        if error.is_deterministic_worker_rejection() {
            worker.state.quarantine(exit_code);
        } else {
            worker.state.apply_exit(
                definition,
                exit_code.unwrap_or(-1),
                Utc::now(),
                jitter_percent(),
            );
        }
        self.stopped_states.insert(service_id, worker.state);
        self.dirty = true;
    }

    pub async fn run_until_shutdown(
        &mut self,
        poll_interval: Duration,
        api: Option<&ManagementApiClient>,
    ) -> Result<(), AgentError> {
        let mut shutdown = self.shutdown_rx.clone();
        loop {
            if let Some(api) = api {
                self.serve_management_api(api).await;
            }
            match self.poll_once().await {
                Err(AgentError::ShutdownRequested) => break,
                result => result?,
            }
            if shutdown.has_changed().unwrap_or(true) {
                break;
            }
            tokio::select! {
                biased;
                changed = shutdown.changed() => {
                    let _ = changed;
                    break;
                }
                () = tokio::time::sleep(poll_interval) => {}
            }
        }
        self.stop_all("agent shutdown").await
    }

    /// Reports the current service read model and executes one claimed command, if any.
    async fn serve_management_api(&mut self, api: &ManagementApiClient) {
        let snapshots = self.service_snapshots();
        if let Err(error) = api.heartbeat(snapshots).await {
            warn!(%error, "management API heartbeat failed");
            return;
        }

        let command = match api.claim_command().await {
            Ok(Some(command)) => command,
            Ok(None) => return,
            Err(error) => {
                warn!(%error, "management API command claim failed");
                return;
            }
        };

        if let Err(error) = api.mark_running(&command).await {
            warn!(%error, "management API command progress report failed");
            return;
        }

        let result = self.execute_command(&command).await;
        if let Err(error) = api.complete_command(&command, result).await {
            warn!(%error, "management API command completion report failed");
        }
    }

    /// Read model of every registered service, including services that are not running.
    fn service_snapshots(&self) -> Vec<ServiceSnapshot> {
        self.registry
            .iter()
            .map(|definition| {
                let state = self
                    .workers
                    .get(definition.id)
                    .map(|worker| &worker.state)
                    .or_else(|| self.stopped_states.get(definition.id));
                match state {
                    Some(state) => service_snapshot(self.agent_id(), state),
                    None => service_snapshot(
                        self.agent_id(),
                        &ServiceState::new(
                            definition.id,
                            &planned_instance_id(definition.id, self.agent_id()),
                        ),
                    ),
                }
            })
            .collect()
    }

    async fn execute_command(&mut self, command: &AgentCommandDto) -> Result<String, AgentError> {
        let service_id = command.service_definition_id.as_str();
        match command.action {
            OperationActionDto::Start => {
                self.start_service(service_id).await?;
                Ok(format!("service '{service_id}' started and ready"))
            }
            OperationActionDto::Stop => {
                let outcome = self
                    .stop_service(service_id, "management API command")
                    .await?;
                Ok(format!(
                    "service '{service_id}' stopped (drained={}, forced={})",
                    outcome.drained, outcome.forced
                ))
            }
            OperationActionDto::Restart => {
                self.restart_service(service_id, "management API command")
                    .await?;
                Ok(format!("service '{service_id}' restarted and ready"))
            }
        }
    }

    async fn stop_all(&mut self, reason: &str) -> Result<(), AgentError> {
        let service_ids = self
            .workers
            .keys()
            .chain(
                self.stopped_states
                    .iter()
                    .filter(|(_, state)| state.desired_state == DesiredState::Running)
                    .map(|(service_id, _)| service_id),
            )
            .cloned()
            .collect::<HashSet<_>>();
        let mut first_error = None;
        for service_id in service_ids {
            let definition = self
                .registry
                .get(&service_id)
                .cloned()
                .ok_or_else(|| AgentError::UnknownWorker(service_id.clone()))?;
            if let Some(mut worker) = self.workers.remove(&service_id) {
                worker.state.request_process_stop();
                match worker.stop(&definition, reason).await {
                    Ok(outcome) => {
                        worker
                            .state
                            .apply_agent_shutdown_exit(outcome.exit_code.unwrap_or(-1));
                        self.stopped_states.insert(service_id, worker.state);
                        self.dirty = true;
                    }
                    Err(error) => {
                        error!(service_id, %error, "failed to stop worker cleanly");
                        self.workers.insert(service_id, worker);
                        first_error.get_or_insert(error);
                    }
                }
            } else if let Some(mut state) = self.stopped_states.remove(&service_id) {
                state.request_process_stop();
                self.stopped_states.insert(service_id, state);
                self.dirty = true;
            }
        }
        if self.dirty {
            self.persist_state()?;
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn start_due_restarts(&mut self) -> Result<(), AgentError> {
        let now = Utc::now();
        let restart_ids = self
            .stopped_states
            .iter()
            .filter(|(_, state)| {
                state.desired_state == DesiredState::Running && state.restart_due(now)
            })
            .map(|(service_id, _)| service_id.clone())
            .collect::<Vec<_>>();
        for service_id in restart_ids {
            if self.shutdown_rx.has_changed().unwrap_or(true) {
                return Err(AgentError::ShutdownRequested);
            }
            let Some(definition) = self.registry.get(&service_id).cloned() else {
                continue;
            };
            let Some(mut service_state) = self.stopped_states.remove(&service_id) else {
                continue;
            };
            let instance_id = format!("{}@{}:{}", definition.id, self.agent_id, Uuid::new_v4());
            service_state.prepare_restart(&instance_id);
            let fallback_state = service_state.clone();
            let mut shutdown = self.shutdown_rx.clone();
            match self
                .spawn_worker(&definition, &instance_id, service_state)
                .await
            {
                Ok(mut worker) => match worker.wait_until_ready(&definition, &mut shutdown).await {
                    Ok(()) => {
                        self.workers.insert(service_id.clone(), worker);
                        self.dirty = true;
                        warn!(service_id, instance_id, "worker restart became ready");
                    }
                    Err(AgentError::ShutdownRequested) => {
                        let exit_code = worker.terminate().await.unwrap_or(None);
                        worker
                            .state
                            .apply_agent_shutdown_exit(exit_code.unwrap_or(-1));
                        self.stopped_states.insert(service_id.clone(), worker.state);
                        self.dirty = true;
                        self.persist_state()?;
                        return Err(AgentError::ShutdownRequested);
                    }
                    Err(error) => {
                        let exit_code = match &error {
                            AgentError::ExitedBeforeReady(exit_code) => *exit_code,
                            _ => worker.terminate().await.unwrap_or(None),
                        };
                        if error.is_deterministic_worker_rejection() {
                            worker.state.quarantine(exit_code);
                        } else {
                            worker.state.apply_exit(
                                &definition,
                                exit_code.unwrap_or(-1),
                                Utc::now(),
                                jitter_percent(),
                            );
                        }
                        self.stopped_states.insert(service_id.clone(), worker.state);
                        self.dirty = true;
                        error!(service_id, %error, "worker restart failed before readiness");
                    }
                },
                Err(error) => {
                    error!(service_id, %error, "worker restart failed to spawn");
                    let mut service_state = fallback_state;
                    service_state.apply_exit(&definition, -1, Utc::now(), jitter_percent());
                    self.stopped_states.insert(service_id, service_state);
                    self.dirty = true;
                }
            }
        }
        Ok(())
    }

    async fn spawn_worker(
        &mut self,
        definition: &WorkerDefinition,
        instance_id: &str,
        mut state: ServiceState,
    ) -> Result<ManagedWorker, AgentError> {
        let executable = self.worker_dir.join(definition.executable_name);
        let executable = fs::canonicalize(&executable).map_err(AgentError::StateIo)?;
        let expected_executable_path = executable_identity(&executable);
        let mut command = CommandWrap::with_new(&executable, |command| {
            command
                .env_clear()
                .env("TM_SERVICE_ID", definition.id)
                .env("TM_SERVICE_INSTANCE_ID", instance_id)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
        });
        allowlisted_environment(command.command_mut());
        if definition.id == MARKET_WORKER_ID {
            worker_specific_environment(command.command_mut(), MARKET_WORKER_ENVIRONMENT);
        }
        if definition.id == RESEARCH_WORKER_ID {
            worker_specific_environment(command.command_mut(), RESEARCH_WORKER_ENVIRONMENT);
        }
        if definition.id == NEWS_WORKER_ID {
            worker_specific_environment(command.command_mut(), NEWS_WORKER_ENVIRONMENT);
        }
        if let Some(crash_after_ms) = self.crash_after_ms {
            command.command_mut().env(
                "TM_SAMPLE_WORKER_CRASH_AFTER_MS",
                crash_after_ms.to_string(),
            );
        }
        apply_platform_containment(&mut command);

        let mut child = command.spawn().map_err(AgentError::Command)?;
        self.crash_after_ms = None;
        let expected_pid = child
            .id()
            .ok_or_else(|| AgentError::Command(io::Error::other("worker PID is unavailable")))?;
        state.process_identity = Some(WorkerProcessIdentity {
            pid: expected_pid,
            executable_path: expected_executable_path.clone(),
            executable_version: definition.executable_version.to_owned(),
        });
        let stdin = child
            .stdin()
            .take()
            .ok_or_else(|| AgentError::Command(io::Error::other("worker stdin is unavailable")))?;
        let stdout = child
            .stdout()
            .take()
            .ok_or_else(|| AgentError::Command(io::Error::other("worker stdout is unavailable")))?;
        let stderr = child
            .stderr()
            .take()
            .ok_or_else(|| AgentError::Command(io::Error::other("worker stderr is unavailable")))?;
        let (event_tx, event_rx) = mpsc::channel(MAX_WORKER_EVENTS_PER_BATCH);
        let stdout_tx = event_tx.clone();
        let _stdout_task = tokio::spawn(async move {
            read_worker_messages(stdout, stdout_tx).await;
        });
        let _stderr_task = tokio::spawn(async move {
            read_worker_logs(stderr, event_tx).await;
        });

        Ok(ManagedWorker {
            child,
            stdin,
            events: event_rx,
            definition: definition.clone(),
            state,
            drained: None,
            expected_pid,
            expected_executable_path,
        })
    }

    fn persist_state(&mut self) -> Result<(), AgentError> {
        let states = self.service_states();
        state::save(&self.state_file, &self.agent_id, &states).map_err(AgentError::StateIo)?;
        self.dirty = false;
        Ok(())
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        if let Some(listener) = self.shutdown_listener.take() {
            listener.abort();
        }
    }
}

impl ManagedWorker {
    async fn wait_until_ready(
        &mut self,
        definition: &WorkerDefinition,
        shutdown: &mut watch::Receiver<()>,
    ) -> Result<(), AgentError> {
        let deadline = Instant::now() + definition.startup_timeout;
        loop {
            if shutdown.has_changed().unwrap_or(true) {
                return Err(AgentError::ShutdownRequested);
            }
            if self.state.readiness == tm_trading_domain::Readiness::Ready
                && self.state.observed_state == tm_trading_domain::ObservedState::Running
            {
                return Ok(());
            }
            if let Some(status) = self.child.try_wait().map_err(AgentError::Command)? {
                return Err(AgentError::ExitedBeforeReady(status.code()));
            }
            if Instant::now() >= deadline {
                return Err(AgentError::StartupTimeout);
            }
            tokio::select! {
                result = tokio::time::timeout(Duration::from_millis(25), self.events.recv()) => {
                    match result {
                        Ok(Some(event)) => self.handle_event(event)?,
                        Ok(None) => return Err(AgentError::ExitedBeforeReady(None)),
                        Err(_) => {}
                    }
                }
                changed = shutdown.changed() => {
                    let _ = changed;
                    return Err(AgentError::ShutdownRequested);
                }
            }
        }
    }

    async fn stop(
        &mut self,
        definition: &WorkerDefinition,
        reason: &str,
    ) -> Result<StopOutcome, AgentError> {
        let grace_ms = u64::try_from(definition.stop_grace.as_millis()).unwrap_or(u64::MAX);
        let deadline = Instant::now() + definition.stop_grace;
        let command = SupervisorCommand::Shutdown {
            reason: reason.to_owned(),
            grace_ms,
        };
        let command_result = tokio::time::timeout_at(deadline, self.send_command(&command)).await;
        if !matches!(command_result, Ok(Ok(()))) {
            let exit_code = self.force_terminate().await?;
            return Ok(StopOutcome {
                drained: false,
                forced: true,
                exit_code,
            });
        }

        loop {
            for _ in 0..MAX_WORKER_EVENTS_PER_BATCH {
                match self.events.try_recv() {
                    Ok(event) => {
                        if let Err(error) = self.handle_event(event) {
                            warn!(%error, "worker reported an error while stopping");
                        }
                    }
                    Err(_) => break,
                }
            }
            if let Some(status) = self.child.try_wait().map_err(AgentError::Command)? {
                let exit_code = status.code();
                self.drain_final_events(FINAL_EVENT_DRAIN_TIMEOUT).await;
                return Ok(StopOutcome {
                    drained: self.drained.unwrap_or(false),
                    forced: false,
                    exit_code,
                });
            }
            if Instant::now() >= deadline {
                let exit_code = self.force_terminate().await?;
                return Ok(StopOutcome {
                    drained: false,
                    forced: true,
                    exit_code,
                });
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    async fn drain_final_events(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let received = tokio::time::timeout(remaining, self.events.recv()).await;
            match received {
                Ok(Some(event)) => {
                    if let Err(error) = self.handle_event(event) {
                        warn!(%error, "worker reported an error while stopping");
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    }

    async fn force_terminate(&mut self) -> Result<Option<i32>, AgentError> {
        self.child
            .start_kill()
            .map_err(|error| AgentError::WorkerTermination(error.to_string()))?;
        match tokio::time::timeout(FORCED_TERMINATION_TIMEOUT, self.child.wait()).await {
            Ok(Ok(status)) => Ok(status.code()),
            Ok(Err(error)) => Err(AgentError::WorkerTermination(error.to_string())),
            Err(_) => Err(AgentError::WorkerTermination(format!(
                "worker did not exit within {} seconds",
                FORCED_TERMINATION_TIMEOUT.as_secs()
            ))),
        }
    }

    async fn terminate(&mut self) -> Result<Option<i32>, AgentError> {
        self.force_terminate().await
    }

    async fn send_command(&mut self, command: &SupervisorCommand) -> Result<(), AgentError> {
        let line = serde_json::to_string(command)
            .map_err(|error| AgentError::Command(io::Error::other(error)))?;
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(AgentError::Command)?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(AgentError::Command)?;
        self.stdin.flush().await.map_err(AgentError::Command)
    }

    fn handle_event(&mut self, event: WorkerEvent) -> Result<(), AgentError> {
        match event {
            WorkerEvent::Message(message) => {
                if let WorkerMessage::Hello {
                    pid,
                    executable_path,
                    executable_version,
                    ..
                } = &message
                {
                    if *pid != self.expected_pid {
                        return self.reject_identity(format!(
                            "PID {pid} does not match spawned PID {}",
                            self.expected_pid
                        ));
                    }
                    if executable_identity(Path::new(executable_path))
                        != self.expected_executable_path
                    {
                        return self.reject_identity(format!(
                            "executable path '{executable_path}' does not match '{}'",
                            self.expected_executable_path
                        ));
                    }
                    if executable_version != self.definition.executable_version {
                        return self.reject_identity(format!(
                            "executable version '{executable_version}' does not match '{}'",
                            self.definition.executable_version
                        ));
                    }
                }
                let drained = match &message {
                    WorkerMessage::Stopped { drained, .. } => Some(*drained),
                    _ => None,
                };
                if let Err(error) =
                    self.state
                        .apply_worker_message(&self.definition, &message, Utc::now())
                {
                    self.state
                        .record_log(Utc::now(), format!("protocol error: {error}"));
                    return Err(AgentError::Protocol(error));
                }
                if let Some(drained) = drained {
                    self.drained = Some(drained);
                }
            }
            WorkerEvent::Log(line) => self.state.record_log(Utc::now(), line),
            WorkerEvent::ProtocolError(error) => {
                self.state
                    .record_log(Utc::now(), format!("protocol error: {error}"));
                return Err(AgentError::InvalidProtocol(error));
            }
            WorkerEvent::ProtocolStreamError(error) => {
                self.state
                    .record_log(Utc::now(), format!("protocol stream error: {error}"));
                return Err(AgentError::ProtocolStream(error));
            }
        }
        Ok(())
    }

    fn reject_identity(&mut self, message: String) -> Result<(), AgentError> {
        self.state.record_log(Utc::now(), message.clone());
        Err(AgentError::WorkerIdentityMismatch(message))
    }
}

async fn read_worker_messages(stdout: ChildStdout, event_tx: mpsc::Sender<WorkerEvent>) {
    let mut lines = BufReader::new(stdout).lines();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(error) => {
                let _ = event_tx
                    .send(WorkerEvent::ProtocolStreamError(format!(
                        "failed to read worker protocol output: {error}"
                    )))
                    .await;
                break;
            }
        };
        match serde_json::from_str::<WorkerMessage>(&line) {
            Ok(message) => {
                if event_tx.send(WorkerEvent::Message(message)).await.is_err() {
                    break;
                }
            }
            Err(error) => {
                if event_tx
                    .send(WorkerEvent::ProtocolError(format!(
                        "invalid worker protocol message: {error}"
                    )))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    }
}

async fn read_worker_logs(stderr: ChildStderr, event_tx: mpsc::Sender<WorkerEvent>) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if event_tx.send(WorkerEvent::Log(line)).await.is_err() {
            break;
        }
    }
}

fn allowlisted_environment(command: &mut Command) {
    const ALLOWED: &[&str] = &[
        "PATH",
        "SystemRoot",
        "SYSTEMROOT",
        "WINDIR",
        "TEMP",
        "TMP",
        "TMPDIR",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "TM_LOG_FORMAT",
        "TM_SAMPLE_WORKER_HEARTBEAT_MS",
        "TM_SAMPLE_WORKER_CYCLE_MS",
    ];
    for key in ALLOWED {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
        }
    }
}

/// Environment handed to one specific worker.
///
/// The database credential reaches only the market worker, which is the worker that writes
/// market observations. No other worker receives it.
fn worker_specific_environment(command: &mut Command, keys: &[&str]) {
    for key in keys {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
        }
    }
}

fn executable_identity(path: &Path) -> String {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let value = path.to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        value.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        value
    }
}

/// Instance identifier used before a service has ever been started.
fn planned_instance_id(service_definition_id: &str, agent_id: &str) -> String {
    format!("{service_definition_id}@{agent_id}")
}

/// Maps the supervisor state model onto the shared API read model.
fn service_snapshot(agent_id: &str, state: &ServiceState) -> ServiceSnapshot {
    ServiceSnapshot {
        service_instance_id: state.service_instance_id.clone(),
        service_definition_id: state.service_definition_id.clone(),
        agent_id: agent_id.to_owned(),
        desired_state: match state.desired_state {
            DesiredState::Running => DesiredStateDto::Running,
            DesiredState::Stopped => DesiredStateDto::Stopped,
        },
        observed_state: match state.observed_state {
            ObservedState::Starting => ObservedStateDto::Starting,
            ObservedState::Running => ObservedStateDto::Running,
            ObservedState::Stopping => ObservedStateDto::Stopping,
            ObservedState::Stopped => ObservedStateDto::Stopped,
            ObservedState::Failed => ObservedStateDto::Failed,
            ObservedState::Unknown => ObservedStateDto::Unknown,
        },
        readiness: match state.readiness {
            Readiness::Ready => ReadinessDto::Ready,
            Readiness::Degraded => ReadinessDto::Degraded,
            Readiness::NotReady => ReadinessDto::NotReady,
            Readiness::Unknown => ReadinessDto::Unknown,
        },
        process_alive: state.process_alive,
        restart_count: state.restart_count,
        last_exit_code: state.last_exit_code,
        retry_blocked: state.retry_blocked,
        last_heartbeat_at: state.last_heartbeat_at,
        updated_at: Utc::now(),
    }
}

#[cfg(unix)]
fn apply_platform_containment(command: &mut CommandWrap) {
    command.wrap(ProcessGroup::leader()).wrap(KillOnDrop);
}
#[cfg(windows)]
fn apply_platform_containment(command: &mut CommandWrap) {
    // Detach the worker from the Agent console so console control events reach only the Agent,
    // which drains the worker over the explicit control channel. Without this, a console close
    // kills the worker with 0xC000013A before the drain can complete.
    command
        .wrap(CreationFlags(DETACHED_PROCESS))
        .wrap(JobObject)
        .wrap(KillOnDrop);
}

async fn shutdown_signal() -> io::Result<()> {
    #[cfg(windows)]
    {
        let mut ctrl_close = tokio::signal::windows::ctrl_close()?;
        let mut ctrl_logoff = tokio::signal::windows::ctrl_logoff()?;
        let mut ctrl_shutdown = tokio::signal::windows::ctrl_shutdown()?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = ctrl_close.recv() => Ok(()),
            _ = ctrl_logoff.recv() => Ok(()),
            _ = ctrl_shutdown.recv() => Ok(()),
        }
    }
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
}

fn jitter_percent() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() % 101)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::process::Stdio;

    use super::*;
    use tm_trading_supervisor_core::sample_worker_definition;

    fn unique_state_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp/agent-tests")
            .join(format!("tm-trading-agent-{}", Uuid::new_v4()))
    }

    #[cfg(windows)]
    #[test]
    fn worker_is_detached_from_the_agent_console() {
        let mut command = CommandWrap::with_new("worker.exe", |_| {});
        apply_platform_containment(&mut command);

        let flags = command
            .get_wrap::<CreationFlags>()
            .expect("windows worker command must set creation flags");
        assert_eq!(flags.0, DETACHED_PROCESS);
        assert!(command.get_wrap::<JobObject>().is_some());
        assert!(command.get_wrap::<KillOnDrop>().is_some());
    }

    #[test]
    fn invalid_agent_id_is_rejected() {
        let state_dir = unique_state_dir();
        let error = match Agent::new(
            "bad agent",
            std::env::temp_dir(),
            &state_dir,
            WorkerRegistry::sample(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("expected invalid agent ID error"),
        };

        assert!(matches!(error, AgentError::InvalidAgentId));
    }

    #[test]
    fn a_configuration_exit_is_not_retried() {
        let error = AgentError::ExitedBeforeReady(Some(WORKER_CONFIGURATION_EXIT_CODE));

        assert!(error.is_deterministic_worker_rejection());
        assert!(!AgentError::ExitedBeforeReady(Some(2)).is_deterministic_worker_rejection());
    }

    #[tokio::test]
    async fn unknown_worker_is_rejected_without_spawning_a_process() {
        let state_dir = unique_state_dir();
        let mut agent = Agent::new(
            "local-agent",
            std::env::temp_dir(),
            &state_dir,
            WorkerRegistry::sample(),
        )
        .unwrap();

        let result = agent.start_service("not-registered").await;

        assert!(matches!(result, Err(AgentError::UnknownWorker(_))));
    }

    #[test]
    fn a_second_agent_cannot_own_the_same_state_directory() {
        let state_dir = unique_state_dir();
        let _first = Agent::new(
            "local-agent",
            std::env::temp_dir(),
            &state_dir,
            WorkerRegistry::sample(),
        )
        .unwrap();

        let error = Agent::new(
            "local-agent",
            std::env::temp_dir(),
            &state_dir,
            WorkerRegistry::sample(),
        )
        .unwrap_err();

        assert!(matches!(error, AgentError::AgentAlreadyRunning(_)));
    }

    #[tokio::test]
    async fn an_early_worker_exit_consumes_only_one_restart_attempt() {
        let mut definition = sample_worker_definition();
        #[cfg(windows)]
        let executable =
            PathBuf::from(std::env::var("SystemRoot").unwrap()).join("System32/find.exe");
        #[cfg(unix)]
        let executable = PathBuf::from("/usr/bin/false");
        definition.executable_name = if cfg!(windows) { "find.exe" } else { "false" };
        definition.startup_timeout = Duration::from_secs(2);
        let worker_dir = executable.parent().unwrap().to_path_buf();
        let state_dir = unique_state_dir();
        let mut agent = Agent::new(
            "local-agent",
            worker_dir,
            &state_dir,
            WorkerRegistry::new(vec![definition.clone()]),
        )
        .unwrap();

        let error = agent.start_service(definition.id).await.unwrap_err();

        assert!(matches!(error, AgentError::ExitedBeforeReady(_)));
        let state = agent.service_state(definition.id).unwrap();
        assert_eq!(state.restart_count, 1);
        assert!(state.next_restart_at.is_some());
    }

    #[tokio::test]
    async fn a_stopped_service_cannot_be_resurrected_by_clearing_its_retry_block() {
        let state_dir = unique_state_dir();
        let mut agent = Agent::new(
            "local-agent",
            std::env::temp_dir(),
            &state_dir,
            WorkerRegistry::sample(),
        )
        .unwrap();
        let definition = sample_worker_definition();
        let mut state = ServiceState::new(definition.id, "sample-worker@local-agent:1");
        state.request_stop();
        state.retry_blocked = true;
        state.next_restart_at = Some(Utc::now());
        agent.stopped_states.insert(definition.id.to_owned(), state);

        let result = agent.clear_retry_block(definition.id);

        assert!(result.is_err());
        agent.start_due_restarts().await.unwrap();
        assert!(!agent.workers.contains_key(definition.id));
        let state = agent.service_state(definition.id).unwrap();
        assert_eq!(state.desired_state, DesiredState::Stopped);
    }

    #[tokio::test]
    async fn explicit_stop_cancels_a_pending_automatic_restart() {
        let state_dir = unique_state_dir();
        let mut agent = Agent::new(
            "local-agent",
            std::env::temp_dir(),
            &state_dir,
            WorkerRegistry::sample(),
        )
        .unwrap();
        let definition = sample_worker_definition();
        let mut state = ServiceState::new(definition.id, "sample-worker@local-agent:1");
        state.request_start();
        state.process_alive = Some(false);
        state.apply_exit(&definition, 9, Utc::now(), 0);
        agent.stopped_states.insert(definition.id.to_owned(), state);

        let outcome = agent
            .stop_service(definition.id, "test stop")
            .await
            .unwrap();

        assert!(!outcome.forced);
        let state = agent.service_state(definition.id).unwrap();
        assert_eq!(state.desired_state, DesiredState::Stopped);
        assert!(state.next_restart_at.is_none());
    }

    #[tokio::test]
    async fn explicit_stop_cancels_running_intent_without_a_pending_restart() {
        let state_dir = unique_state_dir();
        let mut agent = Agent::new(
            "local-agent",
            std::env::temp_dir(),
            &state_dir,
            WorkerRegistry::sample(),
        )
        .unwrap();
        let definition = sample_worker_definition();
        let mut state = ServiceState::new(definition.id, "sample-worker@local-agent:1");
        state.request_start();
        state.process_alive = Some(false);
        state.next_restart_at = None;
        agent.stopped_states.insert(definition.id.to_owned(), state);

        let outcome = agent
            .stop_service(definition.id, "test stop")
            .await
            .unwrap();

        assert!(!outcome.forced);
        let state = agent.service_state(definition.id).unwrap();
        assert_eq!(state.desired_state, DesiredState::Stopped);
        assert_eq!(
            state.observed_state,
            tm_trading_domain::ObservedState::Stopped
        );
        assert!(state.next_restart_at.is_none());
    }

    #[tokio::test]
    async fn a_late_final_worker_event_is_collected_before_the_stop_outcome_is_reported() {
        let definition = sample_worker_definition();
        let instance_id = "sample-worker@local-agent:1";
        let (mut worker, event_tx) = test_managed_worker(&definition, instance_id).await;
        event_tx
            .send(WorkerEvent::Message(WorkerMessage::Hello {
                instance_id: instance_id.to_owned(),
                at: Utc::now(),
                pid: worker.expected_pid,
                executable_path: worker.expected_executable_path.clone(),
                executable_version: definition.executable_version.to_owned(),
                schema_version: tm_trading_agent_protocol::WORKER_SCHEMA_VERSION.to_owned(),
            }))
            .await
            .unwrap();
        let hello = worker.events.recv().await.unwrap();
        worker.handle_event(hello).unwrap();
        worker.child.start_kill().unwrap();
        worker.child.wait().await.unwrap();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = event_tx
                .send(WorkerEvent::Message(WorkerMessage::Stopped {
                    instance_id: instance_id.to_owned(),
                    at: Utc::now(),
                    reason: "test stop".to_owned(),
                    drained: true,
                }))
                .await;
        });

        worker.drain_final_events(Duration::from_secs(1)).await;

        assert_eq!(worker.drained, Some(true));
    }

    #[tokio::test]
    async fn launch_honors_a_persisted_stopped_desired_state() {
        let state_dir = unique_state_dir();
        let definition = sample_worker_definition();
        {
            let mut agent = Agent::new(
                "local-agent",
                std::env::temp_dir(),
                &state_dir,
                WorkerRegistry::sample(),
            )
            .unwrap();
            let mut state = ServiceState::new(definition.id, "sample-worker@local-agent:1");
            state.request_stop();
            agent.stopped_states.insert(definition.id.to_owned(), state);
            agent.persist_state().unwrap();
        }

        let mut agent = Agent::new(
            "local-agent",
            std::env::temp_dir(),
            &state_dir,
            WorkerRegistry::sample(),
        )
        .unwrap();

        let started = agent.start_service_for_launch(definition.id).await.unwrap();

        assert!(!started);
        assert!(!agent.workers.contains_key(definition.id));
        assert_eq!(
            agent.service_state(definition.id).unwrap().desired_state,
            DesiredState::Stopped
        );
    }

    #[tokio::test]
    async fn launch_honors_a_persisted_running_desired_state_and_backoff() {
        let state_dir = unique_state_dir();
        let restart_at = Utc::now() + chrono::Duration::seconds(30);
        let definition = sample_worker_definition();
        {
            let mut agent = Agent::new(
                "local-agent",
                std::env::temp_dir(),
                &state_dir,
                WorkerRegistry::sample(),
            )
            .unwrap();
            let mut state = ServiceState::new(definition.id, "sample-worker@local-agent:1");
            state.request_start();
            state.process_alive = Some(false);
            state.next_restart_at = Some(restart_at);
            agent.stopped_states.insert(definition.id.to_owned(), state);
            agent.persist_state().unwrap();
        }

        let mut agent = Agent::new(
            "local-agent",
            std::env::temp_dir(),
            &state_dir,
            WorkerRegistry::sample(),
        )
        .unwrap();

        let started = agent.start_service_for_launch(definition.id).await.unwrap();

        assert!(!started);
        assert!(!agent.workers.contains_key(definition.id));
        assert_eq!(
            agent.service_state(definition.id).unwrap().desired_state,
            DesiredState::Running
        );
        assert_eq!(
            agent.service_state(definition.id).unwrap().next_restart_at,
            Some(restart_at)
        );
    }

    #[tokio::test]
    async fn agent_shutdown_preserves_a_pending_restart_backoff() {
        let state_dir = unique_state_dir();
        let mut agent = Agent::new(
            "local-agent",
            std::env::temp_dir(),
            &state_dir,
            WorkerRegistry::sample(),
        )
        .unwrap();
        let definition = sample_worker_definition();
        let restart_at = Utc::now() + chrono::Duration::seconds(30);
        let mut state = ServiceState::new(definition.id, "sample-worker@local-agent:1");
        state.request_start();
        state.process_alive = Some(false);
        state.next_restart_at = Some(restart_at);
        agent.stopped_states.insert(definition.id.to_owned(), state);

        agent.shutdown("test shutdown").await.unwrap();

        let state = agent.service_state(definition.id).unwrap();
        assert_eq!(state.desired_state, DesiredState::Running);
        assert_eq!(state.next_restart_at, Some(restart_at));
        assert_eq!(state.restart_count, 0);
    }

    fn test_waiting_executable() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(std::env::var("SystemRoot").unwrap()).join("System32/sort.exe")
        } else {
            PathBuf::from("/bin/cat")
        }
    }

    async fn test_managed_worker(
        definition: &WorkerDefinition,
        instance_id: &str,
    ) -> (ManagedWorker, mpsc::Sender<WorkerEvent>) {
        let executable = test_waiting_executable();
        let mut command = Command::new(&executable);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let _stdout = child.stdout.take();
        let _stderr = child.stderr.take();
        let expected_pid = child.id().unwrap();
        let expected_executable_path = executable_identity(&executable);
        let (event_tx, event_rx) = mpsc::channel(MAX_WORKER_EVENTS_PER_BATCH + 1);
        let mut state = ServiceState::new(definition.id, instance_id);
        state.request_start();
        let worker = ManagedWorker {
            child: Box::new(child),
            stdin,
            events: event_rx,
            definition: definition.clone(),
            state,
            drained: None,
            expected_pid,
            expected_executable_path,
        };
        (worker, event_tx)
    }

    #[tokio::test]
    async fn worker_version_mismatch_is_rejected_and_logged() {
        let mut definition = sample_worker_definition();
        definition.executable_version = "expected-version";
        let instance_id = "sample-worker@local-agent:1";
        let (mut worker, event_tx) = test_managed_worker(&definition, instance_id).await;
        event_tx
            .send(WorkerEvent::Message(WorkerMessage::Hello {
                instance_id: instance_id.to_owned(),
                at: Utc::now(),
                pid: worker.expected_pid,
                executable_path: worker.expected_executable_path.clone(),
                executable_version: "wrong-version".to_owned(),
                schema_version: tm_trading_agent_protocol::WORKER_SCHEMA_VERSION.to_owned(),
            }))
            .await
            .unwrap();

        let event = worker
            .events
            .recv()
            .await
            .expect("identity event should be queued");
        let error = worker.handle_event(event).unwrap_err();

        assert!(matches!(error, AgentError::WorkerIdentityMismatch(_)));
        assert!(
            worker
                .state
                .logs
                .iter()
                .any(|entry| entry.line.contains("wrong-version"))
        );
        let _ = worker.child.start_kill();
        let _ = worker.child.wait().await;
    }

    #[tokio::test]
    async fn malformed_worker_protocol_is_rejected_and_logged() {
        let definition = sample_worker_definition();
        let (mut worker, event_tx) =
            test_managed_worker(&definition, "sample-worker@local-agent:1").await;
        event_tx
            .send(WorkerEvent::ProtocolError(
                "invalid worker protocol message: expected JSON".to_owned(),
            ))
            .await
            .unwrap();
        let event = worker.events.recv().await.unwrap();

        let error = worker.handle_event(event).unwrap_err();

        assert!(matches!(error, AgentError::InvalidProtocol(_)));
        assert!(
            worker
                .state
                .logs
                .iter()
                .any(|entry| entry.line.contains("expected JSON"))
        );
        let _ = worker.child.start_kill();
        let _ = worker.child.wait().await;
    }

    #[tokio::test]
    async fn one_invalid_worker_does_not_stop_other_workers() {
        let mut invalid_definition = sample_worker_definition();
        invalid_definition.id = "invalid-worker";
        invalid_definition.stop_grace = Duration::from_millis(100);
        let mut valid_definition = sample_worker_definition();
        valid_definition.id = "valid-worker";
        valid_definition.stop_grace = Duration::from_millis(100);
        let state_dir = unique_state_dir();
        let mut agent = Agent::new(
            "local-agent",
            std::env::temp_dir(),
            &state_dir,
            WorkerRegistry::new(vec![invalid_definition.clone(), valid_definition.clone()]),
        )
        .unwrap();
        let (invalid_worker, event_tx) =
            test_managed_worker(&invalid_definition, "invalid-worker@local-agent:1").await;
        let (valid_worker, _) =
            test_managed_worker(&valid_definition, "valid-worker@local-agent:1").await;
        let invalid_pid = invalid_worker.expected_pid;
        event_tx
            .send(WorkerEvent::Message(WorkerMessage::Hello {
                instance_id: "invalid-worker@local-agent:1".to_owned(),
                at: Utc::now(),
                pid: invalid_pid.wrapping_add(1),
                executable_path: invalid_worker.expected_executable_path.clone(),
                executable_version: invalid_definition.executable_version.to_owned(),
                schema_version: tm_trading_agent_protocol::WORKER_SCHEMA_VERSION.to_owned(),
            }))
            .await
            .unwrap();
        agent
            .workers
            .insert("invalid-worker".to_owned(), invalid_worker);
        agent
            .workers
            .insert("valid-worker".to_owned(), valid_worker);

        agent.poll_once().await.unwrap();

        assert!(!agent.workers.contains_key("invalid-worker"));
        assert!(agent.workers.contains_key("valid-worker"));
        assert_eq!(
            agent
                .service_state("invalid-worker")
                .unwrap()
                .observed_state,
            tm_trading_domain::ObservedState::Failed
        );
        assert!(
            agent
                .service_state("invalid-worker")
                .unwrap()
                .next_restart_at
                .is_none()
        );
        let mut valid_worker = agent.workers.remove("valid-worker").unwrap();
        let _ = valid_worker.child.start_kill();
        let _ = valid_worker.child.wait().await;
    }

    #[tokio::test]
    async fn malformed_worker_protocol_is_quarantined_without_automatic_retry() {
        let definition = sample_worker_definition();
        let state_dir = unique_state_dir();
        let mut agent = Agent::new(
            "local-agent",
            std::env::temp_dir(),
            &state_dir,
            WorkerRegistry::new(vec![definition.clone()]),
        )
        .unwrap();
        let (worker, event_tx) =
            test_managed_worker(&definition, "sample-worker@local-agent:1").await;
        event_tx
            .send(WorkerEvent::ProtocolError(
                "invalid worker protocol message: expected JSON".to_owned(),
            ))
            .await
            .unwrap();
        agent.workers.insert(definition.id.to_owned(), worker);

        agent.poll_once().await.unwrap();

        assert!(!agent.workers.contains_key(definition.id));
        let state = agent.service_state(definition.id).unwrap();
        assert_eq!(
            state.observed_state,
            tm_trading_domain::ObservedState::Failed
        );
        assert!(state.retry_blocked);
        assert!(state.next_restart_at.is_none());
        let checkpoint: serde_json::Value =
            serde_json::from_slice(&fs::read(&agent.state_file).unwrap()).unwrap();
        assert_eq!(checkpoint["services"][0]["retryBlocked"], true);
    }

    #[tokio::test]
    async fn a_protocol_stream_read_error_is_retryable_and_restarts_the_worker() {
        let definition = sample_worker_definition();
        let state_dir = unique_state_dir();
        let mut agent = Agent::new(
            "local-agent",
            std::env::temp_dir(),
            &state_dir,
            WorkerRegistry::new(vec![definition.clone()]),
        )
        .unwrap();
        let (worker, event_tx) =
            test_managed_worker(&definition, "sample-worker@local-agent:1").await;
        event_tx
            .send(WorkerEvent::ProtocolStreamError(
                "failed to read worker protocol output".to_owned(),
            ))
            .await
            .unwrap();
        agent.workers.insert(definition.id.to_owned(), worker);

        agent.poll_once().await.unwrap();

        assert!(!agent.workers.contains_key(definition.id));
        let state = agent.service_state(definition.id).unwrap();
        assert_eq!(
            state.observed_state,
            tm_trading_domain::ObservedState::Failed
        );
        assert!(!state.retry_blocked);
        assert_eq!(state.restart_count, 1);
        assert!(state.next_restart_at.is_some());
    }

    #[tokio::test]
    async fn polling_bounds_events_processed_from_one_worker() {
        let definition = sample_worker_definition();
        let state_dir = unique_state_dir();
        let mut agent = Agent::new(
            "local-agent",
            std::env::temp_dir(),
            &state_dir,
            WorkerRegistry::new(vec![definition.clone()]),
        )
        .unwrap();
        let (worker, event_tx) =
            test_managed_worker(&definition, "sample-worker@local-agent:1").await;
        for index in 0..MAX_WORKER_EVENTS_PER_BATCH + 1 {
            event_tx
                .send(WorkerEvent::Log(format!("event-{index}")))
                .await
                .unwrap();
        }
        agent.workers.insert(definition.id.to_owned(), worker);

        agent.poll_once().await.unwrap();

        let mut worker = agent.workers.remove(definition.id).unwrap();
        let mut remaining = 0;
        while worker.events.try_recv().is_ok() {
            remaining += 1;
        }
        assert_eq!(remaining, 1);
        let _ = worker.child.start_kill();
        let _ = worker.child.wait().await;
    }

    #[tokio::test]
    async fn run_until_shutdown_returns_after_a_shutdown_request() {
        let state_dir = unique_state_dir();
        let mut agent = Agent::new(
            "local-agent",
            std::env::temp_dir(),
            &state_dir,
            WorkerRegistry::sample(),
        )
        .unwrap();
        let shutdown = agent.shutdown_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = shutdown.send(());
        });

        tokio::time::timeout(
            Duration::from_secs(1),
            agent.run_until_shutdown(Duration::from_millis(10), None),
        )
        .await
        .unwrap()
        .unwrap();
    }

    #[tokio::test]
    async fn shutdown_during_replacement_terminates_and_checkpoints_the_worker() {
        let mut definition = sample_worker_definition();
        definition.executable_name = if cfg!(windows) { "sort.exe" } else { "cat" };
        definition.startup_timeout = Duration::from_secs(30);
        definition.stop_grace = Duration::from_millis(100);
        let state_dir = unique_state_dir();
        let worker_dir = test_waiting_executable().parent().unwrap().to_path_buf();
        let mut agent = Agent::new(
            "local-agent",
            worker_dir,
            &state_dir,
            WorkerRegistry::new(vec![definition.clone()]),
        )
        .unwrap();
        let mut state = ServiceState::new(definition.id, "sample-worker@local-agent:1");
        state.request_start();
        state.process_alive = Some(false);
        state.next_restart_at = Some(Utc::now());
        agent.stopped_states.insert(definition.id.to_owned(), state);
        let shutdown = agent.shutdown_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = shutdown.send(());
        });

        let result = agent.start_due_restarts().await;

        assert!(
            matches!(result, Err(AgentError::ShutdownRequested)),
            "{result:?}"
        );
        assert!(!agent.workers.contains_key(definition.id));
        let state = agent.service_state(definition.id).unwrap();
        assert_eq!(state.desired_state, DesiredState::Running);
        assert_eq!(
            state.observed_state,
            tm_trading_domain::ObservedState::Stopped
        );
        assert!(state.next_restart_at.is_none());
        assert!(agent.state_file.exists());
    }

    #[tokio::test]
    async fn automatic_restart_has_a_bounded_startup_deadline() {
        let executable = test_waiting_executable();
        let worker_dir = executable.parent().unwrap().to_path_buf();
        let mut definition = sample_worker_definition();
        definition.executable_name = if cfg!(windows) { "sort.exe" } else { "cat" };
        definition.startup_timeout = Duration::from_millis(100);
        definition.stop_grace = Duration::from_millis(100);
        let state_dir = unique_state_dir();
        let mut agent = Agent::new(
            "local-agent",
            worker_dir,
            &state_dir,
            WorkerRegistry::new(vec![definition.clone()]),
        )
        .unwrap();
        let mut state = ServiceState::new(definition.id, "sample-worker@local-agent:1");
        state.request_start();
        state.process_alive = Some(false);
        state.next_restart_at = Some(Utc::now());
        agent.stopped_states.insert(definition.id.to_owned(), state);

        agent.start_due_restarts().await.unwrap();

        assert!(!agent.workers.contains_key(definition.id));
        let state = agent.service_state(definition.id).unwrap();
        assert_eq!(
            state.observed_state,
            tm_trading_domain::ObservedState::Failed
        );
        assert!(state.next_restart_at.is_some());
    }
}
