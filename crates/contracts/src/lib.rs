mod error;
pub mod openapi;
mod operation;
mod system;

pub use error::{ApiErrorBody, ApiErrorCode, ApiErrorResponse};
pub use openapi::ApiDoc;
pub use operation::{
    CreateOperationRequest, OperationActionDto, OperationResponse, OperationStatusDto,
};
pub use system::{ApiVersionInfo, HealthResponse, HealthStatus};

pub const API_PREFIX: &str = "/api/v1";
pub const API_VERSION: &str = "v1";
pub const CONTRACT_VERSION: &str = "1.0.0";
