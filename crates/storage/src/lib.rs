mod bson_support;
mod candles_memory;
mod memory;
mod mongo;
mod mongo_candles;
mod mongo_news_items;
mod mongo_news_sources;
mod mongo_operations;
mod mongo_providers;
mod mongo_research_events;
mod mongo_services;
mod mongo_watchlists;
mod news_items_memory;
mod news_sources_memory;
mod providers_memory;
mod repository;
mod research_events_memory;
mod watchlists_memory;

pub use candles_memory::InMemoryCandleRepository;
pub use memory::{InMemoryOperationRepository, InMemoryServiceSnapshotRepository};
pub use mongo::MongoConnection;
pub use mongo_candles::MongoCandleRepository;
pub use mongo_news_items::MongoNewsItemRepository;
pub use mongo_news_sources::MongoNewsSourceRepository;
pub use mongo_operations::MongoOperationRepository;
pub use mongo_providers::{MongoProviderInstanceRepository, PROVIDER_INSTANCE_SCHEMA_VERSION};
pub use mongo_research_events::MongoResearchEventRepository;
pub use mongo_services::{MongoServiceSnapshotRepository, SERVICE_SNAPSHOT_SCHEMA_VERSION};
pub use mongo_watchlists::MongoWatchlistRepository;
pub use news_items_memory::InMemoryNewsItemRepository;
pub use news_sources_memory::InMemoryNewsSourceRepository;
pub use providers_memory::InMemoryProviderInstanceRepository;
pub use repository::{
    CandleRepository, CandleUpsertOutcome, CreateOperationOutcome, DEFAULT_NEWS_ITEM_LIMIT,
    NewsItemQuery, NewsItemRepository, NewsItemUpsertOutcome, NewsSourceRepository,
    OperationRepository, ProviderInstanceRepository, RepositoryError, ResearchEventQuery,
    ResearchEventRepository, ResearchEventUpsertOutcome, ServiceSnapshotRepository,
    WatchlistRepository,
};
pub use research_events_memory::InMemoryResearchEventRepository;
pub use watchlists_memory::InMemoryWatchlistRepository;
