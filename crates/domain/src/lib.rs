mod ai_briefing;
mod ai_model;
mod ai_provider;
mod ai_task;
mod candle;
mod lifecycle;
mod news;
mod news_cluster;
mod news_item;
mod operation;
mod provider;
mod research;
mod watchlist;

pub use ai_briefing::{
    AI_BRIEFING_ID_PREFIX, AI_BRIEFING_PRODUCER_VERSION, AI_BRIEFING_SCHEMA_VERSION, AiBriefing,
    AiBriefingError, AiBriefingReference, AiBriefingReferenceKind, MAX_AI_BRIEFING_CONTENT_CHARS,
    MAX_AI_BRIEFING_LIST_ITEMS, MAX_AI_BRIEFING_TEXT_CHARS, NewAiBriefing, is_valid_ai_briefing_id,
};
pub use ai_model::{
    AiModel, AiModelError, MAX_AI_MODEL_CONTEXT_WINDOW, MAX_AI_MODEL_DISPLAY_NAME_CHARS,
    MAX_AI_MODEL_OUTPUT_TOKENS, NewAiModel, StoredAiModel,
};
pub use ai_provider::{
    AI_PROVIDER_API_KEY_ENV_PREFIX, AiProviderHealthState, AiProviderKind, AiProviderProfile,
    AiProviderProfileError, DEFAULT_AI_PROVIDER_API_KEY_ENV, MAX_AI_PROVIDER_DISPLAY_NAME_CHARS,
    MAX_AI_PROVIDER_TIMEOUT_MS, MIN_AI_PROVIDER_TIMEOUT_MS, NewAiProviderProfile,
    StoredAiProviderProfile,
};
pub use ai_task::{
    AiTaskKind, AiTaskModelRef, AiTaskRoute, AiTaskRouteError, MAX_AI_TASK_CONCURRENCY,
    MAX_AI_TASK_MAX_ATTEMPTS, MAX_AI_TASK_OUTPUT_TOKENS, MIN_AI_TASK_CONCURRENCY,
    MIN_AI_TASK_MAX_ATTEMPTS, MIN_AI_TASK_OUTPUT_TOKENS, NewAiTaskRoute, StoredAiTaskRoute,
};
pub use candle::{CANDLE_SCHEMA_VERSION, Candle, CandleError, NewCandle, candle_id};
pub use lifecycle::{DesiredState, ObservedState, Readiness};
pub use news::{
    DEFAULT_NEWS_POLL_INTERVAL_SEC, DEFAULT_NEWS_RETENTION_DAYS, MAX_NEWS_RETENTION_DAYS,
    NEWS_SOURCE_SCHEMA_VERSION, NewNewsSource, NewsHealthState, NewsLicenseGate, NewsLicenseStatus,
    NewsRights, NewsSource, NewsSourceError, NewsSourceHealth, NewsTransport, NewsTrustTier,
    StoredNewsSource, is_safe_news_feed_url, sanitize_news_fetch_error,
};
pub use news_cluster::{
    DEFAULT_NEWS_CLUSTER_MARKET_GROUP, MAX_NEWS_CLUSTER_ITEMS, NEWS_CLUSTER_ID_PREFIX,
    NEWS_CLUSTER_JACCARD_THRESHOLD, NEWS_CLUSTER_SCHEMA_VERSION, NewNewsCluster, NewsCluster,
    NewsClusterError, StoredNewsCluster, news_cluster_id_for, news_cluster_jaccard,
    significant_tokens,
};
pub use news_item::{
    DEFAULT_NEWS_ITEM_LANGUAGE, DEFAULT_NEWS_ITEM_MARKET_GROUP, NEWS_EXCERPT_MAX_CHARS,
    NEWS_ITEM_ID_PREFIX, NEWS_ITEM_SCHEMA_VERSION, NewNewsItem, NewsDirection, NewsItem,
    NewsItemError, NewsItemRights, NewsLevel, StoredNewsItem, canonicalize_news_url,
    market_group_for, news_content_hash_for, news_item_id_for,
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
