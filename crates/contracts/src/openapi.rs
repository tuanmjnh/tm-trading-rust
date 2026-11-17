use utoipa::OpenApi;

use crate::error::ApiErrorResponse;
use crate::operation::{CreateOperationRequest, OperationResponse};
use crate::system::{ApiVersionInfo, HealthResponse};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "TM Trading Rust Management API",
        version = "1.0.0",
        description = "Read-only market intelligence and service-management API."
    ),
    paths(version, health),
    components(schemas(
        ApiVersionInfo,
        HealthResponse,
        ApiErrorResponse,
        CreateOperationRequest,
        OperationResponse
    )),
    tags(
        (name = "System", description = "Version and health endpoints"),
        (name = "Operations", description = "Durable asynchronous service operations")
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
