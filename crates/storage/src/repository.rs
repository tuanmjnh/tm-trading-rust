use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;

use tm_trading_domain::{Operation, OperationError, OperationId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOperationOutcome {
    Created(Operation),
    Existing(Operation),
}

#[async_trait]
pub trait OperationRepository: Send + Sync {
    async fn create(&self, operation: Operation)
    -> Result<CreateOperationOutcome, RepositoryError>;

    async fn get(&self, operation_id: &OperationId) -> Result<Option<Operation>, RepositoryError>;

    async fn claim_next(
        &self,
        agent_id: &str,
        now: DateTime<Utc>,
        lease_duration: Duration,
    ) -> Result<Option<Operation>, RepositoryError>;

    async fn mark_running(
        &self,
        operation_id: &OperationId,
        agent_id: &str,
        fencing_token: u64,
        now: DateTime<Utc>,
    ) -> Result<Operation, RepositoryError>;

    async fn mark_succeeded(
        &self,
        operation_id: &OperationId,
        agent_id: &str,
        fencing_token: u64,
        result_summary: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<Operation, RepositoryError>;

    async fn mark_failed(
        &self,
        operation_id: &OperationId,
        agent_id: &str,
        fencing_token: u64,
        error_code: Option<String>,
        redacted_error: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<Operation, RepositoryError>;

    async fn expire_due(&self, now: DateTime<Utc>) -> Result<usize, RepositoryError>;
}

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("idempotency key was reused with a different request")]
    IdempotencyConflict {
        requested_by: String,
        idempotency_key: String,
    },
    #[error("operation was not found")]
    OperationNotFound,
    #[error("operation repository lock was poisoned")]
    LockPoisoned,
    #[error("operation lease duration must be positive")]
    InvalidLeaseDuration,
    #[error("operation lease duration is out of range")]
    LeaseDurationOutOfRange,
    #[error("MongoDB operation failed")]
    Mongo(#[from] mongodb::error::Error),
    #[error("invalid MongoDB database name")]
    InvalidDatabaseName,
    #[error("MongoDB connections must enable TLS")]
    TlsRequired,
    #[error("MongoDB connections must validate TLS certificates")]
    InsecureTls,
    #[error(transparent)]
    Domain(#[from] OperationError),
}
