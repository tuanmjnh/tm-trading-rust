use axum::Json;
use axum::extract::{Path, State};
use chrono::Utc;
use tm_trading_contracts::{AiModelDto, AiModelListDto, NewAiModelDto, UpdateAiModelDto};
use tm_trading_domain::{AiModel, NewAiModel};

use crate::auth::{CAPABILITY_AI_READ, CAPABILITY_AI_WRITE};
use crate::error::ApiError;
use crate::state::AppState;

pub async fn list_ai_models(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
) -> Result<Json<AiModelListDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_READ)?;
    let models = state.ai_models.list().await.map_err(ApiError::from)?;
    Ok(Json(AiModelListDto {
        generated_at: Utc::now(),
        count: models.len(),
        models: models.iter().map(to_dto).collect(),
    }))
}

pub async fn create_ai_model(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Path(provider_id): Path<String>,
    Json(request): Json<NewAiModelDto>,
) -> Result<Json<AiModelDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_WRITE)?;
    // A model without its provider would be unreachable, so the provider must exist first.
    state
        .ai_provider_profiles
        .get(&provider_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| {
            ApiError::UnprocessableEntity("ai provider profile was not found".to_owned())
        })?;
    let model = AiModel::new(NewAiModel {
        id: request.id,
        provider_id,
        model_id: request.model_id,
        display_name: request.display_name,
        context_window: request.context_window,
        max_output_tokens: request.max_output_tokens,
        supports_streaming: request.supports_streaming,
        supports_json_mode: request.supports_json_mode,
        supports_tools: request.supports_tools,
        supports_vision: request.supports_vision,
        created_at: Utc::now(),
    })
    .map_err(|error| ApiError::UnprocessableEntity(error.to_string()))?;
    state
        .ai_models
        .upsert(&model)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(to_dto(&model)))
}

pub async fn get_ai_model(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Path(id): Path<String>,
) -> Result<Json<AiModelDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_READ)?;
    Ok(Json(to_dto(&load(&state, &id).await?)))
}

pub async fn update_ai_model(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Path(id): Path<String>,
    Json(request): Json<UpdateAiModelDto>,
) -> Result<Json<AiModelDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_WRITE)?;
    let existing = load(&state, &id).await?;
    let updated = existing
        .clone()
        .with_settings(
            request
                .display_name
                .as_deref()
                .unwrap_or(existing.display_name()),
            request.context_window.or(existing.context_window()),
            request.max_output_tokens.or(existing.max_output_tokens()),
            request
                .supports_streaming
                .unwrap_or(existing.supports_streaming()),
            request
                .supports_json_mode
                .unwrap_or(existing.supports_json_mode()),
            request.supports_tools.unwrap_or(existing.supports_tools()),
            request
                .supports_vision
                .unwrap_or(existing.supports_vision()),
            request.enabled.unwrap_or(existing.enabled()),
            Utc::now(),
        )
        .map_err(|error| ApiError::UnprocessableEntity(error.to_string()))?;
    state
        .ai_models
        .upsert(&updated)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(to_dto(&updated)))
}

pub async fn delete_ai_model(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Path(id): Path<String>,
) -> Result<Json<()>, ApiError> {
    auth.require_capability(CAPABILITY_AI_WRITE)?;
    state.ai_models.delete(&id).await.map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn load(state: &AppState, id: &str) -> Result<AiModel, ApiError> {
    state
        .ai_models
        .get(id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("ai model was not found".to_owned()))
}

fn to_dto(model: &AiModel) -> AiModelDto {
    AiModelDto {
        id: model.id().to_owned(),
        provider_id: model.provider_id().to_owned(),
        model_id: model.model_id().to_owned(),
        display_name: model.display_name().to_owned(),
        context_window: model.context_window(),
        max_output_tokens: model.max_output_tokens(),
        supports_streaming: model.supports_streaming(),
        supports_json_mode: model.supports_json_mode(),
        supports_tools: model.supports_tools(),
        supports_vision: model.supports_vision(),
        enabled: model.enabled(),
        health_state: model.health_state().as_str().to_owned(),
        last_health_check_at: model.last_health_check_at(),
        last_error_code: model.last_error_code().map(str::to_owned),
        created_at: model.created_at(),
        updated_at: model.updated_at(),
    }
}
