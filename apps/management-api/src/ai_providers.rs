use axum::Json;
use axum::extract::{Path, State};
use chrono::Utc;
use tm_trading_contracts::{
    AiProviderProfileDto, AiProviderProfileListDto, NewAiProviderProfileDto,
    UpdateAiProviderProfileDto,
};
use tm_trading_domain::{AiProviderKind, AiProviderProfile, NewAiProviderProfile};

use crate::auth::{CAPABILITY_AI_READ, CAPABILITY_AI_WRITE};
use crate::error::ApiError;
use crate::state::AppState;

pub async fn list_ai_provider_profiles(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
) -> Result<Json<AiProviderProfileListDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_READ)?;
    let profiles = state
        .ai_provider_profiles
        .list()
        .await
        .map_err(ApiError::from)?;
    let now = Utc::now();
    Ok(Json(AiProviderProfileListDto {
        generated_at: now,
        count: profiles.len(),
        profiles: profiles.iter().map(to_dto).collect(),
    }))
}

pub async fn create_ai_provider_profile(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Json(request): Json<NewAiProviderProfileDto>,
) -> Result<Json<AiProviderProfileDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_WRITE)?;
    let kind = AiProviderKind::parse(&request.kind)
        .ok_or_else(|| ApiError::UnprocessableEntity("unsupported ai provider kind".to_owned()))?;
    let profile = AiProviderProfile::new(NewAiProviderProfile {
        id: request.id,
        kind,
        display_name: request.display_name,
        base_url: request.base_url,
        default_model: request.default_model,
        timeout_ms: request.timeout_ms,
        api_key_env: request.api_key_env,
        created_at: Utc::now(),
    })
    .map_err(|error| ApiError::UnprocessableEntity(error.to_string()))?;
    state
        .ai_provider_profiles
        .upsert(&profile)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(to_dto(&profile)))
}

pub async fn get_ai_provider_profile(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Path(id): Path<String>,
) -> Result<Json<AiProviderProfileDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_READ)?;
    let profile = state
        .ai_provider_profiles
        .get(&id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("ai provider profile was not found".to_owned()))?;
    Ok(Json(to_dto(&profile)))
}

pub async fn update_ai_provider_profile(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Path(id): Path<String>,
    Json(request): Json<UpdateAiProviderProfileDto>,
) -> Result<Json<AiProviderProfileDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_WRITE)?;
    let existing = state
        .ai_provider_profiles
        .get(&id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("ai provider profile was not found".to_owned()))?;
    let updated = existing
        .clone()
        .with_settings(
            request
                .display_name
                .as_deref()
                .unwrap_or(existing.display_name()),
            request.base_url.as_deref().unwrap_or(existing.base_url()),
            request
                .default_model
                .as_deref()
                .unwrap_or(existing.default_model()),
            request.timeout_ms.unwrap_or(existing.timeout_ms()),
            request.enabled.unwrap_or(existing.enabled()),
            request
                .api_key_env
                .as_deref()
                .unwrap_or(existing.api_key_env()),
            Utc::now(),
        )
        .map_err(|error| ApiError::UnprocessableEntity(error.to_string()))?;
    state
        .ai_provider_profiles
        .upsert(&updated)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(to_dto(&updated)))
}

pub async fn delete_ai_provider_profile(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Path(id): Path<String>,
) -> Result<Json<()>, ApiError> {
    // Delete the profile and its models so no model references a missing provider.
    auth.require_capability(CAPABILITY_AI_WRITE)?;
    state
        .ai_provider_profiles
        .delete(&id)
        .await
        .map_err(ApiError::from)?;
    state
        .ai_models
        .delete_by_provider(&id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

pub(crate) fn to_dto(profile: &AiProviderProfile) -> AiProviderProfileDto {
    AiProviderProfileDto {
        id: profile.id().to_owned(),
        kind: profile.kind().as_str().to_owned(),
        display_name: profile.display_name().to_owned(),
        enabled: profile.enabled(),
        base_url: profile.base_url().to_owned(),
        default_model: profile.default_model().to_owned(),
        timeout_ms: profile.timeout_ms(),
        api_key_env: profile.api_key_env().to_owned(),
        health_state: profile.health_state().as_str().to_owned(),
        last_health_check_at: profile.last_health_check_at(),
        last_error_code: profile.last_error_code().map(str::to_owned),
        created_at: profile.created_at(),
        updated_at: profile.updated_at(),
    }
}

pub async fn test_ai_provider_profile(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Path(id): Path<String>,
) -> Result<Json<AiProviderProfileDto>, ApiError> {
    auth.require_capability(CAPABILITY_AI_WRITE)?;
    let profile = state
        .ai_provider_profiles
        .get(&id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| ApiError::NotFound("ai provider profile was not found".to_owned()))?;

    let observed_at = Utc::now();
    let result = run_health_test(&profile).await;
    let (healthy, error_code) = match result {
        Ok(_) => (true, None),
        Err(error) => (false, Some(error.code().to_owned())),
    };
    let updated = profile.with_health(observed_at, healthy, error_code.as_deref());
    state
        .ai_provider_profiles
        .upsert(&updated)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(to_dto(&updated)))
}

async fn run_health_test(
    profile: &AiProviderProfile,
) -> Result<tm_trading_ai_providers::AiProviderHealth, tm_trading_ai_providers::AiProviderError> {
    probe_model(profile, profile.default_model()).await
}

/// Runs the bounded provider-protocol probe with the given model identifier.
pub(crate) async fn probe_model(
    profile: &AiProviderProfile,
    model: &str,
) -> Result<tm_trading_ai_providers::AiProviderHealth, tm_trading_ai_providers::AiProviderError> {
    tm_trading_ai_providers::validate_ai_provider_base_url(profile.base_url())?;
    let api_key = std::env::var(profile.api_key_env())
        .map_err(|_| tm_trading_ai_providers::AiProviderError::MissingApiKey)?;
    let adapter = tm_trading_ai_providers::build_provider(
        profile.kind(),
        profile.base_url(),
        model,
        &api_key,
        std::time::Duration::from_millis(profile.timeout_ms()),
    )?;
    adapter.health().await
}
