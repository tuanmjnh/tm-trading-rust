use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{API_VERSION, CONTRACT_VERSION};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ApiVersionInfo {
    pub api_version: String,
    pub contract_version: String,
    pub service_version: String,
}

impl ApiVersionInfo {
    #[must_use]
    pub fn current() -> Self {
        Self {
            api_version: API_VERSION.to_owned(),
            contract_version: CONTRACT_VERSION.to_owned(),
            service_version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    pub status: HealthStatus,
    pub version: ApiVersionInfo,
}

/// High-level diagnostic snapshot of backend storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StorageDiagnosticsDto {
    pub storage_kind: String,
    pub connected: bool,
    pub database_name: Option<String>,
}

/// Diagnostic health status of a supervised worker process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WorkerDiagnosticsDto {
    pub service_instance_id: String,
    pub observed_state: String,
    pub readiness: String,
    pub process_alive: bool,
    pub restart_count: u32,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
}

/// Diagnostic summary of the notification subsystem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NotificationDiagnosticsDto {
    pub telegram_configured: bool,
    pub outbox_queued: usize,
    pub outbox_dead: usize,
}

/// Comprehensive system diagnostic report (secret-redacted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SystemDiagnosticsDto {
    pub generated_at: DateTime<Utc>,
    pub version: ApiVersionInfo,
    pub os: String,
    pub arch: String,
    pub uptime_seconds: u64,
    pub storage: StorageDiagnosticsDto,
    pub workers: Vec<WorkerDiagnosticsDto>,
    pub notifications: NotificationDiagnosticsDto,
}

/// Redacted diagnostic bundle for troubleshooting and support.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SupportBundleDto {
    pub bundle_id: String,
    pub generated_at: DateTime<Utc>,
    pub diagnostics: SystemDiagnosticsDto,
    pub snapshot_count: usize,
    pub redacted_summary: String,
}
