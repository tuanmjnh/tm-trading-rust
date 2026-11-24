mod ai_briefing;
mod ai_job;
mod ai_model;
mod ai_provider;
mod ai_task;
mod backup;
mod dashboard;
mod error;
mod market;
mod news;
mod notification;
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
pub use ai_job::{
    AiJobDto, AiJobListDto, AiJobQueryDto, AiJobStatusDto, AiJobTaskDto, NewAiJobDto,
};
pub use ai_model::{AiModelDto, AiModelListDto, NewAiModelDto, UpdateAiModelDto};
pub use ai_provider::{
    AiProviderProfileDto, AiProviderProfileListDto, NewAiProviderProfileDto,
    UpdateAiProviderProfileDto,
};
pub use ai_task::{AiTaskRouteDto, AiTaskRouteListDto, NewAiTaskRouteDto, UpdateAiTaskRouteDto};
pub use backup::{
    BackupArchiveDto, BackupPayloadDto, RestoreBackupRequestDto, RestoreBackupResultDto,
};
pub use dashboard::{
    AiSummaryDto, DashboardSummaryDto, MarketSummaryDto, NewsSummaryDto, NotificationsSummaryDto,
    ResearchSummaryDto, ServicesSummaryDto,
};
pub use error::{ApiErrorBody, ApiErrorCode, ApiErrorResponse};
pub use market::{
    FundingRateDto, LiquidationZoneDto, MarketFundingDto, MarketLiquidationDto, MarketMoverDto,
    MarketOverviewDto, MarketPulseDto, MarketStateDto, MarketSymbolDto, TradingViewAlertPayload,
    WebhookResponse,
};
pub use news::{
    NewsClusterDto, NewsClusterListDto, NewsClusterQueryDto, NewsDirectionDto, NewsFreshnessDto,
    NewsHealthStateDto, NewsItemDto, NewsLevelDto, NewsListDto, NewsQueryDto, NewsSourceDto,
    NewsSourceListDto,
};
pub use notification::{
    NotificationAttemptDto, NotificationChannelDto, NotificationLogItemDto, NotificationLogListDto,
    NotificationLogQueryDto, NotificationOutboxItemDto, NotificationOutboxListDto,
    NotificationOutboxQueryDto, NotificationOutboxSummaryDto, NotificationPriorityDto,
    NotificationStateDto, NotificationSummaryQueryDto, SendTestNotificationRequest,
    SendTestNotificationResponse,
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
pub use system::{
    ApiVersionInfo, HealthResponse, HealthStatus, NotificationDiagnosticsDto,
    StorageDiagnosticsDto, SupportBundleDto, SystemDiagnosticsDto, WorkerDiagnosticsDto,
};
pub use watchlist::{CreateWatchlistRequest, ReplaceWatchlistSymbolsRequest, WatchlistDto};

pub const API_PREFIX: &str = "/api/v1";
pub const API_VERSION: &str = "v1";
pub const CONTRACT_VERSION: &str = "1.0.0";
