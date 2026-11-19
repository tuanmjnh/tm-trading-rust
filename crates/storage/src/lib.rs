mod bson_support;
mod candles_memory;
mod memory;
mod mongo;
mod mongo_candles;
mod mongo_operations;
mod mongo_services;
mod repository;

pub use candles_memory::InMemoryCandleRepository;
pub use memory::{InMemoryOperationRepository, InMemoryServiceSnapshotRepository};
pub use mongo::MongoConnection;
pub use mongo_candles::MongoCandleRepository;
pub use mongo_operations::MongoOperationRepository;
pub use mongo_services::{MongoServiceSnapshotRepository, SERVICE_SNAPSHOT_SCHEMA_VERSION};
pub use repository::{
    CandleRepository, CandleUpsertOutcome, CreateOperationOutcome, OperationRepository,
    RepositoryError, ServiceSnapshotRepository,
};
