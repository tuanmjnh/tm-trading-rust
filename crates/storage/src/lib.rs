mod bson_support;
mod candles_memory;
mod memory;
mod mongo;
mod mongo_candles;
mod mongo_operations;
mod mongo_providers;
mod mongo_services;
mod mongo_watchlists;
mod providers_memory;
mod repository;
mod watchlists_memory;

pub use candles_memory::InMemoryCandleRepository;
pub use memory::{InMemoryOperationRepository, InMemoryServiceSnapshotRepository};
pub use mongo::MongoConnection;
pub use mongo_candles::MongoCandleRepository;
pub use mongo_operations::MongoOperationRepository;
pub use mongo_providers::{MongoProviderInstanceRepository, PROVIDER_INSTANCE_SCHEMA_VERSION};
pub use mongo_services::{MongoServiceSnapshotRepository, SERVICE_SNAPSHOT_SCHEMA_VERSION};
pub use mongo_watchlists::MongoWatchlistRepository;
pub use providers_memory::InMemoryProviderInstanceRepository;
pub use repository::{
    CandleRepository, CandleUpsertOutcome, CreateOperationOutcome, OperationRepository,
    ProviderInstanceRepository, RepositoryError, ServiceSnapshotRepository, WatchlistRepository,
};
pub use watchlists_memory::InMemoryWatchlistRepository;
