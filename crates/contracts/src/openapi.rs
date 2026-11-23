use crate::ai_briefing::{
    AiBriefingDto, AiBriefingListDto, AiBriefingQueryDto, AiBriefingReferenceDto,
    AiBriefingReferenceKindDto, AiBriefingTaskDto,
};
use crate::ai_job::{
    AiJobDto, AiJobListDto, AiJobQueryDto, AiJobStatusDto, AiJobTaskDto, NewAiJobDto,
};
use crate::ai_model::{AiModelDto, AiModelListDto, NewAiModelDto, UpdateAiModelDto};
use crate::ai_provider::{
    AiProviderProfileDto, AiProviderProfileListDto, NewAiProviderProfileDto,
    UpdateAiProviderProfileDto,
};
use crate::ai_task::{AiTaskRouteDto, AiTaskRouteListDto, NewAiTaskRouteDto, UpdateAiTaskRouteDto};
use utoipa::OpenApi;

use crate::error::ApiErrorResponse;
use crate::market::{
    MarketMoverDto, MarketOverviewDto, MarketPulseDto, MarketStateDto, MarketSymbolDto,
};
use crate::news::{
    NewsClusterDto, NewsClusterListDto, NewsClusterQueryDto, NewsDirectionDto, NewsFreshnessDto,
    NewsHealthStateDto, NewsItemDto, NewsLevelDto, NewsListDto, NewsQueryDto, NewsSourceDto,
    NewsSourceListDto,
};
use crate::notification::{
    NotificationAttemptDto, NotificationChannelDto, NotificationLogItemDto, NotificationLogListDto,
    NotificationLogQueryDto, NotificationOutboxItemDto, NotificationOutboxListDto,
    NotificationOutboxQueryDto, NotificationOutboxSummaryDto, NotificationPriorityDto,
    NotificationStateDto, NotificationSummaryQueryDto, SendTestNotificationRequest,
    SendTestNotificationResponse,
};
use crate::operation::{CreateOperationRequest, OperationResponse};
use crate::provider::{
    CreateProviderInstanceRequest, ProviderDefinitionDto, ProviderHealthStateDto,
    ProviderInstanceDto, UpdateProviderInstanceRequest,
};
use crate::research::{
    ResearchEventDto, ResearchEventListDto, ResearchEventQueryDto, ResearchFreshnessDto,
    ResearchQualityDto,
};
use crate::service::{
    AgentCommandDto, AgentHeartbeatRequest, AgentHeartbeatResponse, ClaimCommandRequest,
    ClaimCommandResponse, CompleteCommandRequest, CompleteCommandResponse, ProgressCommandRequest,
    ProgressCommandResponse, ServiceSnapshot,
};
use crate::system::{
    ApiVersionInfo, HealthResponse, NotificationDiagnosticsDto, StorageDiagnosticsDto,
    SupportBundleDto, SystemDiagnosticsDto, WorkerDiagnosticsDto,
};
use crate::watchlist::{CreateWatchlistRequest, ReplaceWatchlistSymbolsRequest, WatchlistDto};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "TM Trading Rust Management API",
        version = "1.0.0",
        description = "Read-only market intelligence and service-management API."
    ),
    paths(
        version,
        health,
        system_diagnostics,
        create_support_bundle,
        list_services,
        get_service,
        create_service_operation,
        get_operation,
        agent_heartbeat,
        agent_claim_command,
        agent_progress_command,
        agent_complete_command,
        list_watchlists,
        create_watchlist,
        replace_watchlist_symbols,
        delete_watchlist,
        list_provider_catalog,
        list_provider_instances,
        create_provider_instance,
        get_provider_instance,
        update_provider_instance,
        delete_provider_instance,
        test_provider_instance,
        market_overview,
        market_pulse,
        list_research_events,
        get_research_event,
        list_news,
        get_news_item,
        list_news_sources,
        list_news_clusters,
        list_ai_provider_profiles,
        create_ai_provider_profile,
        get_ai_provider_profile,
        update_ai_provider_profile,
        delete_ai_provider_profile,
        test_ai_provider_profile,
        list_ai_models,
        create_ai_model,
        get_ai_model,
        update_ai_model,
        delete_ai_model,
        test_ai_model,
        list_ai_task_routes,
        get_ai_task_route,
        create_ai_task_route,
        update_ai_task_route,
        delete_ai_task_route,
        list_ai_briefings,
        get_ai_briefing,
        list_ai_jobs,
        create_ai_job,
        get_ai_job,
        list_notification_outbox,
        summary_notification_outbox,
        list_notification_logs,
        send_test_notification
    ),
    components(schemas(
        ApiVersionInfo,
        HealthResponse,
        StorageDiagnosticsDto,
        WorkerDiagnosticsDto,
        NotificationDiagnosticsDto,
        SystemDiagnosticsDto,
        SupportBundleDto,
        ApiErrorResponse,
        CreateOperationRequest,
        OperationResponse,
        ServiceSnapshot,
        AgentHeartbeatRequest,
        AgentHeartbeatResponse,
        ClaimCommandRequest,
        ClaimCommandResponse,
        AgentCommandDto,
        ProgressCommandRequest,
        ProgressCommandResponse,
        CompleteCommandRequest,
        CompleteCommandResponse,
        WatchlistDto,
        CreateWatchlistRequest,
        ReplaceWatchlistSymbolsRequest,
        ProviderDefinitionDto,
        ProviderInstanceDto,
        ProviderHealthStateDto,
        CreateProviderInstanceRequest,
        UpdateProviderInstanceRequest,
        MarketStateDto,
        MarketSymbolDto,
        MarketOverviewDto,
        MarketMoverDto,
        MarketPulseDto,
        ResearchEventDto,
        ResearchEventListDto,
        ResearchEventQueryDto,
        ResearchFreshnessDto,
        ResearchQualityDto,
        NewsItemDto,
        NewsListDto,
        NewsQueryDto,
        NewsLevelDto,
        NewsDirectionDto,
        NewsFreshnessDto,
        NewsSourceDto,
        NewsSourceListDto,
        NewsHealthStateDto,
        NewsClusterDto,
        AiProviderProfileDto,
        AiProviderProfileListDto,
        NewAiProviderProfileDto,
        UpdateAiProviderProfileDto,
        AiModelDto,
        AiModelListDto,
        NewAiModelDto,
        UpdateAiModelDto,
        AiTaskRouteDto,
        AiTaskRouteListDto,
        NewAiTaskRouteDto,
        UpdateAiTaskRouteDto,
        AiBriefingDto,
        AiBriefingListDto,
        AiBriefingQueryDto,
        AiBriefingReferenceDto,
        AiBriefingReferenceKindDto,
        AiBriefingTaskDto,
        AiJobDto,
        AiJobListDto,
        AiJobQueryDto,
        AiJobStatusDto,
        NewAiJobDto,
        NewsClusterListDto,
        NewsClusterQueryDto,
        NotificationChannelDto,
        NotificationPriorityDto,
        NotificationStateDto,
        NotificationOutboxSummaryDto,
        NotificationOutboxItemDto,
        NotificationOutboxListDto,
        NotificationOutboxQueryDto,
        NotificationSummaryQueryDto,
        NotificationAttemptDto,
        NotificationLogItemDto,
        NotificationLogListDto,
        NotificationLogQueryDto,
        SendTestNotificationRequest,
        SendTestNotificationResponse
    )),
    tags(
        (name = "System", description = "Version and health endpoints"),
        (name = "Services", description = "Service read model"),
        (name = "Operations", description = "Durable asynchronous service operations"),
        (name = "Watchlists", description = "Owner-scoped symbol lists"),
        (name = "Providers", description = "Provider catalog and configured instances"),
        (name = "Market", description = "Closed-candle market overview and pulse"),
        (name = "Research", description = "Owner-scoped deterministic research observations"),
        (name = "News", description = "Rights-safe normalized news items and their source registry"),
        (name = "AI", description = "AI provider, model, routing, and evidence-bound briefing records"),
        (name = "Notifications", description = "Durable outbox, delivery logs, and notification transport"),
        (name = "Agent", description = "Agent-only heartbeat and command endpoints")
    )
)]
pub struct ApiDoc;

