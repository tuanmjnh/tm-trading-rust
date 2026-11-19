use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;

use tm_trading_contracts::ServiceSnapshot;
use tm_trading_domain::{
    Candle, CandleError, Operation, OperationError, OperationId, ProviderInstance,
    ProviderInstanceError, Watchlist, WatchlistError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOperationOutcome {
    Created(Operation),
    Existing(Operation),
}

/// 24 hex characters, the shape of a MongoDB `ObjectId` in string form.
pub(crate) fn memory_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..24].to_owned()
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

/// Owner-scoped store for named symbol lists, matching the `watchlists` contract in
/// `docs/migration/data-contract-map.md` section 6.5.
///
/// Every method takes the owner explicitly: a list is never addressed by name or id
/// alone, so one owner can never read or write another owner's rows.
#[async_trait]
pub trait WatchlistRepository: Send + Sync {
    async fn list(&self, owner_id: &str) -> Result<Vec<Watchlist>, RepositoryError>;

    /// Creates a list. The first list an owner creates becomes that owner's default.
    async fn create(
        &self,
        owner_id: &str,
        name: &str,
        symbols: &[String],
        now: DateTime<Utc>,
    ) -> Result<Watchlist, RepositoryError>;

    async fn replace_symbols(
        &self,
        owner_id: &str,
        watchlist_id: &str,
        symbols: &[String],
        now: DateTime<Utc>,
    ) -> Result<Watchlist, RepositoryError>;

    async fn delete(&self, owner_id: &str, watchlist_id: &str) -> Result<(), RepositoryError>;
}

/// Configured provider instances, separate from the static adapter catalog.
///
/// Every method records configuration or observed health; the repository never calls a
/// provider itself, so a slow network cannot block a store write.
#[async_trait]
pub trait ProviderInstanceRepository: Send + Sync {
    async fn list(&self) -> Result<Vec<ProviderInstance>, RepositoryError>;

    async fn get(
        &self,
        provider_instance_id: &str,
    ) -> Result<Option<ProviderInstance>, RepositoryError>;

    async fn create(
        &self,
        provider_definition_id: &str,
        display_name: &str,
        base_url: &str,
        timeout_ms: u64,
        now: DateTime<Utc>,
    ) -> Result<ProviderInstance, RepositoryError>;

    async fn update(&self, instance: ProviderInstance)
    -> Result<ProviderInstance, RepositoryError>;

    /// Writes only the health fields, leaving the operator's configuration untouched.
    async fn record_health(
        &self,
        provider_instance_id: &str,
        observed_at: DateTime<Utc>,
        healthy: bool,
        latency_ms: Option<u64>,
        error_code: Option<&str>,
    ) -> Result<ProviderInstance, RepositoryError>;

    async fn delete(&self, provider_instance_id: &str) -> Result<(), RepositoryError>;
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
    Watchlist(#[from] WatchlistError),
    #[error("a watchlist with this name already exists for the owner")]
    DuplicateWatchlistName,
    #[error("watchlist was not found")]
    WatchlistNotFound,
    #[error("provider instance was not found")]
    ProviderInstanceNotFound,
    #[error(transparent)]
    ProviderInstance(#[from] ProviderInstanceError),
    #[error(transparent)]
    Domain(#[from] OperationError),
}
