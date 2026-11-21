use axum::Json;
use axum::extract::{Path, State};
use chrono::Utc;
use tm_trading_contracts::{
    AiTaskRouteDto, AiTaskRouteListDto, NewAiTaskRouteDto, UpdateAiTaskRouteDto,
};
use tm_trading_domain::{AiTaskKind, AiTaskModelRef, AiTaskRoute, NewAiTaskRoute};

use crate::auth::{CAPABILITY_AI_READ, CAPABILITY_AI_WRITE};
use crate::error::ApiError;
use crate::state::AppState;

pub async fn list_ai_task_routes(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
) -> Result<Json<AiTaskRouteListDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_READ)?;
    let routes = state.ai_task_routes.list().await.map_err(ApiError::from)?;
    Ok(Json(AiTaskRouteListDto {
        generated_at: Utc::now(),
        count: routes.len(),
        routes: routes.iter().map(to_dto).collect(),
    }))
}

pub async fn get_ai_task_route(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Path(task): Path<String>,
) -> Result<Json<AiTaskRouteDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_READ)?;
    let route = load(&state, &task).await?;
    Ok(Json(to_dto(&route)))
}

pub async fn create_ai_task_route(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Json(request): Json<NewAiTaskRouteDto>,
) -> Result<Json<AiTaskRouteDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_WRITE)?;
    let task = AiTaskKind::parse(&request.task)
        .ok_or_else(|| ApiError::UnprocessableEntity("unsupported ai task".to_owned()))?;
    let route = AiTaskRoute::new(NewAiTaskRoute {
        task,
        primary: request.primary,
        fallbacks: request.fallbacks,
        timeout_ms: request.timeout_ms,
        max_attempts: request.max_attempts,
        concurrency: request.concurrency,
        max_output_tokens: request.max_output_tokens,
        created_at: Utc::now(),
    })
    .map_err(|error| ApiError::UnprocessableEntity(error.to_string()))?;
    // A route that names a model the registry does not have could never serve a request.
    validate_models(&state, &route).await?;
    state
        .ai_task_routes
        .upsert(&route)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(to_dto(&route)))
}

pub async fn update_ai_task_route(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Path(task): Path<String>,
    Json(request): Json<UpdateAiTaskRouteDto>,
) -> Result<Json<AiTaskRouteDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_WRITE)?;
    let existing = load(&state, &task).await?;
    let updated = existing
        .clone()
        .with_settings(
            request
                .primary
                .unwrap_or_else(|| existing.primary().to_owned()),
            request
                .fallbacks
                .unwrap_or_else(|| existing.fallbacks().to_vec()),
            request.enabled.unwrap_or(existing.enabled()),
            request.timeout_ms.unwrap_or(existing.timeout_ms()),
            request.max_attempts.unwrap_or(existing.max_attempts()),
            request.concurrency.unwrap_or(existing.concurrency()),
            request.max_output_tokens.or(existing.max_output_tokens()),
            Utc::now(),
        )
        .map_err(|error| ApiError::UnprocessableEntity(error.to_string()))?;
    validate_models(&state, &updated).await?;
    state
        .ai_task_routes
        .upsert(&updated)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(to_dto(&updated)))
}

pub async fn delete_ai_task_route(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Path(task): Path<String>,
) -> Result<Json<()>, ApiError> {
    auth.require_capability(CAPABILITY_AI_WRITE)?;
    state
        .ai_task_routes
        .delete(&task)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn load(state: &AppState, task: &str) -> Result<AiTaskRoute, ApiError> {
    state
        .ai_task_routes
        .get(task)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("ai task route was not found".to_owned()))
}

/// Every provider entry must name a registered model of an existing provider profile.
async fn validate_models(state: &AppState, route: &AiTaskRoute) -> Result<(), ApiError> {
    for entry in route.chain() {
        let reference = AiTaskModelRef::parse(entry)
            .map_err(|error| ApiError::UnprocessableEntity(error.to_string()))?;
        let provider = state
            .ai_provider_profiles
            .get(reference.provider_id())
            .await
            .map_err(ApiError::from)?
            .ok_or_else(|| {
                ApiError::UnprocessableEntity("ai provider profile was not found".to_owned())
            })?;
        let model = state
            .ai_models
            .get(reference.model_registry_id())
            .await
            .map_err(ApiError::from)?
            .ok_or_else(|| ApiError::UnprocessableEntity("ai model was not found".to_owned()))?;
        if model.provider_id() != provider.id() {
            return Err(ApiError::UnprocessableEntity(
                "ai model does not belong to the named provider".to_owned(),
            ));
        }
    }
    Ok(())
}

fn to_dto(route: &AiTaskRoute) -> AiTaskRouteDto {
    AiTaskRouteDto {
        task: route.task().as_str().to_owned(),
        primary: route.primary().to_owned(),
        fallbacks: route.fallbacks().to_vec(),
        enabled: route.enabled(),
        timeout_ms: route.timeout_ms(),
        max_attempts: route.max_attempts(),
        concurrency: route.concurrency(),
        max_output_tokens: route.max_output_tokens(),
        providers_in_reach: route.providers_in_reach(),
        created_at: route.created_at(),
        updated_at: route.updated_at(),
    }
}