#[utoipa::path(
    get,
    path = "/api/v1/version",
    tag = "System",
    responses(
        (status = 200, description = "API and service versions", body = ApiVersionInfo),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn version() {}

#[utoipa::path(
    get,
    path = "/api/v1/health",
    tag = "System",
    responses(
        (status = 200, description = "Current API health", body = HealthResponse),
        (status = 503, description = "A required dependency is unavailable", body = HealthResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn health() {}

#[utoipa::path(
    get,
    path = "/api/v1/system/diagnostics",
    tag = "System",
    responses(
        (status = 200, description = "Secret-redacted system diagnostics report", body = SystemDiagnosticsDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn system_diagnostics() {}

#[utoipa::path(
    post,
    path = "/api/v1/system/support-bundle",
    tag = "System",
    responses(
        (status = 200, description = "Redacted diagnostic support bundle", body = SupportBundleDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn create_support_bundle() {}

#[utoipa::path(
    get,
    path = "/api/v1/services",
    tag = "Services",
    responses(
        (status = 200, description = "Known service instances and their last reported state", body = Vec<ServiceSnapshot>),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn list_services() {}

#[utoipa::path(
    get,
    path = "/api/v1/services/{service_instance_id}",
    tag = "Services",
    params(("service_instance_id" = String, Path, description = "Service instance identifier")),
    responses(
        (status = 200, description = "Service instance state", body = ServiceSnapshot),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 404, description = "Unknown service instance", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn get_service() {}

#[utoipa::path(
    post,
    path = "/api/v1/services/{service_instance_id}/operations",
    tag = "Operations",
    params(
        ("service_instance_id" = String, Path, description = "Service instance identifier"),
        ("Idempotency-Key" = String, Header, description = "Caller supplied idempotency key")
    ),
    request_body(content = CreateOperationRequest),
    responses(
        (status = 202, description = "Operation accepted", body = OperationResponse),
        (status = 400, description = "Malformed request", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 409, description = "Idempotency key reused with a different request", body = ApiErrorResponse),
        (status = 422, description = "Request does not match the target service", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn create_service_operation() {}

#[utoipa::path(
    get,
    path = "/api/v1/operations/{operation_id}",
    tag = "Operations",
    params(("operation_id" = String, Path, description = "Operation identifier")),
    responses(
        (status = 200, description = "Durable operation state", body = OperationResponse),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 404, description = "Unknown operation", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn get_operation() {}

#[utoipa::path(
    post,
    path = "/api/v1/agent/heartbeat",
    tag = "Agent",
    request_body(content = AgentHeartbeatRequest),
    responses(
        (status = 200, description = "Heartbeat accepted", body = AgentHeartbeatResponse),
        (status = 401, description = "Missing or invalid agent credentials", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn agent_heartbeat() {}

#[utoipa::path(
    post,
    path = "/api/v1/agent/commands/claim",
    tag = "Agent",
    request_body(content = ClaimCommandRequest),
    responses(
        (status = 200, description = "Next leased command, if any", body = ClaimCommandResponse),
        (status = 401, description = "Missing or invalid agent credentials", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn agent_claim_command() {}

#[utoipa::path(
    post,
    path = "/api/v1/agent/commands/{operation_id}/progress",
    tag = "Agent",
    params(("operation_id" = String, Path, description = "Operation identifier")),
    request_body(content = ProgressCommandRequest),
    responses(
        (status = 200, description = "Command started executing", body = ProgressCommandResponse),
        (status = 401, description = "Missing or invalid agent credentials", body = ApiErrorResponse),
        (status = 404, description = "Unknown operation", body = ApiErrorResponse),
        (status = 409, description = "Lease is no longer held by this Agent", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn agent_progress_command() {}

#[utoipa::path(
    post,
    path = "/api/v1/agent/commands/{operation_id}/complete",
    tag = "Agent",
    params(("operation_id" = String, Path, description = "Operation identifier")),
    request_body(content = CompleteCommandRequest),
    responses(
        (status = 200, description = "Command result accepted", body = CompleteCommandResponse),
        (status = 401, description = "Missing or invalid agent credentials", body = ApiErrorResponse),
        (status = 404, description = "Unknown operation", body = ApiErrorResponse),
        (status = 409, description = "Lease is no longer held by this Agent", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn agent_complete_command() {}

#[utoipa::path(
    get,
    path = "/api/v1/watchlists",
    tag = "Watchlists",
    responses(
        (status = 200, description = "Watchlists owned by the caller", body = Vec<WatchlistDto>),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing capability", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn list_watchlists() {}

#[utoipa::path(
    post,
    path = "/api/v1/watchlists",
    tag = "Watchlists",
    request_body(content = CreateWatchlistRequest),
    responses(
        (status = 201, description = "Created watchlist", body = WatchlistDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing capability", body = ApiErrorResponse),
        (status = 409, description = "The owner already has a list with this name", body = ApiErrorResponse),
        (status = 422, description = "The watchlist failed validation", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn create_watchlist() {}

#[utoipa::path(
    put,
    path = "/api/v1/watchlists/{watchlist_id}/symbols",
    tag = "Watchlists",
    params(("watchlist_id" = String, Path, description = "Watchlist identifier")),
    request_body(content = ReplaceWatchlistSymbolsRequest),
    responses(
        (status = 200, description = "Updated watchlist", body = WatchlistDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown watchlist", body = ApiErrorResponse),
        (status = 422, description = "The symbol set failed validation", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn replace_watchlist_symbols() {}

#[utoipa::path(
    delete,
    path = "/api/v1/watchlists/{watchlist_id}",
    tag = "Watchlists",
    params(("watchlist_id" = String, Path, description = "Watchlist identifier")),
    responses(
        (status = 204, description = "Watchlist deleted"),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown watchlist", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn delete_watchlist() {}

#[utoipa::path(
    get,
    path = "/api/v1/providers/catalog",
    tag = "Providers",
    responses(
        (status = 200, description = "Provider adapters compiled into this service", body = Vec<ProviderDefinitionDto>),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing capability", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn list_provider_catalog() {}

#[utoipa::path(
    get,
    path = "/api/v1/providers",
    tag = "Providers",
    responses(
        (status = 200, description = "Configured provider instances", body = Vec<ProviderInstanceDto>),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing capability", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn list_provider_instances() {}

#[utoipa::path(
    post,
    path = "/api/v1/providers",
    tag = "Providers",
    request_body(content = CreateProviderInstanceRequest),
    responses(
        (status = 201, description = "Created provider instance", body = ProviderInstanceDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing capability", body = ApiErrorResponse),
        (status = 422, description = "Unknown definition or invalid configuration", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn create_provider_instance() {}

#[utoipa::path(
    get,
    path = "/api/v1/providers/{provider_instance_id}",
    tag = "Providers",
    params(("provider_instance_id" = String, Path, description = "Provider instance identifier")),
    responses(
        (status = 200, description = "Configured provider instance", body = ProviderInstanceDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown provider instance", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn get_provider_instance() {}

#[utoipa::path(
    patch,
    path = "/api/v1/providers/{provider_instance_id}",
    tag = "Providers",
    params(("provider_instance_id" = String, Path, description = "Provider instance identifier")),
    request_body(content = UpdateProviderInstanceRequest),
    responses(
        (status = 200, description = "Updated provider instance", body = ProviderInstanceDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown provider instance", body = ApiErrorResponse),
        (status = 422, description = "Invalid configuration", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn update_provider_instance() {}

#[utoipa::path(
    delete,
    path = "/api/v1/providers/{provider_instance_id}",
    tag = "Providers",
    params(("provider_instance_id" = String, Path, description = "Provider instance identifier")),
    responses(
        (status = 204, description = "Provider instance deleted"),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown provider instance", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn delete_provider_instance() {}

#[utoipa::path(
    post,
    path = "/api/v1/providers/{provider_instance_id}/test",
    tag = "Providers",
    params(("provider_instance_id" = String, Path, description = "Provider instance identifier")),
    responses(
        (status = 200, description = "Bounded health probe recorded on the instance", body = ProviderInstanceDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown provider instance", body = ApiErrorResponse),
        (status = 422, description = "The instance cannot build an adapter", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn test_provider_instance() {}

#[utoipa::path(
    get,
    path = "/api/v1/market/overview",
    tag = "Market",
    params(
        ("symbols" = Option<String>, Query, description = "Comma-separated symbols; defaults to the caller's default watchlist"),
        ("timeframe" = Option<String>, Query, description = "Closed-candle timeframe, default 1m"),
        ("bars" = Option<usize>, Query, description = "Bars per symbol, 1 to 500, default 60")
    ),
    responses(
        (status = 200, description = "Closed-candle overview with an explicit empty, partial, or stale state", body = MarketOverviewDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing capability", body = ApiErrorResponse),
        (status = 422, description = "Unsupported timeframe or an out-of-range request", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn market_overview() {}

#[utoipa::path(
    get,
    path = "/api/v1/market/pulse",
    tag = "Market",
    params(
        ("symbols" = Option<String>, Query, description = "Comma-separated symbols; defaults to the caller's default watchlist"),
        ("timeframe" = Option<String>, Query, description = "Closed-candle timeframe, default 1m"),
        ("bars" = Option<usize>, Query, description = "Bars per symbol, 1 to 500, default 60")
    ),
    responses(
        (status = 200, description = "Ranked movers with a change, volume ratio, and volatility summary", body = MarketPulseDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing capability", body = ApiErrorResponse),
        (status = 422, description = "Unsupported timeframe or an out-of-range request", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn market_pulse() {}

#[utoipa::path(
    get,
    path = "/api/v1/research/events",
    tag = "Research",
    params(
        ("symbol" = Option<String>, Query, description = "Exact symbol filter"),
        ("timeframe" = Option<String>, Query, description = "Exact timeframe filter"),
        ("event_type" = Option<String>, Query, description = "Exact event type filter"),
        ("since" = Option<String>, Query, description = "Only events detected at or after this RFC 3339 instant"),
        ("limit" = Option<usize>, Query, description = "Maximum number of events, 1 to 200, default 50")
    ),
    responses(
        (status = 200, description = "Owner-scoped research observations, newest first", body = ResearchEventListDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 422, description = "The requested limit is out of range", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn list_research_events() {}

#[utoipa::path(
    get,
    path = "/api/v1/research/events/{event_id}",
    tag = "Research",
    params(("event_id" = String, Path, description = "Research event identifier")),
    responses(
        (status = 200, description = "Research observation with provenance and freshness", body = ResearchEventDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown research event", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn get_research_event() {}

#[utoipa::path(
    get,
    path = "/api/v1/news",
    tag = "News",
    params(
        ("source_id" = Option<String>, Query, description = "Exact source identifier filter"),
        ("market_group" = Option<String>, Query, description = "Exact market group filter"),
        ("level" = Option<String>, Query, description = "Exact editorial level: HIGH, MED, or LOW"),
        ("since" = Option<String>, Query, description = "Only items ingested at or after this RFC 3339 instant"),
        ("limit" = Option<usize>, Query, description = "Maximum number of items, 1 to 200, default 50")
    ),
    responses(
        (status = 200, description = "Rights-safe normalized stories, newest ingestion first", body = NewsListDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 422, description = "The requested limit or level is invalid", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn list_news() {}

#[utoipa::path(
    get,
    path = "/api/v1/news/{news_id}",
    tag = "News",
    params(("news_id" = String, Path, description = "Deterministic news item identifier")),
    responses(
        (status = 200, description = "One rights-safe normalized story", body = NewsItemDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown or non-displayable news item", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn get_news_item() {}

#[utoipa::path(
    get,
    path = "/api/v1/news/sources",
    tag = "News",
    responses(
        (status = 200, description = "Configured news sources with their observed health", body = NewsSourceListDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn list_news_sources() {}

#[utoipa::path(
    get,
    path = "/api/v1/news/clusters",
    tag = "News",
    params(
        ("market_group" = Option<String>, Query, description = "Exact market group filter"),
        ("since" = Option<String>, Query, description = "Only clusters whose last observation is at or after this RFC 3339 instant"),
        ("limit" = Option<usize>, Query, description = "Maximum number of clusters, 1 to 200, default 50")
    ),
    responses(
        (status = 200, description = "News event clusters, newest observation first", body = NewsClusterListDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 422, description = "The requested limit is invalid", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn list_news_clusters() {}

#[utoipa::path(
    get,
    path = "/api/v1/ai-providers",
    tag = "AI",
    responses(
        (status = 200, description = "AI provider profiles", body = AiProviderProfileListDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn list_ai_provider_profiles() {}

#[utoipa::path(
    post,
    path = "/api/v1/ai-providers",
    tag = "AI",
    request_body = NewAiProviderProfileDto,
    responses(
        (status = 200, description = "Created AI provider profile", body = AiProviderProfileDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 422, description = "Invalid AI provider configuration", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn create_ai_provider_profile() {}

#[utoipa::path(
    get,
    path = "/api/v1/ai-providers/{id}",
    tag = "AI",
    params(("id" = String, Path, description = "AI provider profile identifier")),
    responses(
        (status = 200, description = "AI provider profile", body = AiProviderProfileDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown AI provider profile", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn get_ai_provider_profile() {}

#[utoipa::path(
    patch,
    path = "/api/v1/ai-providers/{id}",
    tag = "AI",
    params(("id" = String, Path, description = "AI provider profile identifier")),
    request_body = UpdateAiProviderProfileDto,
    responses(
        (status = 200, description = "Updated AI provider profile", body = AiProviderProfileDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown AI provider profile", body = ApiErrorResponse),
        (status = 422, description = "Invalid AI provider configuration", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn update_ai_provider_profile() {}

#[utoipa::path(
    delete,
    path = "/api/v1/ai-providers/{id}",
    tag = "AI",
    params(("id" = String, Path, description = "AI provider profile identifier")),
    responses(
        (status = 200, description = "AI provider profile deleted"),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn delete_ai_provider_profile() {}

#[utoipa::path(
    post,
    path = "/api/v1/ai-providers/{id}/test",
    tag = "AI",
    params(("id" = String, Path, description = "AI provider profile identifier")),
    responses(
        (status = 200, description = "AI provider health test result", body = AiProviderProfileDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown AI provider profile", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn test_ai_provider_profile() {}

#[utoipa::path(
    get,
    path = "/api/v1/ai-models",
    tag = "AI",
    responses(
        (status = 200, description = "AI models", body = AiModelListDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn list_ai_models() {}

#[utoipa::path(
    post,
    path = "/api/v1/ai-providers/{id}/models",
    tag = "AI",
    params(("id" = String, Path, description = "AI provider profile identifier")),
    request_body = NewAiModelDto,
    responses(
        (status = 200, description = "Created AI model", body = AiModelDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 422, description = "Invalid AI model configuration", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn create_ai_model() {}

#[utoipa::path(
    get,
    path = "/api/v1/ai-models/{id}",
    tag = "AI",
    params(("id" = String, Path, description = "AI model identifier")),
    responses(
        (status = 200, description = "AI model", body = AiModelDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown AI model", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn get_ai_model() {}

#[utoipa::path(
    patch,
    path = "/api/v1/ai-models/{id}",
    tag = "AI",
    params(("id" = String, Path, description = "AI model identifier")),
    request_body = UpdateAiModelDto,
    responses(
        (status = 200, description = "Updated AI model", body = AiModelDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown AI model", body = ApiErrorResponse),
        (status = 422, description = "Invalid AI model configuration", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn update_ai_model() {}

#[utoipa::path(
    delete,
    path = "/api/v1/ai-models/{id}",
    tag = "AI",
    params(("id" = String, Path, description = "AI model identifier")),
    responses(
        (status = 200, description = "AI model deleted"),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn delete_ai_model() {}

#[utoipa::path(
    post,
    path = "/api/v1/ai-models/{id}/test",
    tag = "AI",
    params(("id" = String, Path, description = "AI model identifier")),
    responses(
        (status = 200, description = "AI model health observation", body = AiModelDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown AI model", body = ApiErrorResponse),
        (status = 422, description = "AI model belongs to a missing provider", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn test_ai_model() {}

#[utoipa::path(
    get,
    path = "/api/v1/ai-task-routes",
    tag = "AI",
    responses(
        (status = 200, description = "AI task routes", body = AiTaskRouteListDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn list_ai_task_routes() {}

#[utoipa::path(
    get,
    path = "/api/v1/ai-task-routes/{task}",
    tag = "AI",
    params(("task" = String, Path, description = "AI task identifier")),
    responses(
        (status = 200, description = "AI task route", body = AiTaskRouteDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown AI task route", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn get_ai_task_route() {}

#[utoipa::path(
    post,
    path = "/api/v1/ai-task-routes",
    tag = "AI",
    request_body = NewAiTaskRouteDto,
    responses(
        (status = 200, description = "Configured AI task route", body = AiTaskRouteDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 422, description = "Invalid AI task route", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn create_ai_task_route() {}

#[utoipa::path(
    patch,
    path = "/api/v1/ai-task-routes/{task}",
    tag = "AI",
    params(("task" = String, Path, description = "AI task identifier")),
    request_body = UpdateAiTaskRouteDto,
    responses(
        (status = 200, description = "Updated AI task route", body = AiTaskRouteDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown AI task route", body = ApiErrorResponse),
        (status = 422, description = "Invalid AI task route", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn update_ai_task_route() {}

#[utoipa::path(
    delete,
    path = "/api/v1/ai-task-routes/{task}",
    tag = "AI",
    params(("task" = String, Path, description = "AI task identifier")),
    responses(
        (status = 200, description = "AI task route deleted"),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown AI task route", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn delete_ai_task_route() {}

#[utoipa::path(
    get,
    path = "/api/v1/ai/briefings",
    tag = "AI",
    params(
        ("task" = Option<AiBriefingTaskDto>, Query, description = "Exact AI task filter"),
        ("limit" = Option<usize>, Query, description = "Maximum number of briefings, 1 to 200, default 50", minimum = 1, maximum = 200)
    ),
    responses(
        (status = 200, description = "Owner-scoped AI briefings, newest first", body = AiBriefingListDto),
        (status = 400, description = "Malformed query parameter", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 422, description = "The requested task or limit is invalid", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn list_ai_briefings() {}

#[utoipa::path(
    get,
    path = "/api/v1/ai/briefings/{briefing_id}",
    tag = "AI",
    params(("briefing_id" = String, Path, description = "AI briefing identifier", pattern = "^aibr1_[0-9a-f]{16}$")),
    responses(
        (status = 200, description = "Evidence-bound AI briefing", body = AiBriefingDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown AI briefing", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn get_ai_briefing() {}

#[utoipa::path(
    get,
    path = "/api/v1/ai/jobs",
    tag = "AI",
    params(
        ("task" = Option<AiJobTaskDto>, Query, description = "Exact AI task filter"),
        ("limit" = Option<usize>, Query, description = "Maximum number of jobs, 1 to 200, default 50", minimum = 1, maximum = 200)
    ),
    responses(
        (status = 200, description = "Owner-scoped AI jobs, newest first", body = AiJobListDto),
        (status = 400, description = "Malformed query parameter", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 422, description = "The requested task or limit is invalid", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn list_ai_jobs() {}

#[utoipa::path(
    post,
    path = "/api/v1/ai/jobs",
    tag = "AI",
    request_body = NewAiJobDto,
    responses(
        (status = 200, description = "The queued job was already stored", body = AiJobDto),
        (status = 201, description = "AI job queued", body = AiJobDto),
        (status = 400, description = "Malformed JSON request body", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 409, description = "The idempotency key was reused with a different request", body = ApiErrorResponse),
        (status = 422, description = "The request body is invalid under the API contract (including unknown fields), or the task or requested inputs are invalid", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn create_ai_job() {}

#[utoipa::path(
    get,
    path = "/api/v1/ai/jobs/{job_id}",
    tag = "AI",
    params(("job_id" = String, Path, description = "AI job identifier", pattern = "^aijob1_[0-9a-f]{32}$")),
    responses(
        (status = 200, description = "Owner-scoped AI job", body = AiJobDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 404, description = "Unknown AI job", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn get_ai_job() {}

#[utoipa::path(
    get,
    path = "/api/v1/notifications/outbox",
    tag = "Notifications",
    params(
        ("owner_id" = Option<String>, Query, description = "Owner identifier filter"),
        ("channel" = Option<NotificationChannelDto>, Query, description = "Target delivery channel filter"),
        ("state" = Option<NotificationStateDto>, Query, description = "Delivery state filter"),
        ("dead" = Option<bool>, Query, description = "Filter dead-letter items"),
        ("limit" = Option<usize>, Query, description = "Maximum number of items, 1 to 200, default 50", minimum = 1, maximum = 200)
    ),
    responses(
        (status = 200, description = "Durable outbox items", body = NotificationOutboxListDto),
        (status = 400, description = "Malformed query parameter", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 422, description = "The requested limit or parameters are invalid", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn list_notification_outbox() {}

#[utoipa::path(
    get,
    path = "/api/v1/notifications/outbox/summary",
    tag = "Notifications",
    params(
        ("owner_id" = Option<String>, Query, description = "Owner identifier filter")
    ),
    responses(
        (status = 200, description = "Summary counts of durable notification outbox", body = NotificationOutboxSummaryDto),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn summary_notification_outbox() {}

#[utoipa::path(
    get,
    path = "/api/v1/notifications/logs",
    tag = "Notifications",
    params(
        ("owner_id" = Option<String>, Query, description = "Owner identifier filter"),
        ("kind" = Option<String>, Query, description = "Notification kind filter"),
        ("priority" = Option<NotificationPriorityDto>, Query, description = "Priority filter"),
        ("limit" = Option<usize>, Query, description = "Maximum number of logs, 1 to 200, default 50", minimum = 1, maximum = 200)
    ),
    responses(
        (status = 200, description = "Delivery audit logs, newest first", body = NotificationLogListDto),
        (status = 400, description = "Malformed query parameter", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 422, description = "The requested limit or parameters are invalid", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn list_notification_logs() {}

#[utoipa::path(
    post,
    path = "/api/v1/notifications/test",
    tag = "Notifications",
    request_body = SendTestNotificationRequest,
    responses(
        (status = 200, description = "Test notification outcome", body = SendTestNotificationResponse),
        (status = 400, description = "Malformed JSON request body", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid credentials", body = ApiErrorResponse),
        (status = 403, description = "Missing required capability", body = ApiErrorResponse),
        (status = 500, description = "Unexpected server error", body = ApiErrorResponse)
    )
)]
pub fn send_test_notification() {}
