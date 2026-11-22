use std::collections::VecDeque;
use std::time::Duration;

use chrono::{DateTime, Utc};
use thiserror::Error;

use tm_trading_agent_protocol::{WORKER_SCHEMA_VERSION, WorkerEvidence, WorkerMessage};
use tm_trading_domain::{DesiredState, ObservedState, Readiness};

pub const SAMPLE_WORKER_ID: &str = "sample-worker";
pub const MARKET_WORKER_ID: &str = "market-worker";
pub const RESEARCH_WORKER_ID: &str = "research-worker";
pub const NEWS_WORKER_ID: &str = "news-worker";
pub const AI_WORKER_ID: &str = "ai-worker";
pub const MAX_LOG_ENTRIES: usize = 100;
pub const MAX_LOG_LINE_CHARS: usize = 1_000;
const FUTURE_TIMESTAMP_SKEW: Duration = Duration::from_secs(5);

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SupervisorError {
    #[error("unsupported worker schema {actual}; expected {expected}")]
    UnsupportedSchema {
        expected: &'static str,
        actual: String,
    },
    #[error("worker instance mismatch: expected {expected}, got {actual}")]
    InstanceMismatch { expected: String, actual: String },
    #[error("worker must complete the {expected} handshake first")]
    HandshakeRequired { expected: &'static str },
    #[error("worker handshake was already completed")]
    HandshakeAlreadyComplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceState {
    Passed,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessCheck {
    pub id: &'static str,
    pub required: bool,
    pub state: EvidenceState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessReport {
    pub readiness: Readiness,
    pub checks: Vec<ReadinessCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadinessRule {
    pub id: &'static str,
    pub required: bool,
    pub max_age: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerDefinition {
    pub id: &'static str,
    pub executable_name: &'static str,
    pub executable_version: &'static str,
    pub readiness_rules: Vec<ReadinessRule>,
    pub startup_timeout: Duration,
    pub stop_grace: Duration,
    pub max_restart_attempts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRegistry {
    workers: Vec<WorkerDefinition>,
}

impl WorkerRegistry {
    #[must_use]
    pub fn new(workers: Vec<WorkerDefinition>) -> Self {
        Self { workers }
    }

    #[must_use]
    pub fn sample_with_version(executable_version: &'static str) -> Self {
        let mut definition = sample_worker_definition();
        definition.executable_version = executable_version;
        Self {
            workers: vec![definition],
        }
    }

    #[must_use]
    pub fn sample() -> Self {
        Self {
            workers: vec![sample_worker_definition()],
        }
    }

    /// Registry for the local Agent: the sample worker plus the enabled domain workers.
    ///
    /// Domain workers are registered only when explicitly enabled, so a default installation
    /// does not run a service that cannot collect or scan anything.
    #[must_use]
    pub fn local_with_version(
        executable_version: &'static str,
        market_worker_enabled: bool,
        research_worker_enabled: bool,
        news_worker_enabled: bool,
        ai_worker_enabled: bool,
    ) -> Self {
        let mut sample = sample_worker_definition();
        sample.executable_version = executable_version;
        let mut workers = vec![sample];
        if market_worker_enabled {
            let mut market = market_worker_definition();
            market.executable_version = executable_version;
            workers.push(market);
        }
        if research_worker_enabled {
            let mut research = research_worker_definition();
            research.executable_version = executable_version;
            workers.push(research);
        }
        if news_worker_enabled {
            let mut news = news_worker_definition();
            news.executable_version = executable_version;
            workers.push(news);
        }
        if ai_worker_enabled {
            let mut ai = ai_worker_definition();
            ai.executable_version = executable_version;
            workers.push(ai);
        }
        Self { workers }
    }

    #[must_use]
    pub fn get(&self, service_definition_id: &str) -> Option<&WorkerDefinition> {
        self.workers
            .iter()
            .find(|worker| worker.id == service_definition_id)
    }

    /// All registered worker definitions in registration order.
    pub fn iter(&self) -> impl Iterator<Item = &WorkerDefinition> {
        self.workers.iter()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub at: DateTime<Utc>,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerProcessIdentity {
    pub pid: u32,
    pub executable_path: String,
    pub executable_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceState {
    pub service_definition_id: String,
    pub service_instance_id: String,
    pub desired_state: DesiredState,
    pub observed_state: ObservedState,
    pub readiness: Readiness,
    pub process_alive: Option<bool>,
    pub process_identity: Option<WorkerProcessIdentity>,
    handshake_complete: bool,
    pub evidence: WorkerEvidence,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_exit_code: Option<i32>,
    pub restart_count: u32,
    pub next_restart_at: Option<DateTime<Utc>>,
    pub retry_blocked: bool,
    pub logs: VecDeque<LogEntry>,
}

impl ServiceState {
    #[must_use]
    pub fn new(service_definition_id: &str, service_instance_id: &str) -> Self {
        Self {
            service_definition_id: service_definition_id.to_owned(),
            service_instance_id: service_instance_id.to_owned(),
            desired_state: DesiredState::Stopped,
            observed_state: ObservedState::Unknown,
            readiness: Readiness::Unknown,
            process_alive: None,
            process_identity: None,
            handshake_complete: false,
            evidence: WorkerEvidence::default(),
            last_heartbeat_at: None,
            last_success_at: None,
            last_exit_code: None,
            restart_count: 0,
            next_restart_at: None,
            retry_blocked: false,
            logs: VecDeque::new(),
        }
    }

    pub fn request_start(&mut self) {
        self.desired_state = DesiredState::Running;
        self.observed_state = ObservedState::Starting;
        self.readiness = Readiness::Unknown;
        self.process_alive = None;
        self.process_identity = None;
        self.handshake_complete = false;
        self.evidence = WorkerEvidence::default();
        self.last_heartbeat_at = None;
        self.last_success_at = None;
        self.next_restart_at = None;
        self.retry_blocked = false;
    }

    pub fn request_stop(&mut self) {
        self.desired_state = DesiredState::Stopped;
        self.observed_state = if self.process_alive == Some(true) {
            ObservedState::Stopping
        } else {
            ObservedState::Stopped
        };
        self.readiness = Readiness::NotReady;
        self.next_restart_at = None;
        self.retry_blocked = false;
    }

    pub fn request_process_stop(&mut self) {
        self.observed_state = if self.process_alive == Some(true) {
            ObservedState::Stopping
        } else {
            ObservedState::Stopped
        };
        self.readiness = Readiness::NotReady;
    }

    pub fn apply_agent_shutdown_exit(&mut self, exit_code: i32) {
        self.process_alive = Some(false);
        self.last_exit_code = Some(exit_code);
        self.last_heartbeat_at = None;
        self.readiness = Readiness::NotReady;
        self.observed_state = ObservedState::Stopped;
    }

    pub fn quarantine(&mut self, exit_code: Option<i32>) {
        self.process_alive = Some(false);
        if let Some(exit_code) = exit_code {
            self.last_exit_code = Some(exit_code);
        }
        self.last_heartbeat_at = None;
        self.readiness = Readiness::NotReady;
        self.next_restart_at = None;
        self.observed_state = ObservedState::Failed;
        self.retry_blocked = true;
    }

    pub fn apply_worker_message(
        &mut self,
        definition: &WorkerDefinition,
        message: &WorkerMessage,
        now: DateTime<Utc>,
    ) -> Result<(), SupervisorError> {
        let actual_instance_id = message.instance_id();
        if actual_instance_id != self.service_instance_id {
            return Err(SupervisorError::InstanceMismatch {
                expected: self.service_instance_id.clone(),
                actual: actual_instance_id.to_owned(),
            });
        }
        if self.handshake_complete && matches!(message, WorkerMessage::Hello { .. }) {
            return Err(SupervisorError::HandshakeAlreadyComplete);
        }
        if !self.handshake_complete && !matches!(message, WorkerMessage::Hello { .. }) {
            return Err(SupervisorError::HandshakeRequired {
                expected: WORKER_SCHEMA_VERSION,
            });
        }

        match message {
            WorkerMessage::Hello {
                at: _,
                pid,
                executable_path,
                executable_version,
                schema_version,
                ..
            } => {
                if schema_version != WORKER_SCHEMA_VERSION {
                    return Err(SupervisorError::UnsupportedSchema {
                        expected: WORKER_SCHEMA_VERSION,
                        actual: schema_version.clone(),
                    });
                }
                self.process_alive = Some(true);
                self.process_identity = Some(WorkerProcessIdentity {
                    pid: *pid,
                    executable_path: executable_path.clone(),
                    executable_version: executable_version.clone(),
                });
                self.handshake_complete = true;
                self.observed_state = if self.desired_state == DesiredState::Stopped {
                    ObservedState::Stopping
                } else {
                    ObservedState::Starting
                };
                self.readiness = Readiness::NotReady;
            }
            WorkerMessage::Heartbeat { at, evidence, .. } => {
                self.update_evidence(evidence);
                self.process_alive = Some(true);
                self.last_heartbeat_at = Some(*at);
                if self.desired_state == DesiredState::Running
                    && self.observed_state != ObservedState::Stopping
                {
                    self.observed_state = ObservedState::Running;
                }
                self.refresh_readiness(definition, now);
            }
            WorkerMessage::Ready {
                at: _, evidence, ..
            } => {
                self.update_evidence(evidence);
                self.process_alive = Some(true);
                if self.desired_state == DesiredState::Running {
                    self.observed_state = ObservedState::Running;
                }
                self.refresh_readiness(definition, now);
            }
            WorkerMessage::Cycle {
                instance_id: _,
                at,
                ok,
                skipped,
                duration_ms,
                output_count,
                evidence,
                error,
                summary,
            } => {
                self.update_evidence(evidence);
                self.process_alive = Some(true);
                if self.desired_state == DesiredState::Running {
                    self.observed_state = ObservedState::Running;
                }
                if *ok && skipped.is_none() && error.is_none() {
                    self.last_success_at = Some(*at);
                    self.evidence.last_success_at = Some(*at);
                    self.evidence.last_output_count = *output_count;
                } else if let Some(error) = error {
                    self.record_log(*at, format!("cycle failed: {error}"));
                } else if let Some(skipped) = skipped {
                    self.record_log(*at, format!("cycle skipped: {skipped}"));
                }
                let _ = (duration_ms, summary);
                self.refresh_readiness(definition, now);
            }
            WorkerMessage::OperationResult {
                ok,
                result,
                evidence,
                error,
                ..
            } => {
                self.update_evidence(evidence);
                self.process_alive = Some(true);
                if *ok && result.skipped.is_none() && error.is_none() {
                    self.last_success_at = Some(now);
                    self.evidence.last_success_at = Some(now);
                } else if let Some(error) = error {
                    self.record_log(now, format!("operation failed: {error}"));
                }
                self.refresh_readiness(definition, now);
            }
            WorkerMessage::Stopping { at, reason, .. } => {
                self.observed_state = ObservedState::Stopping;
                self.readiness = Readiness::NotReady;
                self.record_log(*at, format!("worker is stopping: {reason}"));
            }
            WorkerMessage::Stopped {
                at,
                reason,
                drained,
                ..
            } => {
                self.process_alive = Some(false);
                self.observed_state = ObservedState::Stopped;
                self.readiness = Readiness::NotReady;
                self.last_heartbeat_at = None;
                self.record_log(*at, format!("worker stopped: {reason}; drained={drained}"));
            }
            WorkerMessage::Fatal { at, error, .. } => {
                self.observed_state = ObservedState::Failed;
                self.readiness = Readiness::NotReady;
                self.record_log(*at, format!("worker fatal error: {error}"));
            }
        }

        Ok(())
    }

    pub fn apply_exit(
        &mut self,
        definition: &WorkerDefinition,
        exit_code: i32,
        now: DateTime<Utc>,
        jitter_percent: u32,
    ) {
        self.process_alive = Some(false);
        self.last_exit_code = Some(exit_code);
        self.last_heartbeat_at = None;
        self.readiness = Readiness::NotReady;
        self.next_restart_at = None;

        if self.desired_state == DesiredState::Running {
            self.observed_state = ObservedState::Failed;
            if !self.retry_blocked {
                self.restart_count = self.restart_count.saturating_add(1);
                if self.restart_count <= definition.max_restart_attempts {
                    let delay = restart_backoff(self.restart_count, jitter_percent);
                    self.next_restart_at = now.checked_add_signed(
                        chrono::Duration::from_std(delay)
                            .unwrap_or_else(|_| chrono::Duration::zero()),
                    );
                }
            }
        } else {
            self.observed_state = ObservedState::Stopped;
        }
    }

    pub fn prepare_restart(&mut self, service_instance_id: &str) {
        self.service_instance_id = service_instance_id.to_owned();
        self.desired_state = DesiredState::Running;
        self.observed_state = ObservedState::Starting;
        self.readiness = Readiness::Unknown;
        self.process_alive = None;
        self.process_identity = None;
        self.handshake_complete = false;
        self.evidence = WorkerEvidence::default();
        self.last_heartbeat_at = None;
        self.last_success_at = None;
        self.next_restart_at = None;
        self.retry_blocked = false;
    }

    #[must_use]
    pub fn restart_due(&self, now: DateTime<Utc>) -> bool {
        self.next_restart_at
            .is_some_and(|restart_at| restart_at <= now)
    }

    pub fn record_log(&mut self, at: DateTime<Utc>, line: String) {
        let mut line = line;
        if line.chars().count() > MAX_LOG_LINE_CHARS {
            line = line
                .chars()
                .take(MAX_LOG_LINE_CHARS.saturating_sub(3))
                .collect::<String>()
                + "...";
        }
        if self.logs.len() == MAX_LOG_ENTRIES {
            self.logs.pop_front();
        }
        self.logs.push_back(LogEntry { at, line });
    }

    fn update_evidence(&mut self, evidence: &WorkerEvidence) {
        self.evidence = evidence.clone();
        if let Some(last_success_at) = evidence.last_success_at {
            self.last_success_at = Some(last_success_at);
        }
    }

    pub fn refresh_readiness(&mut self, definition: &WorkerDefinition, now: DateTime<Utc>) {
        self.readiness = evaluate_readiness(
            definition,
            self.process_alive,
            self.last_heartbeat_at,
            &self.evidence,
            now,
        )
        .readiness;
    }
}

#[must_use]
pub fn sample_worker_definition() -> WorkerDefinition {
    WorkerDefinition {
        id: SAMPLE_WORKER_ID,
        executable_name: if cfg!(windows) {
            "tm-trading-sample-worker.exe"
        } else {
            "tm-trading-sample-worker"
        },
        executable_version: env!("CARGO_PKG_VERSION"),
        readiness_rules: vec![
            ReadinessRule {
                id: "process-alive",
                required: true,
                max_age: None,
            },
            ReadinessRule {
                id: "capability-verified",
                required: true,
                max_age: None,
            },
            ReadinessRule {
                id: "recent-heartbeat",
                required: true,
                max_age: Some(Duration::from_secs(30)),
            },
            ReadinessRule {
                id: "recent-success",
                required: false,
                max_age: Some(Duration::from_secs(60)),
            },
        ],
        startup_timeout: Duration::from_secs(120),
        stop_grace: Duration::from_secs(15),
        max_restart_attempts: 5,
    }
}

#[must_use]
pub fn market_worker_definition() -> WorkerDefinition {
    WorkerDefinition {
        id: MARKET_WORKER_ID,
        executable_name: if cfg!(windows) {
            "tm-trading-market-worker.exe"
        } else {
            "tm-trading-market-worker"
        },
        executable_version: env!("CARGO_PKG_VERSION"),
        readiness_rules: vec![
            ReadinessRule {
                id: "process-alive",
                required: true,
                max_age: None,
            },
            ReadinessRule {
                id: "capability-verified",
                required: true,
                max_age: None,
            },
            ReadinessRule {
                id: "recent-heartbeat",
                required: true,
                max_age: Some(Duration::from_secs(30)),
            },
            ReadinessRule {
                id: "dependencies-reachable",
                required: true,
                max_age: None,
            },
            ReadinessRule {
                id: "sink-writable",
                required: true,
                max_age: None,
            },
            // A stale collection is a stale market view, so the worker stops being ready
            // when the last successful cycle ages out.
            ReadinessRule {
                id: "recent-success",
                required: true,
                max_age: Some(Duration::from_secs(600)),
            },
        ],
        startup_timeout: Duration::from_secs(120),
        stop_grace: Duration::from_secs(15),
        max_restart_attempts: 5,
    }
}

#[must_use]
pub fn research_worker_definition() -> WorkerDefinition {
    WorkerDefinition {
        id: RESEARCH_WORKER_ID,
        executable_name: if cfg!(windows) {
            "tm-trading-research-worker.exe"
        } else {
            "tm-trading-research-worker"
        },
        executable_version: env!("CARGO_PKG_VERSION"),
        readiness_rules: vec![
            ReadinessRule {
                id: "process-alive",
                required: true,
                max_age: None,
            },
            ReadinessRule {
                id: "capability-verified",
                required: true,
                max_age: None,
            },
            ReadinessRule {
                id: "recent-heartbeat",
                required: true,
                max_age: Some(Duration::from_secs(30)),
            },
            ReadinessRule {
                id: "dependencies-reachable",
                required: true,
                max_age: None,
            },
            ReadinessRule {
                id: "sink-writable",
                required: true,
                max_age: None,
            },
            ReadinessRule {
                id: "recent-success",
                required: true,
                max_age: Some(Duration::from_secs(600)),
            },
        ],
        startup_timeout: Duration::from_secs(120),
        stop_grace: Duration::from_secs(15),
        max_restart_attempts: 5,
    }
}

#[must_use]
pub fn news_worker_definition() -> WorkerDefinition {
    WorkerDefinition {
        id: NEWS_WORKER_ID,
        executable_name: if cfg!(windows) {
            "tm-trading-news-worker.exe"
        } else {
            "tm-trading-news-worker"
        },
        executable_version: env!("CARGO_PKG_VERSION"),
        readiness_rules: vec![
            ReadinessRule {
                id: "process-alive",
                required: true,
                max_age: None,
            },
            ReadinessRule {
                id: "capability-verified",
                required: true,
                max_age: None,
            },
            ReadinessRule {
                id: "recent-heartbeat",
                required: true,
                max_age: Some(Duration::from_secs(30)),
            },
            ReadinessRule {
                id: "dependencies-reachable",
                required: true,
                max_age: None,
            },
            ReadinessRule {
                id: "sink-writable",
                required: true,
                max_age: None,
            },
            // Legacy readiness requires a recent successful collection, so a stale
            // news view is never presented as ready.
            ReadinessRule {
                id: "recent-success",
                required: true,
                max_age: Some(Duration::from_secs(1_800)),
            },
        ],
        startup_timeout: Duration::from_secs(120),
        stop_grace: Duration::from_secs(15),
        max_restart_attempts: 5,
    }
}

#[must_use]
pub fn ai_worker_definition() -> WorkerDefinition {
    WorkerDefinition {
        id: AI_WORKER_ID,
        executable_name: if cfg!(windows) {
            "tm-trading-ai-worker.exe"
        } else {
            "tm-trading-ai-worker"
        },
        executable_version: env!("CARGO_PKG_VERSION"),
        readiness_rules: vec![
            ReadinessRule {
                id: "process-alive",
                required: true,
                max_age: None,
            },
            ReadinessRule {
                id: "capability-verified",
                required: true,
                max_age: None,
            },
            ReadinessRule {
                id: "recent-heartbeat",
                required: true,
                max_age: Some(Duration::from_secs(30)),
            },
            ReadinessRule {
                id: "dependencies-reachable",
                required: true,
                max_age: None,
            },
            // The AI worker is idle between queued jobs, so a missing briefing is not a
            // failure. A stale one only degrades the service.
            ReadinessRule {
                id: "recent-success",
                required: false,
                max_age: Some(Duration::from_secs(3_600)),
            },
        ],
        startup_timeout: Duration::from_secs(120),
        stop_grace: Duration::from_secs(15),
        max_restart_attempts: 5,
    }
}

#[must_use]
pub fn evaluate_readiness(
    definition: &WorkerDefinition,
    process_alive: Option<bool>,
    last_heartbeat_at: Option<DateTime<Utc>>,
    evidence: &WorkerEvidence,
    now: DateTime<Utc>,
) -> ReadinessReport {
    let checks = definition
        .readiness_rules
        .iter()
        .map(|rule| ReadinessCheck {
            id: rule.id,
            required: rule.required,
            state: evaluate_rule(rule, process_alive, last_heartbeat_at, evidence, now),
        })
        .collect::<Vec<_>>();

    let readiness = if process_alive.is_none() {
        Readiness::Unknown
    } else if checks
        .iter()
        .any(|check| check.required && check.state != EvidenceState::Passed)
    {
        Readiness::NotReady
    } else if checks
        .iter()
        .any(|check| !check.required && check.state == EvidenceState::Failed)
    {
        Readiness::Degraded
    } else {
        Readiness::Ready
    };

    ReadinessReport { readiness, checks }
}

fn evaluate_rule(
    rule: &ReadinessRule,
    process_alive: Option<bool>,
    last_heartbeat_at: Option<DateTime<Utc>>,
    evidence: &WorkerEvidence,
    now: DateTime<Utc>,
) -> EvidenceState {
    match rule.id {
        "process-alive" => match process_alive {
            Some(true) => EvidenceState::Passed,
            Some(false) => EvidenceState::Failed,
            None => EvidenceState::Unknown,
        },
        "capability-verified" => match evidence.capability_verified {
            Some(true) => EvidenceState::Passed,
            Some(false) => EvidenceState::Failed,
            None => EvidenceState::Unknown,
        },
        "recent-heartbeat" => evaluate_timestamp(last_heartbeat_at, rule.max_age, now),
        "recent-success" => evaluate_timestamp(evidence.last_success_at, rule.max_age, now),
        "dependencies-reachable" => evaluate_boolean(evidence.dependencies_reachable),
        "sink-writable" => evaluate_boolean(evidence.sink_writable),
        _ => EvidenceState::Unknown,
    }
}

fn evaluate_boolean(value: Option<bool>) -> EvidenceState {
    match value {
        Some(true) => EvidenceState::Passed,
        Some(false) => EvidenceState::Failed,
        None => EvidenceState::Unknown,
    }
}

fn evaluate_timestamp(
    observed_at: Option<DateTime<Utc>>,
    max_age: Option<Duration>,
    now: DateTime<Utc>,
) -> EvidenceState {
    let Some(observed_at) = observed_at else {
        return EvidenceState::Unknown;
    };
    if observed_at > now + chrono::Duration::from_std(FUTURE_TIMESTAMP_SKEW).unwrap_or_default() {
        return EvidenceState::Failed;
    }
    let Some(max_age) = max_age else {
        return EvidenceState::Passed;
    };
    let max_age = chrono::Duration::from_std(max_age).unwrap_or_default();
    if now - observed_at > max_age {
        EvidenceState::Failed
    } else {
        EvidenceState::Passed
    }
}

#[must_use]
pub fn restart_backoff(attempt: u32, jitter_percent: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(6);
    let base_seconds = 2_u64.checked_shl(exponent).unwrap_or(60);
    let jitter_seconds = base_seconds.saturating_mul(u64::from(jitter_percent.min(100))) / 200;
    Duration::from_secs(base_seconds.saturating_add(jitter_seconds).min(60))
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};

    use tm_trading_agent_protocol::WorkerMessage;

    use super::*;

    fn state() -> ServiceState {
        ServiceState::new("sample-worker", "sample-worker@local-agent:1")
    }

    #[test]
    fn domain_workers_are_only_registered_when_they_are_enabled() {
        let disabled = WorkerRegistry::local_with_version("0.1.0", false, false, false, false);
        assert_eq!(disabled.iter().count(), 1);
        assert!(disabled.get(SAMPLE_WORKER_ID).is_some());
        assert!(disabled.get(MARKET_WORKER_ID).is_none());
        assert!(disabled.get(RESEARCH_WORKER_ID).is_none());
        assert!(disabled.get(NEWS_WORKER_ID).is_none());
        assert!(disabled.get(AI_WORKER_ID).is_none());

        let news = WorkerRegistry::local_with_version("0.1.0", false, false, true, false);
        assert_eq!(news.iter().count(), 2);
        let news = news.get(NEWS_WORKER_ID).expect("news worker");
        assert_eq!(news.executable_version, "0.1.0");
        assert!(
            news.readiness_rules
                .iter()
                .any(|rule| rule.id == "recent-success" && rule.required)
        );

        let market = WorkerRegistry::local_with_version("0.1.0", true, false, false, false);
        assert_eq!(market.iter().count(), 2);
        let market = market.get(MARKET_WORKER_ID).expect("market worker");
        assert_eq!(market.executable_version, "0.1.0");
        assert!(
            market
                .readiness_rules
                .iter()
                .any(|rule| rule.id == "recent-success" && rule.required)
        );

        let research = WorkerRegistry::local_with_version("0.1.0", false, true, false, false);
        assert_eq!(research.iter().count(), 2);
        let research = research.get(RESEARCH_WORKER_ID).expect("research worker");
        assert_eq!(research.executable_version, "0.1.0");
        assert!(
            research
                .readiness_rules
                .iter()
                .any(|rule| rule.id == "recent-success" && rule.required)
        );

        let ai = WorkerRegistry::local_with_version("0.1.0", false, false, false, true);
        assert_eq!(ai.iter().count(), 2);
        let ai = ai.get(AI_WORKER_ID).expect("ai worker");
        assert_eq!(ai.executable_version, "0.1.0");
        assert!(
            ai.readiness_rules
                .iter()
                .any(|rule| rule.id == "recent-success" && !rule.required)
        );
    }

    #[test]
    fn a_market_provider_failure_revokes_readiness_immediately() {
        let definition = market_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 20, 0, 0, 0).unwrap();
        let evidence = WorkerEvidence {
            capability_verified: Some(true),
            dependencies_reachable: Some(false),
            sink_writable: Some(true),
            last_success_at: Some(now),
            ..WorkerEvidence::default()
        };

        let report = evaluate_readiness(&definition, Some(true), Some(now), &evidence, now);

        assert_eq!(report.readiness, Readiness::NotReady);
        assert!(report.checks.iter().any(|check| {
            check.id == "dependencies-reachable" && check.state == EvidenceState::Failed
        }));
    }

    #[test]
    fn missing_required_evidence_is_not_ready() {
        let definition = sample_worker_definition();
        let evidence = WorkerEvidence::default();
        let now = Utc::now();

        let report = evaluate_readiness(&definition, Some(true), Some(now), &evidence, now);

        assert_eq!(report.readiness, Readiness::NotReady);
        assert!(report.checks.iter().any(|check| {
            check.id == "capability-verified" && check.state == EvidenceState::Unknown
        }));
    }

    #[test]
    fn missing_optional_evidence_does_not_degrade_readiness() {
        let definition = sample_worker_definition();
        let evidence = WorkerEvidence {
            capability_verified: Some(true),
            ..WorkerEvidence::default()
        };
        let now = Utc::now();

        let report = evaluate_readiness(&definition, Some(true), Some(now), &evidence, now);

        assert_eq!(report.readiness, Readiness::Ready);
    }

    #[test]
    fn stale_optional_evidence_degrades_readiness() {
        let definition = sample_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let evidence = WorkerEvidence {
            capability_verified: Some(true),
            last_success_at: Some(now - Duration::minutes(2)),
            ..WorkerEvidence::default()
        };

        let report = evaluate_readiness(&definition, Some(true), Some(now), &evidence, now);

        assert_eq!(report.readiness, Readiness::Degraded);
    }

    #[test]
    fn stale_heartbeat_is_not_ready_even_when_the_process_is_alive() {
        let definition = sample_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let evidence = WorkerEvidence {
            capability_verified: Some(true),
            ..WorkerEvidence::default()
        };

        let report = evaluate_readiness(
            &definition,
            Some(true),
            Some(now - Duration::seconds(31)),
            &evidence,
            now,
        );

        assert_eq!(report.readiness, Readiness::NotReady);
    }

    #[test]
    fn hello_starts_the_service_without_claiming_readiness() {
        let definition = sample_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = state();
        state.request_start();

        state
            .apply_worker_message(
                &definition,
                &WorkerMessage::Hello {
                    instance_id: state.service_instance_id.clone(),
                    at: now,
                    pid: 42,
                    executable_path: "sample-worker".to_owned(),
                    executable_version: env!("CARGO_PKG_VERSION").to_owned(),
                    schema_version: WORKER_SCHEMA_VERSION.to_owned(),
                },
                now,
            )
            .unwrap();

        assert_eq!(state.observed_state, ObservedState::Starting);
        assert_eq!(state.readiness, Readiness::NotReady);
        assert_eq!(state.process_alive, Some(true));
    }

    #[test]
    fn ready_message_requires_a_separate_heartbeat() {
        let definition = sample_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = state();
        state.request_start();
        state
            .apply_worker_message(
                &definition,
                &WorkerMessage::Hello {
                    instance_id: state.service_instance_id.clone(),
                    at: now,
                    pid: 42,
                    executable_path: "sample-worker".to_owned(),
                    executable_version: env!("CARGO_PKG_VERSION").to_owned(),
                    schema_version: WORKER_SCHEMA_VERSION.to_owned(),
                },
                now,
            )
            .unwrap();
        let evidence = WorkerEvidence {
            capability_verified: Some(true),
            ..WorkerEvidence::default()
        };

        state
            .apply_worker_message(
                &definition,
                &WorkerMessage::Ready {
                    instance_id: state.service_instance_id.clone(),
                    at: now,
                    evidence,
                },
                now,
            )
            .unwrap();

        assert_eq!(state.observed_state, ObservedState::Running);
        assert_eq!(state.readiness, Readiness::NotReady);

        state
            .apply_worker_message(
                &definition,
                &WorkerMessage::Heartbeat {
                    instance_id: state.service_instance_id.clone(),
                    at: now,
                    evidence: WorkerEvidence {
                        capability_verified: Some(true),
                        ..WorkerEvidence::default()
                    },
                },
                now,
            )
            .unwrap();

        assert_eq!(state.readiness, Readiness::Ready);
    }

    #[test]
    fn stale_heartbeat_is_evaluated_at_agent_receipt_time() {
        let definition = sample_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = state();
        state.request_start();
        state
            .apply_worker_message(
                &definition,
                &WorkerMessage::Hello {
                    instance_id: state.service_instance_id.clone(),
                    at: now,
                    pid: 42,
                    executable_path: "sample-worker".to_owned(),
                    executable_version: env!("CARGO_PKG_VERSION").to_owned(),
                    schema_version: WORKER_SCHEMA_VERSION.to_owned(),
                },
                now,
            )
            .unwrap();
        let evidence = WorkerEvidence {
            capability_verified: Some(true),
            ..WorkerEvidence::default()
        };

        state
            .apply_worker_message(
                &definition,
                &WorkerMessage::Heartbeat {
                    instance_id: state.service_instance_id.clone(),
                    at: now - Duration::seconds(31),
                    evidence,
                },
                now,
            )
            .unwrap();

        assert_eq!(state.readiness, Readiness::NotReady);
    }

    #[test]
    fn future_heartbeat_is_evaluated_at_agent_receipt_time() {
        let definition = sample_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = state();
        state.request_start();
        state
            .apply_worker_message(
                &definition,
                &WorkerMessage::Hello {
                    instance_id: state.service_instance_id.clone(),
                    at: now,
                    pid: 42,
                    executable_path: "sample-worker".to_owned(),
                    executable_version: env!("CARGO_PKG_VERSION").to_owned(),
                    schema_version: WORKER_SCHEMA_VERSION.to_owned(),
                },
                now,
            )
            .unwrap();
        let evidence = WorkerEvidence {
            capability_verified: Some(true),
            ..WorkerEvidence::default()
        };

        state
            .apply_worker_message(
                &definition,
                &WorkerMessage::Heartbeat {
                    instance_id: state.service_instance_id.clone(),
                    at: now + Duration::seconds(10),
                    evidence,
                },
                now,
            )
            .unwrap();

        assert_eq!(state.readiness, Readiness::NotReady);
    }

    #[test]
    fn duplicate_hello_is_rejected() {
        let definition = sample_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = state();
        state.request_start();
        let hello = WorkerMessage::Hello {
            instance_id: state.service_instance_id.clone(),
            at: now,
            pid: 42,
            executable_path: "sample-worker".to_owned(),
            executable_version: env!("CARGO_PKG_VERSION").to_owned(),
            schema_version: WORKER_SCHEMA_VERSION.to_owned(),
        };
        state
            .apply_worker_message(&definition, &hello, now)
            .unwrap();

        let error = state
            .apply_worker_message(&definition, &hello, now)
            .unwrap_err();

        assert!(matches!(error, SupervisorError::HandshakeAlreadyComplete));
    }

    #[test]
    fn stopping_without_a_live_process_is_observed_as_stopped() {
        let mut state = state();
        state.request_start();

        state.request_stop();

        assert_eq!(state.observed_state, ObservedState::Stopped);
        assert_eq!(state.readiness, Readiness::NotReady);
    }

    #[test]
    fn unsupported_worker_schema_is_rejected() {
        let definition = sample_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = state();

        let error = state
            .apply_worker_message(
                &definition,
                &WorkerMessage::Hello {
                    instance_id: state.service_instance_id.clone(),
                    at: now,
                    pid: 42,
                    executable_path: "sample-worker".to_owned(),
                    executable_version: env!("CARGO_PKG_VERSION").to_owned(),
                    schema_version: "worker.v1".to_owned(),
                },
                now,
            )
            .unwrap_err();

        assert_eq!(
            error,
            SupervisorError::UnsupportedSchema {
                expected: WORKER_SCHEMA_VERSION,
                actual: "worker.v1".to_owned(),
            }
        );
    }

    #[test]
    fn unexpected_exit_fails_the_service_and_schedules_a_restart() {
        let definition = sample_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = state();
        state.request_start();
        state.process_alive = Some(true);

        state.apply_exit(&definition, 9, now, 0);

        assert_eq!(state.observed_state, ObservedState::Failed);
        assert_eq!(state.readiness, Readiness::NotReady);
        assert_eq!(state.restart_count, 1);
        assert!(state.next_restart_at.is_some());
    }

    #[test]
    fn expected_stop_does_not_schedule_a_restart() {
        let definition = sample_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = state();
        state.request_start();
        state.process_alive = Some(true);
        state.request_stop();

        state.apply_exit(&definition, 0, now, 0);

        assert_eq!(state.observed_state, ObservedState::Stopped);
        assert_eq!(state.restart_count, 0);
        assert!(state.next_restart_at.is_none());
    }

    #[test]
    fn agent_shutdown_preserves_the_running_intent_without_consuming_restart_budget() {
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let restart_at = now + Duration::seconds(30);
        let mut state = state();
        state.request_start();
        state.process_alive = Some(true);
        state.next_restart_at = Some(restart_at);

        state.apply_agent_shutdown_exit(0);

        assert_eq!(state.desired_state, DesiredState::Running);
        assert_eq!(state.observed_state, ObservedState::Stopped);
        assert_eq!(state.readiness, Readiness::NotReady);
        assert_eq!(state.restart_count, 0);
        assert_eq!(state.next_restart_at, Some(restart_at));
    }

    #[test]
    fn a_quarantined_worker_is_not_automatically_retried() {
        let definition = sample_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = state();
        state.request_start();
        state.process_alive = Some(true);

        state.quarantine(Some(2));
        state.apply_exit(&definition, 2, now, 0);

        assert_eq!(state.desired_state, DesiredState::Running);
        assert_eq!(state.observed_state, ObservedState::Failed);
        assert_eq!(state.restart_count, 0);
        assert!(state.next_restart_at.is_none());
        assert!(state.retry_blocked);
    }

    #[test]
    fn exhausted_restart_budget_does_not_schedule_another_attempt() {
        let mut definition = sample_worker_definition();
        definition.max_restart_attempts = 1;
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = state();
        state.request_start();
        state.process_alive = Some(true);

        state.apply_exit(&definition, 9, now, 0);
        assert!(state.next_restart_at.is_some());
        state.prepare_restart("sample-worker@local-agent:2");
        state.process_alive = Some(true);
        state.apply_exit(&definition, 9, now + Duration::seconds(10), 0);

        assert_eq!(state.restart_count, 2);
        assert!(state.next_restart_at.is_none());
    }

    #[test]
    fn ready_without_a_successful_handshake_is_rejected() {
        let definition = sample_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = state();
        state.request_start();
        let evidence = WorkerEvidence {
            capability_verified: Some(true),
            ..WorkerEvidence::default()
        };

        let result = state.apply_worker_message(
            &definition,
            &WorkerMessage::Ready {
                instance_id: state.service_instance_id.clone(),
                at: now,
                evidence,
            },
            now,
        );

        assert!(result.is_err());
    }

    #[test]
    fn stale_heartbeat_refresh_makes_a_live_worker_not_ready() {
        let definition = sample_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = state();
        state.process_alive = Some(true);
        state.last_heartbeat_at = Some(now - Duration::seconds(31));
        state.readiness = Readiness::Ready;
        let evidence = WorkerEvidence {
            capability_verified: Some(true),
            ..WorkerEvidence::default()
        };
        state.evidence = evidence;

        state.refresh_readiness(&definition, now);

        assert_eq!(state.readiness, Readiness::NotReady);
    }

    #[test]
    fn a_crash_after_a_stopping_message_still_restarts_when_desired_state_is_running() {
        let definition = sample_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = state();
        state.request_start();
        state.process_alive = Some(true);
        state
            .apply_worker_message(
                &definition,
                &WorkerMessage::Hello {
                    instance_id: state.service_instance_id.clone(),
                    at: now,
                    pid: 42,
                    executable_path: "sample-worker".to_owned(),
                    executable_version: env!("CARGO_PKG_VERSION").to_owned(),
                    schema_version: WORKER_SCHEMA_VERSION.to_owned(),
                },
                now,
            )
            .unwrap();
        state
            .apply_worker_message(
                &definition,
                &WorkerMessage::Stopping {
                    instance_id: state.service_instance_id.clone(),
                    at: now,
                    reason: "worker announced shutdown".to_owned(),
                },
                now,
            )
            .unwrap();

        state.apply_exit(&definition, 9, now, 0);

        assert_eq!(state.restart_count, 1);
        assert!(state.next_restart_at.is_some());
    }

    #[test]
    fn preparing_a_restart_preserves_the_previous_exit_code() {
        let definition = sample_worker_definition();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = state();
        state.request_start();
        state.process_alive = Some(true);
        state.apply_exit(&definition, 9, now, 0);

        state.prepare_restart("sample-worker@local-agent:2");

        assert_eq!(state.last_exit_code, Some(9));
    }

    #[test]
    fn log_tail_is_bounded() {
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = state();

        for index in 0..105 {
            state.record_log(now, format!("line-{index}"));
        }

        assert_eq!(state.logs.len(), MAX_LOG_ENTRIES);
        assert_eq!(state.logs.front().unwrap().line, "line-5");
        assert_eq!(state.logs.back().unwrap().line, "line-104");
    }
}
