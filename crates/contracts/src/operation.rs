use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use tm_trading_domain::{Operation, OperationAction, OperationStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationActionDto {
    Start,
    Stop,
    Restart,
}

impl From<OperationAction> for OperationActionDto {
    fn from(value: OperationAction) -> Self {
        match value {
            OperationAction::Start => Self::Start,
            OperationAction::Stop => Self::Stop,
            OperationAction::Restart => Self::Restart,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatusDto {
    Accepted,
    Queued,
    Claimed,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Expired,
}

impl From<OperationStatus> for OperationStatusDto {
    fn from(value: OperationStatus) -> Self {
        match value {
            OperationStatus::Accepted => Self::Accepted,
            OperationStatus::Queued => Self::Queued,
            OperationStatus::Claimed => Self::Claimed,
            OperationStatus::Running => Self::Running,
            OperationStatus::Succeeded => Self::Succeeded,
            OperationStatus::Failed => Self::Failed,
            OperationStatus::Cancelled => Self::Cancelled,
            OperationStatus::Expired => Self::Expired,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateOperationRequest {
    pub service_instance_id: String,
    pub action: OperationActionDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct OperationResponse {
    pub operation_id: String,
    pub schema_version: u32,
    pub idempotency_key: String,
    pub correlation_id: String,
    pub service_instance_id: String,
    pub service_definition_id: String,
    pub target_agent_id: String,
    pub action: OperationActionDto,
    pub status: OperationStatusDto,
    pub attempt: u32,
    pub fencing_token: u64,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub requested_by: String,
    pub requested_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub result_summary: Option<String>,
    pub redacted_error: Option<String>,
    pub error_code: Option<String>,
}

impl From<&Operation> for OperationResponse {
    fn from(value: &Operation) -> Self {
        Self {
            operation_id: value.id().to_string(),
            schema_version: value.schema_version(),
            idempotency_key: value.idempotency_key().to_owned(),
            correlation_id: value.correlation_id().to_string(),
            service_instance_id: value.service_instance_id().to_owned(),
            service_definition_id: value.service_definition_id().to_owned(),
            target_agent_id: value.target_agent_id().to_owned(),
            action: value.action().into(),
            status: value.status().into(),
            attempt: value.attempt(),
            fencing_token: value.fencing_token(),
            lease_expires_at: value.lease_expires_at(),
            requested_by: value.requested_by().to_owned(),
            requested_at: value.requested_at(),
            started_at: value.started_at(),
            finished_at: value.finished_at(),
            expires_at: value.expires_at(),
            result_summary: value.result_summary().map(str::to_owned),
            redacted_error: value.redacted_error().map(str::to_owned),
            error_code: value.error_code().map(str::to_owned),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    use tm_trading_domain::{NewOperation, Operation, OperationAction};

    use super::*;

    #[test]
    fn response_contains_the_complete_durable_operation_shape() {
        let requested_at = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut operation = Operation::new(NewOperation {
            idempotency_key: "start-1".to_owned(),
            request_hash: "hash-1".to_owned(),
            requested_by: "user-1".to_owned(),
            requested_at,
            target_agent_id: "agent-1".to_owned(),
            service_instance_id: "sample-worker@agent-1".to_owned(),
            service_definition_id: "sample-worker".to_owned(),
            action: OperationAction::Start,
            expires_at: requested_at + Duration::minutes(5),
            correlation_id: Uuid::new_v4(),
        })
        .unwrap();
        operation
            .claim(
                "agent-1",
                requested_at,
                requested_at + Duration::seconds(30),
            )
            .unwrap();
        operation.mark_running("agent-1", 1, requested_at).unwrap();
        operation
            .mark_succeeded(
                "agent-1",
                1,
                Some("sample worker started".to_owned()),
                requested_at,
            )
            .unwrap();

        let response = OperationResponse::from(&operation);

        assert_eq!(response.schema_version, 1);
        assert_eq!(response.service_definition_id, "sample-worker");
        assert_eq!(
            response.result_summary.as_deref(),
            Some("sample worker started")
        );
        assert!(response.redacted_error.is_none());
        assert!(response.error_code.is_none());
    }
}
