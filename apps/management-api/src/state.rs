use std::sync::Arc;

use tm_trading_contracts::ServiceSnapshot;
use tm_trading_storage::{
    AiBriefingRepository, AiModelRepository, AiProviderProfileRepository, AiTaskRouteRepository,
    CandleRepository, InMemoryAiBriefingRepository, InMemoryAiModelRepository,
    InMemoryAiProviderProfileRepository, InMemoryAiTaskRouteRepository, InMemoryCandleRepository,
    InMemoryNewsClusterRepository, InMemoryNewsItemRepository, InMemoryNewsSourceRepository,
    InMemoryOperationRepository, InMemoryProviderInstanceRepository,
    InMemoryResearchEventRepository, InMemoryServiceSnapshotRepository,
    InMemoryWatchlistRepository, NewsClusterRepository, NewsItemRepository, NewsSourceRepository,
    OperationRepository, ProviderInstanceRepository, RepositoryError, ResearchEventRepository,
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

/// Repository set injected into the shared application state.
pub struct Repositories {
    pub operations: Arc<dyn OperationRepository>,
    pub services: Arc<dyn ServiceSnapshotRepository>,
    pub watchlists: Arc<dyn WatchlistRepository>,
    pub providers: Arc<dyn ProviderInstanceRepository>,
    pub ai_provider_profiles: Arc<dyn AiProviderProfileRepository>,
    pub ai_models: Arc<dyn AiModelRepository>,
    pub ai_task_routes: Arc<dyn AiTaskRouteRepository>,
    pub ai_briefings: Arc<dyn AiBriefingRepository>,
    pub candles: Arc<dyn CandleRepository>,
    pub research_events: Arc<dyn ResearchEventRepository>,
    pub news_sources: Arc<dyn NewsSourceRepository>,
    pub news_items: Arc<dyn NewsItemRepository>,
    pub news_clusters: Arc<dyn NewsClusterRepository>,
}

impl Repositories {
    /// In-memory stores used by the local default and by tests.
    #[must_use]
    pub fn in_memory() -> Self {
        Self {
            operations: Arc::new(InMemoryOperationRepository::new()),
            services: Arc::new(InMemoryServiceSnapshotRepository::new()),
            watchlists: Arc::new(InMemoryWatchlistRepository::new()),
            providers: Arc::new(InMemoryProviderInstanceRepository::new()),
            ai_provider_profiles: Arc::new(InMemoryAiProviderProfileRepository::new()),
            ai_models: Arc::new(InMemoryAiModelRepository::new()),
            ai_task_routes: Arc::new(InMemoryAiTaskRouteRepository::new()),
            ai_briefings: Arc::new(InMemoryAiBriefingRepository::new()),
            candles: Arc::new(InMemoryCandleRepository::new()),
            research_events: Arc::new(InMemoryResearchEventRepository::new()),
            news_sources: Arc::new(InMemoryNewsSourceRepository::new()),
            news_items: Arc::new(InMemoryNewsItemRepository::new()),
            news_clusters: Arc::new(InMemoryNewsClusterRepository::new()),
        }
    }
}

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub operations: Arc<dyn OperationRepository>,
    pub services: Arc<dyn ServiceSnapshotRepository>,
    pub watchlists: Arc<dyn WatchlistRepository>,
    pub providers: Arc<dyn ProviderInstanceRepository>,
    pub ai_provider_profiles: Arc<dyn AiProviderProfileRepository>,
    pub ai_models: Arc<dyn AiModelRepository>,
    pub ai_task_routes: Arc<dyn AiTaskRouteRepository>,
    pub ai_briefings: Arc<dyn AiBriefingRepository>,
    pub candles: Arc<dyn CandleRepository>,
    pub research_events: Arc<dyn ResearchEventRepository>,
    pub news_sources: Arc<dyn NewsSourceRepository>,
    pub news_items: Arc<dyn NewsItemRepository>,
    pub news_clusters: Arc<dyn NewsClusterRepository>,
    pub auth: AuthConfig,
}

impl AppState {
    #[must_use]
    pub fn new(auth: AuthConfig) -> Self {
        Self::with_repositories(auth, Repositories::in_memory())
    }

    #[must_use]
    pub fn with_repositories(auth: AuthConfig, repositories: Repositories) -> Self {
        Self {
            operations: repositories.operations,
            services: repositories.services,
            watchlists: repositories.watchlists,
            providers: repositories.providers,
            ai_provider_profiles: repositories.ai_provider_profiles,
            ai_models: repositories.ai_models,
            ai_task_routes: repositories.ai_task_routes,
            ai_briefings: repositories.ai_briefings,
            candles: repositories.candles,
            research_events: repositories.research_events,
            news_sources: repositories.news_sources,
            news_items: repositories.news_items,
            news_clusters: repositories.news_clusters,
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
