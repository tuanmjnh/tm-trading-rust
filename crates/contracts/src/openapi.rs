use utoipa::OpenApi;

use crate::error::ApiErrorResponse;
use crate::operation::{CreateOperationRequest, OperationResponse};
use crate::service::{
    AgentCommandDto, AgentHeartbeatRequest, AgentHeartbeatResponse, ClaimCommandRequest,
    ClaimCommandResponse, CompleteCommandRequest, CompleteCommandResponse, ProgressCommandRequest,
    ProgressCommandResponse, ServiceSnapshot,
};
use crate::system::{ApiVersionInfo, HealthResponse};

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
        list_services,
        get_service,
        create_service_operation,
        get_operation,
        agent_heartbeat,
        agent_claim_command,
        agent_progress_command,
        agent_complete_command
    ),
    components(schemas(
        ApiVersionInfo,
        HealthResponse,
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
        CompleteCommandResponse
    )),
    tags(
        (name = "System", description = "Version and health endpoints"),
        (name = "Services", description = "Service read model"),
        (name = "Operations", description = "Durable asynchronous service operations"),
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
