use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;

use tm_trading_contracts::ServiceSnapshot;
use tm_trading_domain::{
    AiModelError, AiProviderProfile, AiProviderProfileError, Candle, CandleError, NewNewsSource,
    NewsCluster, NewsClusterError, NewsItem, NewsItemError, NewsLevel, NewsSource, NewsSourceError,
    Operation, OperationError, OperationId, ProviderInstance, ProviderInstanceError, ResearchEvent,
    ResearchEventError, Watchlist, WatchlistError,
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

/// How many of a batch were new rows versus re-recorded rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResearchEventUpsertOutcome {
    pub inserted: usize,
    pub existing: usize,
}

/// Filter of a research event read.
///
/// Every field is optional; an empty filter returns the newest events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchEventQuery {
    pub owner_scope: Option<String>,
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    pub event_type: Option<String>,
    /// Only events detected at or after this instant.
    pub since: Option<DateTime<Utc>>,
    /// Maximum number of events to return.
    pub limit: usize,
}

/// Default number of events a read returns.
pub const DEFAULT_RESEARCH_EVENT_LIMIT: usize = 50;

impl Default for ResearchEventQuery {
    fn default() -> Self {
        Self {
            owner_scope: None,
            symbol: None,
            timeframe: None,
            event_type: None,
            since: None,
            limit: DEFAULT_RESEARCH_EVENT_LIMIT,
        }
    }
}

impl ResearchEventQuery {
    /// Whether one event satisfies the filter.
    #[must_use]
    pub fn matches(&self, event: &ResearchEvent) -> bool {
        self.owner_scope
            .as_ref()
            .is_none_or(|owner| owner == event.owner_scope())
            && self
                .symbol
                .as_ref()
                .is_none_or(|symbol| symbol == event.symbol())
            && self
                .timeframe
                .as_ref()
                .is_none_or(|timeframe| timeframe == event.timeframe())
            && self
                .event_type
                .as_ref()
                .is_none_or(|event_type| event_type == event.event_type())
            && self.since.is_none_or(|since| event.detected_at() >= since)
    }
}

/// Durable store for research events.
///
/// Events are append-only observations: the same observation is written again
/// without creating a second row, so a worker can replay a window safely.
#[async_trait]
pub trait ResearchEventRepository: Send + Sync {
    async fn upsert(
        &self,
        events: &[ResearchEvent],
    ) -> Result<ResearchEventUpsertOutcome, RepositoryError>;

    /// Newest events first.
    async fn list(&self, query: &ResearchEventQuery)
    -> Result<Vec<ResearchEvent>, RepositoryError>;

    async fn get(&self, event_id: &str) -> Result<Option<ResearchEvent>, RepositoryError>;
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

/// Durable registry for policy-backed news sources.
///
/// Seeding writes policy fields only on insert, so an operator's edit survives a later
/// seed. Health and retry fields have their own writes and never overwrite policy.
#[async_trait]
pub trait NewsSourceRepository: Send + Sync {
    async fn upsert_seed(&self, source: NewNewsSource) -> Result<NewsSource, RepositoryError>;

    async fn list(&self) -> Result<Vec<NewsSource>, RepositoryError>;

    async fn get(&self, source_id: &str) -> Result<Option<NewsSource>, RepositoryError>;

    async fn set_enabled(
        &self,
        source_id: &str,
        enabled: bool,
        updated_at: DateTime<Utc>,
    ) -> Result<NewsSource, RepositoryError>;

    async fn record_health(
        &self,
        source_id: &str,
        observed_at: DateTime<Utc>,
        item_count: u32,
        error: Option<&str>,
    ) -> Result<NewsSource, RepositoryError>;

    async fn request_retry(
        &self,
        source_id: &str,
        requested_at: DateTime<Utc>,
    ) -> Result<NewsSource, RepositoryError>;
}

/// How many of a batch were inserted, materially updated, or unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NewsItemUpsertOutcome {
    pub inserted: usize,
    pub updated: usize,
    pub unchanged: usize,
}

/// Filter of a news item read.
///
/// Every field is optional except the bound; an empty filter returns the newest
/// items. Publication time is not used for ordering because it may be absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsItemQuery {
    pub source_id: Option<String>,
    pub market_group: Option<String>,
    pub level: Option<NewsLevel>,
    /// Only items ingested at or after this instant.
    pub since: Option<DateTime<Utc>>,
    /// Maximum number of items to return.
    pub limit: usize,
}

/// Default number of news items a read returns.
pub const DEFAULT_NEWS_ITEM_LIMIT: usize = 50;

