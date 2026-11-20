use std::sync::Arc;

use tm_trading_contracts::ServiceSnapshot;
use tm_trading_storage::{
    CandleRepository, InMemoryCandleRepository, InMemoryOperationRepository,
    InMemoryProviderInstanceRepository, InMemoryResearchEventRepository,
    InMemoryServiceSnapshotRepository, InMemoryWatchlistRepository, OperationRepository,
    ProviderInstanceRepository, RepositoryError, ResearchEventRepository,
    ServiceSnapshotRepository, WatchlistRepository,
};

/// Credentials and capabilities used in local deployment mode.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub user_token: String,
    pub agent_token: String,
    pub user_subject: String,
    pub agent_id: String,
}

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub operations: Arc<dyn OperationRepository>,
    pub services: Arc<dyn ServiceSnapshotRepository>,
    pub watchlists: Arc<dyn WatchlistRepository>,
    pub providers: Arc<dyn ProviderInstanceRepository>,
    pub candles: Arc<dyn CandleRepository>,
    pub research_events: Arc<dyn ResearchEventRepository>,
    pub auth: AuthConfig,
}

impl AppState {
    #[must_use]
    pub fn new(auth: AuthConfig) -> Self {
        Self::with_repositories(
            auth,
            Arc::new(InMemoryOperationRepository::new()),
            Arc::new(InMemoryServiceSnapshotRepository::new()),
            Arc::new(InMemoryWatchlistRepository::new()),
            Arc::new(InMemoryProviderInstanceRepository::new()),
            Arc::new(InMemoryCandleRepository::new()),
            Arc::new(InMemoryResearchEventRepository::new()),
        )
    }

    #[must_use]
    pub fn with_repositories(
        auth: AuthConfig,
        operations: Arc<dyn OperationRepository>,
        services: Arc<dyn ServiceSnapshotRepository>,
        watchlists: Arc<dyn WatchlistRepository>,
        providers: Arc<dyn ProviderInstanceRepository>,
        candles: Arc<dyn CandleRepository>,
        research_events: Arc<dyn ResearchEventRepository>,
    ) -> Self {
        Self {
            operations,
            services,
            watchlists,
            providers,
            candles,
            research_events,
            auth,
        }
    }

    /// Replaces the reported state of every service carried by an Agent heartbeat.
    ///
    /// The heartbeat is authoritative for that Agent, so instances the Agent no longer
    /// reports are removed instead of lingering as stale read-model entries.
    pub async fn replace_agent_snapshots(
        &self,
        agent_id: &str,
        snapshots: Vec<ServiceSnapshot>,
    ) -> Result<(), RepositoryError> {
        self.services
            .replace_agent_snapshots(agent_id, snapshots)
            .await
    }

    pub async fn list_snapshots(&self) -> Result<Vec<ServiceSnapshot>, RepositoryError> {
        self.services.list().await
    }

    pub async fn snapshot(
        &self,
        service_instance_id: &str,
    ) -> Result<Option<ServiceSnapshot>, RepositoryError> {
        self.services.get(service_instance_id).await
    }
}
