use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use chrono::{Duration as ChronoDuration, Utc};
use tm_trading_contracts::{
    AgentCommandDto, AgentHeartbeatRequest, AgentHeartbeatResponse, ApiVersionInfo,
    ClaimCommandRequest, ClaimCommandResponse, CompleteCommandRequest, CompleteCommandResponse,
    CreateOperationRequest, HealthResponse, HealthStatus, OperationActionDto, OperationResponse,
    ProgressCommandRequest, ProgressCommandResponse, ServiceSnapshot,
};
use tm_trading_domain::{NewOperation, Operation, OperationAction, OperationId};
use uuid::Uuid;

use crate::auth::{
    AuthContext, CAPABILITY_OPERATION_READ, CAPABILITY_SERVICE_CONTROL, CAPABILITY_SERVICE_READ,
};
use crate::error::ApiError;
use crate::state::AppState;

const OPERATION_TTL_MINUTES: i64 = 10;
const COMMAND_LEASE_SECONDS: u64 = 60;

pub async fn version() -> Json<ApiVersionInfo> {
    Json(ApiVersionInfo::current())
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: HealthStatus::Healthy,
        version: ApiVersionInfo::current(),
    })
}

pub async fn list_services(
    State(state): State<AppState>,
    auth: AuthContext,
) -> Result<Json<Vec<ServiceSnapshot>>, ApiError> {
    auth.require_capability(CAPABILITY_SERVICE_READ)?;
    Ok(Json(state.list_snapshots().await.map_err(ApiError::from)?))
}

pub async fn get_service(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(service_instance_id): Path<String>,
) -> Result<Json<ServiceSnapshot>, ApiError> {
    auth.require_capability(CAPABILITY_SERVICE_READ)?;
    let snapshot = state
        .snapshot(&service_instance_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "service instance '{service_instance_id}' is not known"
            ))
        })?;
    Ok(Json(snapshot))
}

pub async fn create_service_operation(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(service_instance_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<CreateOperationRequest>,
) -> Result<(StatusCode, Json<OperationResponse>), ApiError> {
    auth.require_capability(CAPABILITY_SERVICE_CONTROL)?;
    if request.service_instance_id != service_instance_id {
        return Err(ApiError::UnprocessableEntity(
            "request body service instance does not match the request path".to_owned(),
        ));
    }
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::BadRequest("an Idempotency-Key header is required".to_owned()))?
        .to_owned();

    let snapshot = state
        .snapshot(&service_instance_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "service instance '{service_instance_id}' is not known"
            ))
        })?;

    let action = operation_action(request.action)?;
    let requested_at = Utc::now();
    let operation = Operation::new(NewOperation {
        idempotency_key,
        // Canonical request identity used to detect idempotency-key reuse with a different body.
        request_hash: format!("{service_instance_id}|{action:?}"),
        requested_by: auth.subject().to_owned(),
        requested_at,
        target_agent_id: snapshot.agent_id.clone(),
        service_instance_id: snapshot.service_instance_id.clone(),
        service_definition_id: snapshot.service_definition_id.clone(),
        action,
        expires_at: requested_at + ChronoDuration::minutes(OPERATION_TTL_MINUTES),
        correlation_id: Uuid::new_v4(),
    })
    .map_err(|error| ApiError::BadRequest(error.to_string()))?;

    let outcome = state
        .operations
        .create(operation)
        .await
        .map_err(ApiError::from)?;
    let operation = match outcome {
        tm_trading_storage::CreateOperationOutcome::Created(operation) => operation,
        tm_trading_storage::CreateOperationOutcome::Existing(operation) => operation,
    };
    Ok((
        StatusCode::ACCEPTED,
        Json(OperationResponse::from(&operation)),
    ))
}

