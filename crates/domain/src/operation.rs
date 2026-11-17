use chrono::{DateTime, Utc};
use thiserror::Error;
use uuid::Uuid;

pub const OPERATION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationId(Uuid);

impl OperationId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for OperationId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationAction {
    Start,
    Stop,
    Restart,
}

impl OperationAction {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "START",
            Self::Stop => "STOP",
            Self::Restart => "RESTART",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStatus {
    Accepted,
    Queued,
    Claimed,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Expired,
}

impl OperationStatus {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Expired
        )
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "ACCEPTED",
            Self::Queued => "QUEUED",
            Self::Claimed => "CLAIMED",
            Self::Running => "RUNNING",
            Self::Succeeded => "SUCCEEDED",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
            Self::Expired => "EXPIRED",
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewOperation {
    pub idempotency_key: String,
    pub request_hash: String,
    pub requested_by: String,
    pub requested_at: DateTime<Utc>,
    pub target_agent_id: String,
    pub service_instance_id: String,
    pub service_definition_id: String,
    pub action: OperationAction,
    pub expires_at: DateTime<Utc>,
    pub correlation_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    id: OperationId,
    schema_version: u32,
    idempotency_key: String,
    request_hash: String,
    requested_by: String,
    requested_at: DateTime<Utc>,
    target_agent_id: String,
    service_instance_id: String,
    service_definition_id: String,
    action: OperationAction,
    status: OperationStatus,
    attempt: u32,
    lease_owner: Option<String>,
    lease_expires_at: Option<DateTime<Utc>>,
    fencing_token: u64,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    expires_at: DateTime<Utc>,
    result_summary: Option<String>,
    redacted_error: Option<String>,
    error_code: Option<String>,
    correlation_id: Uuid,
}

impl Operation {
    pub fn new(input: NewOperation) -> Result<Self, OperationError> {
        for (name, value) in [
            ("idempotency_key", &input.idempotency_key),
            ("request_hash", &input.request_hash),
            ("requested_by", &input.requested_by),
            ("target_agent_id", &input.target_agent_id),
            ("service_instance_id", &input.service_instance_id),
            ("service_definition_id", &input.service_definition_id),
        ] {
            if value.trim().is_empty() {
                return Err(OperationError::EmptyField(name));
            }
        }

        if input.expires_at <= input.requested_at {
            return Err(OperationError::InvalidExpiry);
        }

        Ok(Self {
            id: OperationId::new(),
            schema_version: OPERATION_SCHEMA_VERSION,
            idempotency_key: input.idempotency_key,
            request_hash: input.request_hash,
            requested_by: input.requested_by,
            requested_at: input.requested_at,
            target_agent_id: input.target_agent_id,
            service_instance_id: input.service_instance_id,
            service_definition_id: input.service_definition_id,
            action: input.action,
            status: OperationStatus::Accepted,
            attempt: 0,
            lease_owner: None,
            lease_expires_at: None,
            fencing_token: 0,
            started_at: None,
            finished_at: None,
            expires_at: input.expires_at,
            result_summary: None,
            redacted_error: None,
            error_code: None,
            correlation_id: input.correlation_id,
        })
    }

