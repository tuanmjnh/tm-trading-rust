mod error;
mod market;
pub mod openapi;
mod operation;
mod provider;
mod service;
mod system;
mod watchlist;

pub use error::{ApiErrorBody, ApiErrorCode, ApiErrorResponse};
pub use market::{
    MarketMoverDto, MarketOverviewDto, MarketPulseDto, MarketStateDto, MarketSymbolDto,
};
pub use openapi::ApiDoc;
pub use operation::{
    CreateOperationRequest, OperationActionDto, OperationResponse, OperationStatusDto,
};
pub use provider::{
    CreateProviderInstanceRequest, ProviderDefinitionDto, ProviderHealthStateDto,
    ProviderInstanceDto, UpdateProviderInstanceRequest,
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
