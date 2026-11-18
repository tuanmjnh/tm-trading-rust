use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use tm_trading_domain::{DesiredState, ObservedState, Readiness};
use tm_trading_supervisor_core::{ServiceState, WorkerProcessIdentity, WorkerRegistry};

pub(super) const STATE_FILE_NAME: &str = "agent-state.json";
const STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PersistedAgentState {
    schema_version: u32,
    agent_id: String,
    services: Vec<PersistedServiceState>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedServiceState {
    service_definition_id: String,
    service_instance_id: String,
    desired_state: PersistedDesiredState,
    observed_state: StoredObservedState,
    readiness: PersistedReadiness,
    process_identity: Option<PersistedProcessIdentity>,
    last_exit_code: Option<i32>,
    restart_count: u32,
    next_restart_at: Option<DateTime<Utc>>,
    #[serde(default)]
    retry_blocked: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum PersistedDesiredState {
    Running,
    Stopped,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum StoredObservedState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    Unknown,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum PersistedReadiness {
    Ready,
    Degraded,
    NotReady,
    Unknown,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedProcessIdentity {
    pid: u32,
    executable_path: String,
    executable_version: String,
}

pub(super) fn load(
    path: &Path,
    agent_id: &str,
    registry: &WorkerRegistry,
    now: DateTime<Utc>,
) -> Result<Vec<ServiceState>, String> {
    let Some(persisted) = load_persisted_state(path)? else {
        return Ok(Vec::new());
    };
    if persisted.schema_version != STATE_SCHEMA_VERSION {
        return Err(format!(
            "unsupported Agent state schema {} in {}",
            persisted.schema_version,
            path.display()
        ));
    }
    if persisted.agent_id != agent_id {
        return Err(format!(
            "Agent state belongs to '{}', not '{}'",
            persisted.agent_id, agent_id
        ));
    }

    Ok(persisted
        .services
        .into_iter()
        .filter_map(|service| {
            let definition = registry.get(&service.service_definition_id)?;
            Some(service.into_service_state(now, definition.max_restart_attempts))
        })
        .collect())
}

fn load_persisted_state(path: &Path) -> Result<Option<PersistedAgentState>, String> {
    match read_state_file(path) {
        Ok(Some(state)) => Ok(Some(state)),
        Ok(None) => load_and_restore_backup(path),
        Err(primary_error) => match load_and_restore_backup(path) {
            Ok(Some(state)) => Ok(Some(state)),
            Ok(None) => Err(primary_error),
            Err(backup_error) => Err(format!("{primary_error}; {backup_error}")),
        },
    }
}

fn read_state_file(path: &Path) -> Result<Option<PersistedAgentState>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn load_and_restore_backup(path: &Path) -> Result<Option<PersistedAgentState>, String> {
    let backup_path = backup_path(path);
    let Some(state) = read_state_file(&backup_path)? else {
        return Ok(None);
    };
    fs::copy(&backup_path, path).map_err(|error| {
        format!(
            "failed to restore Agent state backup {}: {error}",
            backup_path.display()
        )
    })?;
    Ok(Some(state))
}

pub(super) fn save(path: &Path, agent_id: &str, states: &[ServiceState]) -> io::Result<()> {
    let persisted = PersistedAgentState {
        schema_version: STATE_SCHEMA_VERSION,
        agent_id: agent_id.to_owned(),
        services: states.iter().map(PersistedServiceState::from).collect(),
    };
    let bytes = serde_json::to_vec_pretty(&persisted)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    write_safely(path, &bytes)
}

impl From<&ServiceState> for PersistedServiceState {
    fn from(state: &ServiceState) -> Self {
        Self {
            service_definition_id: state.service_definition_id.clone(),
            service_instance_id: state.service_instance_id.clone(),
            desired_state: match state.desired_state {
                DesiredState::Running => PersistedDesiredState::Running,
                DesiredState::Stopped => PersistedDesiredState::Stopped,
            },
            observed_state: match state.observed_state {
                ObservedState::Starting => StoredObservedState::Starting,
                ObservedState::Running => StoredObservedState::Running,
                ObservedState::Stopping => StoredObservedState::Stopping,
                ObservedState::Stopped => StoredObservedState::Stopped,
                ObservedState::Failed => StoredObservedState::Failed,
                ObservedState::Unknown => StoredObservedState::Unknown,
            },
            readiness: match state.readiness {
                Readiness::Ready => PersistedReadiness::Ready,
                Readiness::Degraded => PersistedReadiness::Degraded,
                Readiness::NotReady => PersistedReadiness::NotReady,
                Readiness::Unknown => PersistedReadiness::Unknown,
            },
            process_identity: state.process_identity.as_ref().map(|identity| {
                PersistedProcessIdentity {
                    pid: identity.pid,
                    executable_path: identity.executable_path.clone(),
                    executable_version: identity.executable_version.clone(),
                }
            }),
            last_exit_code: state.last_exit_code,
            restart_count: state.restart_count,
            next_restart_at: state.next_restart_at,
            retry_blocked: state.retry_blocked,
        }
    }
}

impl PersistedServiceState {
    fn into_service_state(self, now: DateTime<Utc>, max_restart_attempts: u32) -> ServiceState {
        let mut state = ServiceState::new(&self.service_definition_id, &self.service_instance_id);
        state.desired_state = match self.desired_state {
            PersistedDesiredState::Running => DesiredState::Running,
            PersistedDesiredState::Stopped => DesiredState::Stopped,
        };
        state.observed_state = match self.observed_state {
            StoredObservedState::Starting => ObservedState::Starting,
            StoredObservedState::Running => ObservedState::Running,
            StoredObservedState::Stopping => ObservedState::Stopping,
            StoredObservedState::Stopped => ObservedState::Stopped,
            StoredObservedState::Failed => ObservedState::Failed,
            StoredObservedState::Unknown => ObservedState::Unknown,
        };
        state.readiness = match self.readiness {
            PersistedReadiness::Ready => Readiness::Ready,
            PersistedReadiness::Degraded => Readiness::Degraded,
            PersistedReadiness::NotReady => Readiness::NotReady,
            PersistedReadiness::Unknown => Readiness::Unknown,
        };
        state.process_identity = self.process_identity.map(|identity| WorkerProcessIdentity {
            pid: identity.pid,
            executable_path: identity.executable_path,
            executable_version: identity.executable_version,
        });
        state.last_exit_code = self.last_exit_code;
        state.restart_count = self.restart_count;
        state.next_restart_at = self.next_restart_at;
        state.retry_blocked = self.retry_blocked;

        // A persisted process identity is not proof that the prior worker is still live.
        // Reconcile every persisted running service as failed and schedule a fresh instance.
        state.process_alive = Some(false);
        state.last_heartbeat_at = None;
        state.readiness = Readiness::NotReady;
        if state.desired_state == DesiredState::Running {
            state.observed_state = ObservedState::Failed;
            if state.retry_blocked || state.restart_count > max_restart_attempts {
                state.next_restart_at = None;
            } else if state.next_restart_at.is_none() {
                state.next_restart_at = Some(now);
            }
        } else {
            state.observed_state = ObservedState::Stopped;
            state.next_restart_at = None;
        }
        state
    }
}

fn write_safely(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary_path = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("agent-state"),
        Uuid::new_v4()
    ));
    let backup_path = backup_path(path);

    let mut temporary_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)?;
    if let Err(error) = temporary_file
        .write_all(bytes)
        .and_then(|()| temporary_file.sync_all())
    {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }
    drop(temporary_file);

    if path.exists() {
        let _ = fs::remove_file(&backup_path);
        fs::rename(path, &backup_path)?;
    }
    if let Err(error) = fs::rename(&temporary_path, path) {
        if backup_path.exists() {
            let _ = fs::rename(&backup_path, path);
        }
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }
    Ok(())
}

fn backup_path(path: &Path) -> std::path::PathBuf {
    path.with_extension("json.bak")
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use tm_trading_supervisor_core::sample_worker_definition;

    #[test]
    fn a_schema_v1_checkpoint_without_retry_block_remains_readable() {
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp/agent-tests")
            .join(format!("tm-trading-agent-state-{}.json", Uuid::new_v4()));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            br#"{
                "schemaVersion": 1,
                "agentId": "local-agent",
                "services": [{
                    "serviceDefinitionId": "sample-worker",
                    "serviceInstanceId": "sample-worker@local-agent:1",
                    "desiredState": "RUNNING",
                    "observedState": "RUNNING",
                    "readiness": "READY",
                    "processIdentity": null,
                    "lastExitCode": null,
                    "restartCount": 0,
                    "nextRestartAt": null
                }]
            }"#,
        )
        .unwrap();

        let loaded = load(&path, "local-agent", &WorkerRegistry::sample(), now).unwrap();

        assert_eq!(loaded.len(), 1);
        assert!(!loaded[0].retry_blocked);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persisted_running_service_is_reconciled_for_a_fresh_instance() {
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut state = ServiceState::new("sample-worker", "sample-worker@local-agent:1");
        state.request_start();
        state.process_alive = Some(true);
        state.process_identity = Some(WorkerProcessIdentity {
            pid: 42,
            executable_path: "sample-worker.exe".to_owned(),
            executable_version: "0.1.0".to_owned(),
        });
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp/agent-tests")
            .join(format!("tm-trading-agent-state-{}.json", Uuid::new_v4()));

        save(&path, "local-agent", &[state]).unwrap();
        let loaded = load(&path, "local-agent", &WorkerRegistry::sample(), now).unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].desired_state, DesiredState::Running);
        assert_eq!(loaded[0].observed_state, ObservedState::Failed);
        assert_eq!(loaded[0].process_alive, Some(false));
        assert_eq!(loaded[0].next_restart_at, Some(now));
        assert_eq!(
            loaded[0].process_identity.as_ref().map(|item| item.pid),
            Some(42)
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn missing_primary_state_recovers_the_last_known_good_backup() {
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut service_state = ServiceState::new("sample-worker", "sample-worker@local-agent:1");
        service_state.request_start();
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp/agent-tests")
            .join(format!("tm-trading-agent-state-{}.json", Uuid::new_v4()));
        let backup_path = path.with_extension("json.bak");

        save(&path, "local-agent", &[service_state]).unwrap();
        fs::rename(&path, &backup_path).unwrap();

        let loaded = load(&path, "local-agent", &WorkerRegistry::sample(), now).unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].desired_state, DesiredState::Running);
        assert!(backup_path.exists());
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(backup_path);
    }

    #[test]
    fn corrupt_primary_state_recovers_the_last_known_good_backup() {
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut service_state = ServiceState::new("sample-worker", "sample-worker@local-agent:1");
        service_state.request_start();
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp/agent-tests")
            .join(format!("tm-trading-agent-state-{}.json", Uuid::new_v4()));
        let backup_path = path.with_extension("json.bak");

        save(&path, "local-agent", &[service_state]).unwrap();
        fs::copy(&path, &backup_path).unwrap();
        fs::write(&path, b"not-json").unwrap();

        let loaded = load(&path, "local-agent", &WorkerRegistry::sample(), now).unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].desired_state, DesiredState::Running);
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(backup_path);
    }

    #[test]
    fn persisted_running_state_preserves_its_scheduled_restart() {
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let restart_at = now + chrono::Duration::seconds(30);
        let mut service_state = ServiceState::new("sample-worker", "sample-worker@local-agent:1");
        service_state.request_start();
        service_state.next_restart_at = Some(restart_at);
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp/agent-tests")
            .join(format!("tm-trading-agent-state-{}.json", Uuid::new_v4()));

        save(&path, "local-agent", &[service_state]).unwrap();
        let loaded = load(&path, "local-agent", &WorkerRegistry::sample(), now).unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].desired_state, DesiredState::Running);
        assert_eq!(loaded[0].next_restart_at, Some(restart_at));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persisted_quarantine_blocks_automatic_retry() {
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut service_state = ServiceState::new("sample-worker", "sample-worker@local-agent:1");
        service_state.request_start();
        service_state.quarantine(Some(2));
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp/agent-tests")
            .join(format!("tm-trading-agent-state-{}.json", Uuid::new_v4()));

        save(&path, "local-agent", &[service_state]).unwrap();
        let loaded = load(&path, "local-agent", &WorkerRegistry::sample(), now).unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].desired_state, DesiredState::Running);
        assert_eq!(loaded[0].observed_state, ObservedState::Failed);
        assert!(loaded[0].retry_blocked);
        assert!(loaded[0].next_restart_at.is_none());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persisted_running_state_does_not_grant_a_replacement_after_the_restart_budget() {
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let definition = sample_worker_definition();
        let mut service_state = ServiceState::new(definition.id, "sample-worker@local-agent:1");
        service_state.request_start();
        service_state.restart_count = definition.max_restart_attempts + 1;
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp/agent-tests")
            .join(format!("tm-trading-agent-state-{}.json", Uuid::new_v4()));

        save(&path, "local-agent", &[service_state]).unwrap();
        let loaded = load(&path, "local-agent", &WorkerRegistry::sample(), now).unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].desired_state, DesiredState::Running);
        assert_eq!(loaded[0].observed_state, ObservedState::Failed);
        assert!(loaded[0].next_restart_at.is_none());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn state_file_cannot_be_reused_for_a_different_agent_id() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp/agent-tests")
            .join(format!("tm-trading-agent-state-{}.json", Uuid::new_v4()));
        let state = ServiceState::new(sample_worker_definition().id, "sample-worker@local-agent:1");
        save(&path, "local-agent", &[state]).unwrap();

        let error = load(&path, "other-agent", &WorkerRegistry::sample(), Utc::now()).unwrap_err();

        assert!(error.contains("belongs to 'local-agent'"));
        let _ = fs::remove_file(path);
    }
}
