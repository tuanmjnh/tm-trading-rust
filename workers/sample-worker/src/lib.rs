use std::path::Path;

use chrono::{DateTime, Utc};

use tm_trading_agent_protocol::{WORKER_SCHEMA_VERSION, WorkerEvidence, WorkerMessage};

#[derive(Debug, Clone)]
pub struct SampleWorker {
    instance_id: String,
    executable_path: String,
    evidence: WorkerEvidence,
}

impl SampleWorker {
    #[must_use]
    pub fn new(instance_id: &str) -> Self {
        let executable_path = std::env::current_exe()
            .ok()
            .and_then(|path| std::fs::canonicalize(path).ok())
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            instance_id: instance_id.to_owned(),
            executable_path,
            evidence: WorkerEvidence::default(),
        }
    }

    pub fn verify_capability(&mut self, at: DateTime<Utc>) -> bool {
        let WorkerMessage::Cycle {
            ok, output_count, ..
        } = self.execute_sample_cycle(at, 0)
        else {
            unreachable!("the sample capability must produce a cycle result");
        };
        let verified = !self.instance_id.trim().is_empty()
            && !self.executable_path.is_empty()
            && Path::new(&self.executable_path).is_file()
            && ok
            && output_count == Some(1);
        self.evidence.capability_verified = Some(verified);
        verified
    }

    #[must_use]
    pub fn hello(&self, pid: u32, at: DateTime<Utc>) -> WorkerMessage {
        WorkerMessage::Hello {
            instance_id: self.instance_id.clone(),
            at,
            pid,
            executable_path: self.executable_path.clone(),
            executable_version: env!("CARGO_PKG_VERSION").to_owned(),
            schema_version: WORKER_SCHEMA_VERSION.to_owned(),
        }
    }

    #[must_use]
    pub fn ready(&self, at: DateTime<Utc>) -> WorkerMessage {
        WorkerMessage::Ready {
            instance_id: self.instance_id.clone(),
            at,
            evidence: self.evidence.clone(),
        }
    }

    #[must_use]
    pub fn heartbeat(&self, at: DateTime<Utc>) -> WorkerMessage {
        WorkerMessage::Heartbeat {
            instance_id: self.instance_id.clone(),
            at,
            evidence: self.evidence.clone(),
        }
    }

    #[must_use]
    pub fn cycle(&mut self, at: DateTime<Utc>, duration_ms: u64) -> WorkerMessage {
        self.execute_sample_cycle(at, duration_ms)
    }

    fn execute_sample_cycle(&mut self, at: DateTime<Utc>, duration_ms: u64) -> WorkerMessage {
        self.evidence.last_success_at = Some(at);
        self.evidence.last_output_count = Some(1);
        WorkerMessage::Cycle {
            instance_id: self.instance_id.clone(),
            at,
            ok: true,
            skipped: None,
            duration_ms,
            output_count: Some(1),
            evidence: self.evidence.clone(),
            error: None,
            summary: Some("sample cycle completed".to_owned()),
        }
    }

    #[must_use]
    pub fn run_once(
        &mut self,
        operation_id: &str,
        at: DateTime<Utc>,
        duration_ms: u64,
    ) -> WorkerMessage {
        self.evidence.last_success_at = Some(at);
        self.evidence.last_output_count = Some(1);
        WorkerMessage::OperationResult {
            instance_id: self.instance_id.clone(),
            operation_id: operation_id.to_owned(),
            ok: true,
            result: tm_trading_agent_protocol::WorkerResult {
                duration_ms,
                output_count: Some(1),
                skipped: None,
                summary: Some("sample operation completed".to_owned()),
            },
            evidence: self.evidence.clone(),
            error: None,
        }
    }

    #[must_use]
    pub fn stopping(&self, at: DateTime<Utc>, reason: &str) -> WorkerMessage {
        WorkerMessage::Stopping {
            instance_id: self.instance_id.clone(),
            at,
            reason: reason.to_owned(),
        }
    }

    #[must_use]
    pub fn stopped(&self, at: DateTime<Utc>, reason: &str, drained: bool) -> WorkerMessage {
        WorkerMessage::Stopped {
            instance_id: self.instance_id.clone(),
            at,
            reason: reason.to_owned(),
            drained,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use tm_trading_agent_protocol::SupervisorCommand;

    use super::*;

    #[test]
    fn ready_reports_verified_sample_capability_only_after_the_self_check() {
        let mut worker = SampleWorker::new("sample-worker@local-agent:1");
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();

        let WorkerMessage::Ready { evidence, .. } = worker.ready(now) else {
            panic!("expected ready message");
        };
        assert_eq!(evidence.capability_verified, None);

        assert!(worker.verify_capability(now));

        let WorkerMessage::Ready { evidence, .. } = worker.ready(now) else {
            panic!("expected ready message");
        };
        assert_eq!(evidence.capability_verified, Some(true));
        assert_eq!(evidence.last_success_at, Some(now));
        assert_eq!(evidence.last_output_count, Some(1));
    }

    #[test]
    fn run_once_records_a_real_success() {
        let mut worker = SampleWorker::new("sample-worker@local-agent:1");
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();

        let WorkerMessage::OperationResult {
            ok,
            result,
            evidence,
            ..
        } = worker.run_once("operation-1", now, 5)
        else {
            panic!("expected operation result");
        };

        assert!(ok);
        assert_eq!(result.output_count, Some(1));
        assert_eq!(evidence.last_success_at, Some(now));
    }

    #[test]
    fn explicit_shutdown_reports_a_drained_worker() {
        let worker = SampleWorker::new("sample-worker@local-agent:1");
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();

        let WorkerMessage::Stopped { drained, .. } = worker.stopped(now, "test", true) else {
            panic!("expected stopped message");
        };

        assert!(drained);
    }

    #[test]
    fn shutdown_command_is_accepted_by_the_worker_protocol() {
        let command = SupervisorCommand::Shutdown {
            reason: "test".to_owned(),
            grace_ms: 1_000,
        };

        assert!(matches!(command, SupervisorCommand::Shutdown { .. }));
    }
}
