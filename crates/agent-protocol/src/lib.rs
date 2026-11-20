use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const WORKER_SCHEMA_VERSION: &str = "worker.v2";
/// Exit code used when a worker cannot start because its typed configuration is invalid.
pub const WORKER_CONFIGURATION_EXIT_CODE: i32 = 78;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkerEvidence {
    pub dependencies_reachable: Option<bool>,
    pub subscription_acked: Option<bool>,
    pub last_event_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_output_count: Option<u64>,
    pub capability_verified: Option<bool>,
    pub last_job_validated: Option<bool>,
    pub outbox_last_consumed_at: Option<DateTime<Utc>>,
    pub sink_writable: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerResult {
    pub duration_ms: u64,
    pub output_count: Option<u64>,
    pub skipped: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerMessage {
    Hello {
        instance_id: String,
        at: DateTime<Utc>,
        pid: u32,
        executable_path: String,
        executable_version: String,
        schema_version: String,
    },
    Heartbeat {
        instance_id: String,
        at: DateTime<Utc>,
        evidence: WorkerEvidence,
    },
    Ready {
        instance_id: String,
        at: DateTime<Utc>,
        evidence: WorkerEvidence,
    },
    Cycle {
        instance_id: String,
        at: DateTime<Utc>,
        ok: bool,
        skipped: Option<String>,
        duration_ms: u64,
        output_count: Option<u64>,
        evidence: WorkerEvidence,
        error: Option<String>,
        summary: Option<String>,
    },
    OperationResult {
        instance_id: String,
        operation_id: String,
        ok: bool,
        result: WorkerResult,
        evidence: WorkerEvidence,
        error: Option<String>,
    },
    Stopping {
        instance_id: String,
        at: DateTime<Utc>,
        reason: String,
    },
    Stopped {
        instance_id: String,
        at: DateTime<Utc>,
        reason: String,
        drained: bool,
    },
    Fatal {
        instance_id: String,
        at: DateTime<Utc>,
        error: String,
    },
}

impl WorkerMessage {
    #[must_use]
    pub fn instance_id(&self) -> &str {
        match self {
            Self::Hello { instance_id, .. }
            | Self::Heartbeat { instance_id, .. }
            | Self::Ready { instance_id, .. }
            | Self::Cycle { instance_id, .. }
            | Self::OperationResult { instance_id, .. }
            | Self::Stopping { instance_id, .. }
            | Self::Stopped { instance_id, .. }
            | Self::Fatal { instance_id, .. } => instance_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SupervisorCommand {
    Shutdown { reason: String, grace_ms: u64 },
    RunOnce { operation_id: String },
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use super::*;

    #[test]
    fn worker_hello_uses_the_v2_schema() {
        assert_eq!(WORKER_SCHEMA_VERSION, "worker.v2");

        let message = WorkerMessage::Hello {
            instance_id: "sample-worker@local-agent:1".to_owned(),
            at: Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap(),
            pid: 42,
            executable_path: "C:\\tm-trading\\sample-worker.exe".to_owned(),
            executable_version: "0.1.0".to_owned(),
            schema_version: WORKER_SCHEMA_VERSION.to_owned(),
        };

        let value = serde_json::to_value(message).unwrap();

        assert_eq!(value["type"], "hello");
        assert_eq!(value["schema_version"], "worker.v2");
        assert_eq!(value["pid"], 42);
        assert_eq!(value["executable_version"], "0.1.0");
        assert!(value["executable_path"].as_str().is_some());
    }

    #[test]
    fn unknown_worker_evidence_is_discarded() {
        let value = json!({
            "type": "heartbeat",
            "instance_id": "sample-worker@local-agent:1",
            "at": "2026-11-17T00:00:00Z",
            "evidence": {
                "capability_verified": true,
                "process_alive": true,
                "observed": "READY"
            }
        });

        let message: WorkerMessage = serde_json::from_value(value).unwrap();
        let WorkerMessage::Heartbeat { evidence, .. } = message else {
            panic!("expected heartbeat message");
        };

        assert_eq!(evidence.capability_verified, Some(true));
        let serialized = serde_json::to_value(evidence).unwrap();
        assert!(serialized.get("process_alive").is_none());
        assert!(serialized.get("observed").is_none());
    }

    #[test]
    fn skipped_cycle_does_not_claim_zero_outputs() {
        let value = json!({
            "type": "cycle",
            "instance_id": "sample-worker@local-agent:1",
            "at": "2026-11-17T00:00:00Z",
            "ok": true,
            "skipped": "not scheduled",
            "duration_ms": 0,
            "output_count": null,
            "evidence": {},
            "error": null,
            "summary": null
        });

        let message: WorkerMessage = serde_json::from_value(value).unwrap();
        let WorkerMessage::Cycle {
            skipped,
            output_count,
            ..
        } = message
        else {
            panic!("expected cycle message");
        };

        assert_eq!(skipped.as_deref(), Some("not scheduled"));
        assert_eq!(output_count, None);
    }

    #[test]
    fn shutdown_command_uses_the_explicit_control_channel() {
        let command = SupervisorCommand::Shutdown {
            reason: "operation-1".to_owned(),
            grace_ms: 15_000,
        };

        let value = serde_json::to_value(command).unwrap();

        assert_eq!(value["type"], "shutdown");
        assert_eq!(value["reason"], "operation-1");
        assert_eq!(value["grace_ms"], 15_000);
    }
}
