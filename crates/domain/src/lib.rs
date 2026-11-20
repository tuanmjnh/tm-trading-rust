mod candle;
mod lifecycle;
mod news;
mod operation;
mod provider;
mod research;
mod watchlist;

pub use candle::{CANDLE_SCHEMA_VERSION, Candle, CandleError, NewCandle, candle_id};
pub use lifecycle::{DesiredState, ObservedState, Readiness};
pub use news::{
    DEFAULT_NEWS_POLL_INTERVAL_SEC, DEFAULT_NEWS_RETENTION_DAYS, MAX_NEWS_RETENTION_DAYS,
    NEWS_SOURCE_SCHEMA_VERSION, NewNewsSource, NewsHealthState, NewsLicenseGate, NewsLicenseStatus,
    NewsRights, NewsSource, NewsSourceError, NewsSourceHealth, NewsTransport, NewsTrustTier,
    StoredNewsSource, sanitize_news_fetch_error,
};
pub use operation::{
    NewOperation, OPERATION_SCHEMA_VERSION, Operation, OperationAction, OperationError,
    OperationId, OperationStatus, PersistedOperation,
};
pub use provider::{
    MAX_PROVIDER_DISPLAY_NAME_CHARS, MAX_PROVIDER_TIMEOUT_MS, MIN_PROVIDER_TIMEOUT_MS,
    NewProviderInstance, ProviderHealthState, ProviderInstance, ProviderInstanceError,
    StoredProviderInstance, normalize_base_url, normalize_display_name,
};
pub use research::{
    NewResearchEvent, RESEARCH_EVENT_ID_PREFIX, RESEARCH_EVENT_SCHEMA_VERSION,
    RESEARCH_PRODUCER_VERSION, RESEARCH_SNAPSHOT_ID_PREFIX, ResearchEvent, ResearchEventError,
    ResearchQuality, event_id, input_snapshot_id,
};
pub use watchlist::{
    MAX_WATCHLIST_NAME_CHARS, MAX_WATCHLIST_SYMBOLS, Watchlist, WatchlistError, normalize_name,
    normalize_symbols,
};
