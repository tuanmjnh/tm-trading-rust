mod ai_briefing;
mod ai_model;
mod ai_provider;
mod ai_task;
mod error;
mod market;
mod news;
pub mod openapi;
mod operation;
mod provider;
mod research;
mod service;
mod system;
mod watchlist;

pub use ai_briefing::{
    AiBriefingDto, AiBriefingListDto, AiBriefingQueryDto, AiBriefingReferenceDto,
    AiBriefingReferenceKindDto, AiBriefingTaskDto,
};
pub use ai_model::{AiModelDto, AiModelListDto, NewAiModelDto, UpdateAiModelDto};
pub use ai_provider::{
    AiProviderProfileDto, AiProviderProfileListDto, NewAiProviderProfileDto,
    UpdateAiProviderProfileDto,
};
pub use ai_task::{AiTaskRouteDto, AiTaskRouteListDto, NewAiTaskRouteDto, UpdateAiTaskRouteDto};
pub use error::{ApiErrorBody, ApiErrorCode, ApiErrorResponse};
pub use market::{
    MarketMoverDto, MarketOverviewDto, MarketPulseDto, MarketStateDto, MarketSymbolDto,
};
pub use news::{
    NewsClusterDto, NewsClusterListDto, NewsClusterQueryDto, NewsDirectionDto, NewsFreshnessDto,
    NewsHealthStateDto, NewsItemDto, NewsLevelDto, NewsListDto, NewsQueryDto, NewsSourceDto,
    NewsSourceListDto,
};
pub use openapi::ApiDoc;
pub use operation::{
    CreateOperationRequest, OperationActionDto, OperationResponse, OperationStatusDto,
};
pub use provider::{
    CreateProviderInstanceRequest, ProviderDefinitionDto, ProviderHealthStateDto,
    ProviderInstanceDto, UpdateProviderInstanceRequest,
};
pub use research::{
    ResearchEventDto, ResearchEventListDto, ResearchEventQueryDto, ResearchFreshnessDto,
    ResearchQualityDto,
};
pub use service::{
    AgentCommandDto, AgentHeartbeatRequest, AgentHeartbeatResponse, ClaimCommandRequest,
    ClaimCommandResponse, CompleteCommandRequest, CompleteCommandResponse, DesiredStateDto,
    ObservedStateDto, ProgressCommandRequest, ProgressCommandResponse, ReadinessDto,
    ServiceSnapshot,
};
pub use system::{ApiVersionInfo, HealthResponse, HealthStatus};
pub use watchlist::{CreateWatchlistRequest, ReplaceWatchlistSymbolsRequest, WatchlistDto};

pub const API_PREFIX: &str = "/api/v1";
pub const API_VERSION: &str = "v1";
pub const CONTRACT_VERSION: &str = "1.0.0";
