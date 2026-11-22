mod ai_briefings;
mod ai_briefings_memory;
mod ai_jobs;
mod ai_jobs_memory;
mod ai_models;
mod ai_models_memory;
mod ai_providers_memory;
mod ai_task_routes;
mod ai_task_routes_memory;
mod bson_support;
mod candles_memory;
mod memory;
mod mongo;
mod mongo_ai_briefings;
mod mongo_ai_jobs;
mod mongo_ai_models;
mod mongo_ai_providers;
mod mongo_ai_task_routes;
mod mongo_candles;
mod mongo_news_clusters;
mod mongo_news_items;
mod mongo_news_sources;
mod mongo_operations;
mod mongo_providers;
mod mongo_research_events;
mod mongo_services;
mod mongo_watchlists;
mod news_clusters_memory;
mod news_items_memory;
mod news_sources_memory;
mod providers_memory;
mod repository;
mod research_events_memory;
mod watchlists_memory;

pub use ai_briefings::{AiBriefingQuery, AiBriefingRepository, DEFAULT_AI_BRIEFING_LIMIT};
pub use ai_briefings_memory::InMemoryAiBriefingRepository;
pub use ai_jobs::{AiJobQuery, AiJobRepository, CreateAiJobOutcome, DEFAULT_AI_JOB_LIMIT};
pub use ai_jobs_memory::InMemoryAiJobRepository;
pub use ai_models::AiModelRepository;
pub use ai_models_memory::InMemoryAiModelRepository;
pub use ai_providers_memory::InMemoryAiProviderProfileRepository;
pub use ai_task_routes::AiTaskRouteRepository;
pub use ai_task_routes_memory::InMemoryAiTaskRouteRepository;
pub use candles_memory::InMemoryCandleRepository;
pub use memory::{InMemoryOperationRepository, InMemoryServiceSnapshotRepository};
pub use mongo::MongoConnection;
pub use mongo_ai_briefings::MongoAiBriefingRepository;
pub use mongo_ai_jobs::MongoAiJobRepository;
pub use mongo_ai_models::MongoAiModelRepository;
pub use mongo_ai_providers::MongoAiProviderProfileRepository;
pub use mongo_ai_task_routes::MongoAiTaskRouteRepository;
pub use mongo_candles::MongoCandleRepository;
pub use mongo_news_clusters::MongoNewsClusterRepository;
pub use mongo_news_items::MongoNewsItemRepository;
pub use mongo_news_sources::MongoNewsSourceRepository;
pub use mongo_operations::MongoOperationRepository;
pub use mongo_providers::{MongoProviderInstanceRepository, PROVIDER_INSTANCE_SCHEMA_VERSION};
pub use mongo_research_events::MongoResearchEventRepository;
pub use mongo_services::{MongoServiceSnapshotRepository, SERVICE_SNAPSHOT_SCHEMA_VERSION};
pub use mongo_watchlists::MongoWatchlistRepository;
pub use news_clusters_memory::InMemoryNewsClusterRepository;
pub use news_items_memory::InMemoryNewsItemRepository;
pub use news_sources_memory::InMemoryNewsSourceRepository;
pub use providers_memory::InMemoryProviderInstanceRepository;
pub use repository::{
    AiProviderProfileRepository, CandleRepository, CandleUpsertOutcome, CreateOperationOutcome,
    DEFAULT_NEWS_CLUSTER_LIMIT, DEFAULT_NEWS_ITEM_LIMIT, NewsClusterQuery, NewsClusterRepository,
    NewsItemQuery, NewsItemRepository, NewsItemUpsertOutcome, NewsSourceRepository,
    OperationRepository, ProviderInstanceRepository, RepositoryError, ResearchEventQuery,
    ResearchEventRepository, ResearchEventUpsertOutcome, ServiceSnapshotRepository,
    WatchlistRepository,
};
pub use research_events_memory::InMemoryResearchEventRepository;
pub use watchlists_memory::InMemoryWatchlistRepository;