    #[must_use]
    pub fn id(&self) -> OperationId {
        self.id
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    #[must_use]
    pub fn request_hash(&self) -> &str {
        &self.request_hash
    }

    #[must_use]
    pub fn requested_by(&self) -> &str {
        &self.requested_by
    }

    #[must_use]
    pub const fn requested_at(&self) -> DateTime<Utc> {
        self.requested_at
    }

    #[must_use]
    pub fn target_agent_id(&self) -> &str {
        &self.target_agent_id
    }

    #[must_use]
    pub fn service_instance_id(&self) -> &str {
        &self.service_instance_id
    }

    #[must_use]
    pub fn service_definition_id(&self) -> &str {
        &self.service_definition_id
    }

    #[must_use]
    pub const fn action(&self) -> OperationAction {
        self.action
    }

    #[must_use]
    pub const fn status(&self) -> OperationStatus {
        self.status
    }

    #[must_use]
    pub const fn attempt(&self) -> u32 {
        self.attempt
    }

    #[must_use]
    pub fn lease_owner(&self) -> Option<&str> {
        self.lease_owner.as_deref()
    }

    #[must_use]
    pub const fn lease_expires_at(&self) -> Option<DateTime<Utc>> {
        self.lease_expires_at
    }

    #[must_use]
    pub const fn fencing_token(&self) -> u64 {
        self.fencing_token
    }

    #[must_use]
    pub const fn started_at(&self) -> Option<DateTime<Utc>> {
        self.started_at
    }

    #[must_use]
    pub const fn finished_at(&self) -> Option<DateTime<Utc>> {
        self.finished_at
    }

    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    #[must_use]
    pub fn result_summary(&self) -> Option<&str> {
        self.result_summary.as_deref()
    }

    #[must_use]
    pub fn redacted_error(&self) -> Option<&str> {
        self.redacted_error.as_deref()
    }

    #[must_use]
    pub fn error_code(&self) -> Option<&str> {
        self.error_code.as_deref()
    }

    #[must_use]
    pub const fn correlation_id(&self) -> Uuid {
        self.correlation_id
    }

    #[must_use]
    pub fn is_claimable_at(&self, now: DateTime<Utc>) -> bool {
        !self.status.is_terminal()
            && self.expires_at > now
            && self
                .lease_expires_at
                .is_none_or(|lease_expiry| lease_expiry <= now)
    }

    pub fn claim(
        &mut self,
        agent_id: &str,
        now: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<(), OperationError> {
        if agent_id.trim().is_empty() {
            return Err(OperationError::EmptyField("agent_id"));
        }
        if agent_id != self.target_agent_id {
            return Err(OperationError::WrongTargetAgent);
        }
        if self.expires_at <= now {
            return Err(OperationError::Expired);
        }
        if !self.is_claimable_at(now) {
            return Err(OperationError::NotClaimable);
        }
        if lease_expires_at <= now {
            return Err(OperationError::InvalidLease);
        }

        self.attempt = self
            .attempt
            .checked_add(1)
            .ok_or(OperationError::AttemptOverflow)?;
        self.fencing_token = self
            .fencing_token
            .checked_add(1)
            .ok_or(OperationError::FencingTokenOverflow)?;
        self.status = OperationStatus::Claimed;
        self.lease_owner = Some(agent_id.to_owned());
        self.lease_expires_at = Some(lease_expires_at);
        Ok(())
    }

    pub fn mark_running(
        &mut self,
        agent_id: &str,
        fencing_token: u64,
        now: DateTime<Utc>,
    ) -> Result<(), OperationError> {
        self.transition_with_lease(
            OperationStatus::Claimed,
            OperationStatus::Running,
            agent_id,
            fencing_token,
            now,
        )?;
        self.started_at = Some(now);
        Ok(())
    }

    pub fn mark_succeeded(
        &mut self,
        agent_id: &str,
        fencing_token: u64,
        result_summary: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<(), OperationError> {
        self.transition_with_lease(
            OperationStatus::Running,
            OperationStatus::Succeeded,
            agent_id,
            fencing_token,
            now,
        )?;
        self.result_summary = result_summary;
        self.finished_at = Some(now);
        self.clear_lease();
        Ok(())
    }

    pub fn mark_failed(
        &mut self,
        agent_id: &str,
        fencing_token: u64,
        error_code: Option<String>,
        redacted_error: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<(), OperationError> {
        self.transition_with_lease(
            OperationStatus::Running,
            OperationStatus::Failed,
            agent_id,
            fencing_token,
            now,
        )?;
        self.error_code = error_code;
        self.redacted_error = redacted_error;
        self.finished_at = Some(now);
        self.clear_lease();
        Ok(())
    }

    pub fn mark_cancelled(&mut self, now: DateTime<Utc>) -> Result<(), OperationError> {
        if self.status.is_terminal() {
            return Err(OperationError::InvalidTransition {
                from: self.status,
                to: OperationStatus::Cancelled,
            });
        }
        self.status = OperationStatus::Cancelled;
        self.finished_at = Some(now);
        self.clear_lease();
        Ok(())
    }

    pub fn mark_expired(&mut self, now: DateTime<Utc>) -> Result<(), OperationError> {
        if self.status.is_terminal() || now < self.expires_at {
            return Err(OperationError::InvalidTransition {
                from: self.status,
                to: OperationStatus::Expired,
            });
        }
        self.status = OperationStatus::Expired;
        self.finished_at = Some(now);
        self.clear_lease();
        Ok(())
    }

    fn validate_lease(&self, agent_id: &str, fencing_token: u64) -> Result<(), OperationError> {
        if self.lease_owner.as_deref() != Some(agent_id) || self.fencing_token != fencing_token {
            return Err(OperationError::LeaseOwnershipMismatch);
        }
        Ok(())
    }

    fn transition_with_lease(
        &mut self,
        required: OperationStatus,
        next: OperationStatus,
        agent_id: &str,
        fencing_token: u64,
        now: DateTime<Utc>,
    ) -> Result<(), OperationError> {
        self.ensure_transition(required, next, now)?;
        self.validate_lease(agent_id, fencing_token)?;
        self.status = next;
        Ok(())
    }

    fn ensure_transition(
        &self,
        required: OperationStatus,
        next: OperationStatus,
        now: DateTime<Utc>,
    ) -> Result<(), OperationError> {
        if self.status != required {
            return Err(OperationError::InvalidTransition {
                from: self.status,
                to: next,
            });
        }
        if self.expires_at <= now {
            return Err(OperationError::Expired);
        }
        Ok(())
    }

    fn clear_lease(&mut self) {
        self.lease_owner = None;
        self.lease_expires_at = None;
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum OperationError {
    #[error("operation field '{0}' must not be empty")]
    EmptyField(&'static str),
    #[error("operation expiry must be later than its request time")]
    InvalidExpiry,
    #[error("operation has expired")]
    Expired,
    #[error("agent is not the target agent for this operation")]
    WrongTargetAgent,
    #[error("operation lease ownership or fencing token does not match")]
    LeaseOwnershipMismatch,
    #[error("operation is not claimable")]
    NotClaimable,
    #[error("operation lease expiry must be later than the claim time")]
    InvalidLease,
    #[error("operation attempt counter overflowed")]
    AttemptOverflow,
    #[error("operation fencing token overflowed")]
    FencingTokenOverflow,
    #[error("cannot transition operation from {from:?} to {to:?}")]
    InvalidTransition {
        from: OperationStatus,
        to: OperationStatus,
    },
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    use super::*;

    fn operation() -> Operation {
        let requested_at = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        Operation::new(NewOperation {
            idempotency_key: "start-sample-1".to_owned(),
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
        .unwrap()
    }

    #[test]
    fn rejects_empty_required_fields() {
        let requested_at = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let result = Operation::new(NewOperation {
            idempotency_key: String::new(),
            request_hash: "hash-1".to_owned(),
            requested_by: "user-1".to_owned(),
            requested_at,
            target_agent_id: "agent-1".to_owned(),
            service_instance_id: "sample-worker@agent-1".to_owned(),
            service_definition_id: "sample-worker".to_owned(),
            action: OperationAction::Start,
            expires_at: requested_at + Duration::minutes(5),
            correlation_id: Uuid::new_v4(),
        });

        assert_eq!(
            result.unwrap_err(),
            OperationError::EmptyField("idempotency_key")
        );
    }

    #[test]
    fn rejects_non_positive_operation_lifetime() {
        let requested_at = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let result = Operation::new(NewOperation {
            idempotency_key: "start-sample-1".to_owned(),
            request_hash: "hash-1".to_owned(),
            requested_by: "user-1".to_owned(),
            requested_at,
            target_agent_id: "agent-1".to_owned(),
            service_instance_id: "sample-worker@agent-1".to_owned(),
            service_definition_id: "sample-worker".to_owned(),
            action: OperationAction::Start,
            expires_at: requested_at,
            correlation_id: Uuid::new_v4(),
        });

        assert_eq!(result.unwrap_err(), OperationError::InvalidExpiry);
    }

    #[test]
    fn rejects_non_positive_claim_lease() {
        let requested_at = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut operation = operation();

        let result = operation.claim("agent-1", requested_at, requested_at);

        assert_eq!(result.unwrap_err(), OperationError::InvalidLease);
    }

    #[test]
    fn claim_uses_attempt_and_fencing_counters() {
        let requested_at = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut operation = operation();

        operation
            .claim(
                "agent-1",
                requested_at,
                requested_at + Duration::seconds(30),
            )
            .unwrap();

        assert_eq!(operation.status(), OperationStatus::Claimed);
        assert_eq!(operation.attempt(), 1);
        assert_eq!(operation.fencing_token(), 1);
        assert_eq!(operation.lease_owner(), Some("agent-1"));
    }

    #[test]
    fn active_lease_prevents_a_second_claim() {
        let requested_at = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut operation = operation();
        operation
            .claim(
                "agent-1",
                requested_at,
                requested_at + Duration::seconds(30),
            )
            .unwrap();

        let result = operation.claim(
            "agent-1",
            requested_at + Duration::seconds(10),
            requested_at + Duration::seconds(40),
        );

        assert_eq!(result.unwrap_err(), OperationError::NotClaimable);
    }

    #[test]
    fn expired_lease_can_be_reclaimed_with_a_new_fencing_token() {
        let requested_at = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut operation = operation();
        operation
            .claim(
                "agent-1",
                requested_at,
                requested_at + Duration::seconds(30),
            )
            .unwrap();

        operation
            .claim(
                "agent-1",
                requested_at + Duration::seconds(31),
                requested_at + Duration::seconds(61),
            )
            .unwrap();

        assert_eq!(operation.attempt(), 2);
        assert_eq!(operation.fencing_token(), 2);
        assert_eq!(operation.lease_owner(), Some("agent-1"));
    }

    #[test]
    fn wrong_agent_cannot_claim_an_operation() {
        let requested_at = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut operation = operation();

        let result = operation.claim(
            "agent-2",
            requested_at,
            requested_at + Duration::seconds(30),
        );

        assert_eq!(result.unwrap_err(), OperationError::WrongTargetAgent);
    }

    #[test]
    fn stale_fencing_token_cannot_complete_an_operation() {
        let requested_at = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut operation = operation();
        operation
            .claim(
                "agent-1",
                requested_at,
                requested_at + Duration::seconds(30),
            )
            .unwrap();
        operation.mark_running("agent-1", 1, requested_at).unwrap();
        operation
            .claim(
                "agent-1",
                requested_at + Duration::seconds(31),
                requested_at + Duration::seconds(61),
            )
            .unwrap();
        operation
            .mark_running("agent-1", 2, requested_at + Duration::seconds(31))
            .unwrap();

        let result = operation.mark_succeeded(
            "agent-1",
            1,
            Some("stale result".to_owned()),
            requested_at + Duration::seconds(32),
        );

        assert_eq!(result.unwrap_err(), OperationError::LeaseOwnershipMismatch);
    }

    #[test]
    fn terminal_operations_cannot_transition() {
        let requested_at = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        let mut operation = operation();
        operation
            .claim(
                "agent-1",
                requested_at,
                requested_at + Duration::seconds(30),
            )
            .unwrap();
        operation.mark_running("agent-1", 1, requested_at).unwrap();
        operation
            .mark_succeeded("agent-1", 1, Some("started".to_owned()), requested_at)
            .unwrap();

        let result = operation.mark_failed(
            "agent-1",
            1,
            Some("TEST_FAILURE".to_owned()),
            Some("test failure".to_owned()),
            requested_at,
        );

        assert!(matches!(
            result,
            Err(OperationError::InvalidTransition {
                from: OperationStatus::Succeeded,
                to: OperationStatus::Failed
            })
        ));
    }
}
