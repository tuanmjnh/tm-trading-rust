use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::OperationActionDto;

/// Canonical desired state of a service instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DesiredStateDto {
    Running,
    Stopped,
}

/// Canonical observed state of a service instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ObservedStateDto {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    Unknown,
}

/// Canonical readiness of a service instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReadinessDto {
    Ready,
    Degraded,
    NotReady,
    Unknown,
}

/// Read model of one service instance as reported by its owning Agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ServiceSnapshot {
    pub service_instance_id: String,
    pub service_definition_id: String,
    pub agent_id: String,
    pub desired_state: DesiredStateDto,
    pub observed_state: ObservedStateDto,
    pub readiness: ReadinessDto,
    pub process_alive: Option<bool>,
    pub restart_count: u32,
    pub last_exit_code: Option<i32>,
    pub retry_blocked: bool,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

/// Agent heartbeat carrying the current service read model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AgentHeartbeatRequest {
    pub agent_id: String,
    pub agent_version: String,
    pub host_os: String,
    pub host_architecture: String,
    pub services: Vec<ServiceSnapshot>,
}

/// Response to an Agent heartbeat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AgentHeartbeatResponse {
    pub accepted: bool,
    pub server_time: DateTime<Utc>,
}

/// Request to claim the next command addressed to an Agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ClaimCommandRequest {
    pub agent_id: String,
}

/// A single leased command addressed to an Agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AgentCommandDto {
    pub operation_id: String,
    pub service_instance_id: String,
    pub service_definition_id: String,
    pub action: OperationActionDto,
    pub fencing_token: u64,
    pub lease_expires_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Response to a command claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ClaimCommandResponse {
    pub command: Option<AgentCommandDto>,
}

/// Agent report that a claimed command has started executing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ProgressCommandRequest {
    pub operation_id: String,
    pub agent_id: String,
    pub fencing_token: u64,
}

/// Response to a command progress report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ProgressCommandResponse {
    pub accepted: bool,
}

/// Agent report of the outcome of a claimed command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CompleteCommandRequest {
    pub operation_id: String,
    pub agent_id: String,
    pub fencing_token: u64,
    pub succeeded: bool,
    pub result_summary: Option<String>,
    pub error_code: Option<String>,
    pub redacted_error: Option<String>,
}

/// Response to a command completion report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CompleteCommandResponse {
    pub accepted: bool,
}