impl Default for NewsItemQuery {
    fn default() -> Self {
        Self {
            source_id: None,
            market_group: None,
            level: None,
            since: None,
            limit: DEFAULT_NEWS_ITEM_LIMIT,
        }
    }
}

impl NewsItemQuery {
    /// Whether one item satisfies the filter.
    #[must_use]
    pub fn matches(&self, item: &NewsItem) -> bool {
        self.source_id
            .as_ref()
            .is_none_or(|source| source == item.source_id())
            && self
                .market_group
                .as_ref()
                .is_none_or(|group| group == item.market_group())
            && self.level.is_none_or(|level| level == item.level())
            && self.since.is_none_or(|since| item.ingested_at() >= since)
    }
}

/// Durable store for normalized, rights-safe news items.
///
/// The identifier is derived from the canonical article URL, so replaying the
/// same feed never duplicates a story. Re-observation freezes the original
/// observation and ingestion times, and only a material content change updates
/// the envelope.
#[async_trait]
pub trait NewsItemRepository: Send + Sync {
    async fn upsert_seen(
        &self,
        items: &[NewsItem],
    ) -> Result<NewsItemUpsertOutcome, RepositoryError>;

    /// Newest ingested items first.
    async fn list(&self, query: &NewsItemQuery) -> Result<Vec<NewsItem>, RepositoryError>;

    async fn get(&self, item_id: &str) -> Result<Option<NewsItem>, RepositoryError>;
}

/// Filter of a news cluster read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsClusterQuery {
    pub market_group: Option<String>,
    /// Only clusters whose last observation is at or after this instant.
    pub since: Option<DateTime<Utc>>,
    /// Maximum number of clusters to return.
    pub limit: usize,
}

/// Default number of news clusters a read returns.
pub const DEFAULT_NEWS_CLUSTER_LIMIT: usize = 50;

impl Default for NewsClusterQuery {
    fn default() -> Self {
        Self {
            market_group: None,
            since: None,
            limit: DEFAULT_NEWS_CLUSTER_LIMIT,
        }
    }
}

impl NewsClusterQuery {
    /// Whether one cluster satisfies the filter.
    #[must_use]
    pub fn matches(&self, cluster: &NewsCluster) -> bool {
        self.market_group
            .as_ref()
            .is_none_or(|group| group == cluster.market_group())
            && self
                .since
                .is_none_or(|since| cluster.last_seen_at() >= since)
    }
}

/// Durable store for news event clusters.
#[async_trait]
pub trait NewsClusterRepository: Send + Sync {
    async fn upsert(&self, cluster: &NewsCluster) -> Result<(), RepositoryError>;

    /// Newest clusters first, ordered by last observation.
    async fn list(&self, query: &NewsClusterQuery) -> Result<Vec<NewsCluster>, RepositoryError>;

    async fn get(&self, cluster_id: &str) -> Result<Option<NewsCluster>, RepositoryError>;
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

#[async_trait]
pub trait AiProviderProfileRepository: Send + Sync {
    async fn list(&self) -> Result<Vec<AiProviderProfile>, RepositoryError>;
    async fn get(&self, id: &str) -> Result<Option<AiProviderProfile>, RepositoryError>;
    async fn upsert(&self, profile: &AiProviderProfile) -> Result<(), RepositoryError>;
    async fn delete(&self, id: &str) -> Result<(), RepositoryError>;
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
    #[error(transparent)]
    ResearchEvent(#[from] ResearchEventError),
    #[error("a watchlist with this name already exists for the owner")]
    DuplicateWatchlistName,
    #[error("watchlist was not found")]
    WatchlistNotFound,
    #[error("provider instance was not found")]
    ProviderInstanceNotFound,
    #[error("ai provider profile was not found")]
    AiProviderProfileNotFound,
    #[error("ai model was not found")]
    AiModelNotFound,
    #[error("news source was not found")]
    NewsSourceNotFound,
    #[error(transparent)]
    NewsSource(#[from] NewsSourceError),
    #[error(transparent)]
    NewsItem(#[from] NewsItemError),
    #[error(transparent)]
    NewsCluster(#[from] NewsClusterError),
    #[error(transparent)]
    ProviderInstance(#[from] ProviderInstanceError),
    #[error(transparent)]
    AiProviderProfile(#[from] AiProviderProfileError),
    #[error(transparent)]
    AiModel(#[from] AiModelError),
    #[error(transparent)]
    Domain(#[from] OperationError),
}