pub async fn get_operation(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(operation_id): Path<String>,
) -> Result<Json<OperationResponse>, ApiError> {
    auth.require_capability(CAPABILITY_OPERATION_READ)?;
    let operation_id = parse_operation_id(&operation_id)?;
    let operation = state
        .operations
        .get(&operation_id)
        .await
        .map_err(ApiError::from)?
        .ok_or(ApiError::NotFound("operation was not found".to_owned()))?;
    Ok(Json(OperationResponse::from(&operation)))
}

pub async fn agent_heartbeat(
    State(state): State<AppState>,
    auth: AuthContext,
    Json(request): Json<AgentHeartbeatRequest>,
) -> Result<Json<AgentHeartbeatResponse>, ApiError> {
    auth.require_agent(&request.agent_id)?;
    state
        .replace_agent_snapshots(&request.agent_id, request.services)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(AgentHeartbeatResponse {
        accepted: true,
        server_time: Utc::now(),
    }))
}

pub async fn agent_claim_command(
    State(state): State<AppState>,
    auth: AuthContext,
    Json(request): Json<ClaimCommandRequest>,
) -> Result<Json<ClaimCommandResponse>, ApiError> {
    auth.require_agent(&request.agent_id)?;
    let operation = state
        .operations
        .claim_next(
            &request.agent_id,
            Utc::now(),
            Duration::from_secs(COMMAND_LEASE_SECONDS),
        )
        .await
        .map_err(ApiError::from)?;
    let command = operation.map(|operation| AgentCommandDto {
        operation_id: operation.id().to_string(),
        service_instance_id: operation.service_instance_id().to_owned(),
        service_definition_id: operation.service_definition_id().to_owned(),
        action: operation.action().into(),
        fencing_token: operation.fencing_token(),
        lease_expires_at: operation.lease_expires_at().unwrap_or_else(Utc::now),
        expires_at: operation.expires_at(),
    });
    Ok(Json(ClaimCommandResponse { command }))
}

pub async fn agent_progress_command(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(operation_id): Path<String>,
    Json(request): Json<ProgressCommandRequest>,
) -> Result<Json<ProgressCommandResponse>, ApiError> {
    auth.require_agent(&request.agent_id)?;
    let operation_id = parse_operation_id(&operation_id)?;
    state
        .operations
        .mark_running(
            &operation_id,
            &request.agent_id,
            request.fencing_token,
            Utc::now(),
        )
        .await
        .map_err(ApiError::from)?;
    Ok(Json(ProgressCommandResponse { accepted: true }))
}

pub async fn agent_complete_command(
    State(state): State<AppState>,
    auth: AuthContext,
    Path(operation_id): Path<String>,
    Json(request): Json<CompleteCommandRequest>,
) -> Result<Json<CompleteCommandResponse>, ApiError> {
    auth.require_agent(&request.agent_id)?;
    let operation_id = parse_operation_id(&operation_id)?;
    if request.succeeded {
        state
            .operations
            .mark_succeeded(
                &operation_id,
                &request.agent_id,
                request.fencing_token,
                request.result_summary,
                Utc::now(),
            )
            .await
            .map_err(ApiError::from)?;
    } else {
        state
            .operations
            .mark_failed(
                &operation_id,
                &request.agent_id,
                request.fencing_token,
                request.error_code,
                request.redacted_error,
                Utc::now(),
            )
            .await
            .map_err(ApiError::from)?;
    }
    Ok(Json(CompleteCommandResponse { accepted: true }))
}

fn parse_operation_id(value: &str) -> Result<OperationId, ApiError> {
    Uuid::parse_str(value)
        .map(OperationId::from_uuid)
        .map_err(|_| ApiError::BadRequest("operation identifier is not a UUID".to_owned()))
}

fn operation_action(value: OperationActionDto) -> Result<OperationAction, ApiError> {
    match value {
        OperationActionDto::Start => Ok(OperationAction::Start),
        OperationActionDto::Stop => Ok(OperationAction::Stop),
        OperationActionDto::Restart => Ok(OperationAction::Restart),
    }
}
