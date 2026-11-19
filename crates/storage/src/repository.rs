use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;

use tm_trading_contracts::ServiceSnapshot;
use tm_trading_domain::{Candle, CandleError, Operation, OperationError, OperationId};

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

/// Persisted service read model described by `docs/migration/data-contract-map.md` section 8.2.
///
/// The Agent is authoritative for its own instances: a heartbeat replaces every row it owns,
/// so an instance the Agent no longer reports cannot linger in the projection.
#[async_trait]
pub trait ServiceSnapshotRepository: Send + Sync {
    async fn replace_agent_snapshots(
        &self,
        agent_id: &str,
        snapshots: Vec<ServiceSnapshot>,
    ) -> Result<(), RepositoryError>;

    async fn list(&self) -> Result<Vec<ServiceSnapshot>, RepositoryError>;

    async fn get(
        &self,
        service_instance_id: &str,
    ) -> Result<Option<ServiceSnapshot>, RepositoryError>;
}

/// How many of a batch were new rows versus re-recorded rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CandleUpsertOutcome {
    pub inserted: usize,
    pub existing: usize,
}

/// Durable store for closed candles, matching the `candles` contract in
/// `docs/migration/data-contract-map.md` section 6.1.
#[async_trait]
pub trait CandleRepository: Send + Sync {
    async fn upsert_closed(
        &self,
        candles: &[Candle],
    ) -> Result<CandleUpsertOutcome, RepositoryError>;

    /// Most recent closed bars for a symbol and timeframe, newest first.
    async fn recent(
        &self,
        symbol: &str,
        timeframe: &str,
        limit: usize,
    ) -> Result<Vec<Candle>, RepositoryError>;

    async fn get(&self, candle_id: &str) -> Result<Option<Candle>, RepositoryError>;
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
    #[error("operation was modified concurrently")]
    ConcurrentModification,
    #[error("persisted record is corrupt: {0}")]
    CorruptRecord(&'static str),
    #[error("service snapshot belongs to a different agent")]
    SnapshotAgentMismatch,
    #[error("service snapshot field '{0}' must not be empty")]
    EmptySnapshotField(&'static str),
    #[error("MongoDB operation failed")]
    Mongo(#[from] mongodb::error::Error),
    #[error("invalid MongoDB database name")]
    InvalidDatabaseName,
    #[error("MongoDB connections must enable TLS")]
    TlsRequired,
    #[error("MongoDB connections must validate TLS certificates")]
    InsecureTls,
    #[error(transparent)]
    Candle(#[from] CandleError),
    #[error(transparent)]
    Domain(#[from] OperationError),
}
