use axum::Json;
use axum::extract::State;
use chrono::Utc;
use tm_trading_contracts::{
    ApiVersionInfo, BackupArchiveDto, BackupPayloadDto, NotificationDiagnosticsDto,
    RestoreBackupRequestDto, RestoreBackupResultDto, StorageDiagnosticsDto, SupportBundleDto,
    SystemDiagnosticsDto, WatchlistDto, WorkerDiagnosticsDto,
};
use tm_trading_domain::{
    AiModel, AiProviderKind, AiProviderProfile, AiTaskKind, AiTaskRoute, NewAiModel,
    NewAiProviderProfile, NewAiTaskRoute,
};
use uuid::Uuid;

use crate::ai_models::to_dto as model_to_dto;
use crate::ai_providers::to_dto as provider_to_dto;
use crate::ai_task_routes::to_dto as route_to_dto;
use crate::auth::{AuthContext, CAPABILITY_SERVICE_CONTROL, CAPABILITY_SERVICE_READ};
use crate::error::ApiError;
use crate::state::AppState;

/// Returns a secret-redacted system diagnostics report.
pub async fn system_diagnostics(
    State(state): State<AppState>,
    auth: AuthContext,
) -> Result<Json<SystemDiagnosticsDto>, ApiError> {
    auth.require_capability(CAPABILITY_SERVICE_READ)?;
    let diagnostics = collect_diagnostics(&state).await?;
    Ok(Json(diagnostics))
}

/// Generates a diagnostic support bundle.
pub async fn create_support_bundle(
    State(state): State<AppState>,
    auth: AuthContext,
) -> Result<Json<SupportBundleDto>, ApiError> {
    auth.require_capability(CAPABILITY_SERVICE_READ)?;
    let now = Utc::now();
    let diagnostics = collect_diagnostics(&state).await?;
    let snapshot_count = diagnostics.workers.len();
    let bundle_id = format!("bundle_{}", Uuid::new_v4().simple());
    let redacted_summary = format!(
        "Support bundle {bundle_id} generated at {now}. Storage: {}, Workers: {snapshot_count}, Telegram: {}. Secrets and credentials strictly redacted.",
        diagnostics.storage.storage_kind,
        if diagnostics.notifications.telegram_configured {
            "configured"
        } else {
            "unconfigured"
        }
    );

    Ok(Json(SupportBundleDto {
        bundle_id,
        generated_at: now,
        diagnostics,
        snapshot_count,
        redacted_summary,
    }))
}

/// Exports system configuration snapshot into a signed/checksummed backup archive.
pub async fn export_backup(
    State(state): State<AppState>,
    auth: AuthContext,
) -> Result<Json<BackupArchiveDto>, ApiError> {
    auth.require_capability(CAPABILITY_SERVICE_READ)?;

    let watchlists_domain = state
        .watchlists
        .list(auth.subject())
        .await
        .map_err(ApiError::from)?;
    let watchlists: Vec<WatchlistDto> = watchlists_domain
        .into_iter()
        .map(WatchlistDto::from)
        .collect();

    let profiles_domain = state
        .ai_provider_profiles
        .list()
        .await
        .map_err(ApiError::from)?;
    let ai_provider_profiles = profiles_domain.iter().map(provider_to_dto).collect();

    let routes_domain = state.ai_task_routes.list().await.map_err(ApiError::from)?;
    let ai_task_routes = routes_domain.iter().map(route_to_dto).collect();

    let models_domain = state.ai_models.list().await.map_err(ApiError::from)?;
    let ai_models = models_domain.iter().map(model_to_dto).collect();

    let payload = BackupPayloadDto {
        watchlists,
        ai_provider_profiles,
        ai_models,
        ai_task_routes,
    };

    let archive = BackupArchiveDto::new(
        ApiVersionInfo::current(),
        state.storage_kind.clone(),
        payload,
    )
    .map_err(|e| ApiError::Internal(format!("failed to compute backup checksum: {e}")))?;

    Ok(Json(archive))
}

/// Validates the integrity and checksum of a backup archive without applying changes.
pub async fn verify_backup(
    State(_state): State<AppState>,
    auth: AuthContext,
    Json(archive): Json<BackupArchiveDto>,
) -> Result<Json<RestoreBackupResultDto>, ApiError> {
    auth.require_capability(CAPABILITY_SERVICE_READ)?;

    let checksum_verified = archive.verify_integrity();
    let message = if checksum_verified {
        format!(
            "Integrity verified. Backup contains {} watchlists, {} AI profiles, {} AI models, {} AI routes.",
            archive.payload.watchlists.len(),
            archive.payload.ai_provider_profiles.len(),
            archive.payload.ai_models.len(),
            archive.payload.ai_task_routes.len()
        )
    } else {
        "Backup integrity verification failed: SHA-256 checksum mismatch.".to_owned()
    };

    Ok(Json(RestoreBackupResultDto {
        restored_at: Utc::now(),
        checksum_verified,
        watchlists_restored: archive.payload.watchlists.len(),
        ai_provider_profiles_restored: archive.payload.ai_provider_profiles.len(),
        ai_models_restored: archive.payload.ai_models.len(),
        ai_task_routes_restored: archive.payload.ai_task_routes.len(),
        message,
    }))
}

/// Restores configurations from a verified backup archive.
pub async fn restore_backup(
    State(state): State<AppState>,
    auth: AuthContext,
    Json(request): Json<RestoreBackupRequestDto>,
) -> Result<Json<RestoreBackupResultDto>, ApiError> {
    auth.require_capability(CAPABILITY_SERVICE_CONTROL)?;

    if !request.archive.verify_integrity() {
        return Err(ApiError::BadRequest(
            "Backup archive failed integrity check: SHA-256 checksum mismatch".to_owned(),
        ));
    }

    let mut watchlists_restored = 0;
    let existing_watchlists = state
        .watchlists
        .list(auth.subject())
        .await
        .map_err(ApiError::from)?;

    for wl in request.archive.payload.watchlists {
        if let Some(existing) = existing_watchlists
            .iter()
            .find(|e| e.name() == wl.name || e.id() == wl.id)
        {
            if request.overwrite_existing {
                state
                    .watchlists
                    .replace_symbols(auth.subject(), existing.id(), &wl.symbols, Utc::now())
                    .await
                    .map_err(ApiError::from)?;
                watchlists_restored += 1;
            }
        } else {
            state
                .watchlists
                .create(auth.subject(), &wl.name, &wl.symbols, Utc::now())
                .await
                .map_err(ApiError::from)?;
            watchlists_restored += 1;
        }
    }

    // 1. Restore AI Provider Profiles first so model references resolve
    let mut profiles_restored = 0;
    for p in request.archive.payload.ai_provider_profiles {
        if let Some(kind) = AiProviderKind::parse(&p.kind)
            && let Ok(profile) = AiProviderProfile::new(NewAiProviderProfile {
                id: p.id,
                kind,
                display_name: p.display_name,
                base_url: p.base_url,
                default_model: p.default_model,
                timeout_ms: p.timeout_ms,
                api_key_env: p.api_key_env,
                created_at: Utc::now(),
            })
        {
            state
                .ai_provider_profiles
                .upsert(&profile)
                .await
                .map_err(ApiError::from)?;
            profiles_restored += 1;
        }
    }

    // 2. Restore AI Models second
    let mut models_restored = 0;
    for m in request.archive.payload.ai_models {
        if let Ok(model) = AiModel::new(NewAiModel {
            id: m.id,
            provider_id: m.provider_id,
            model_id: m.model_id,
            display_name: m.display_name,
            context_window: m.context_window,
            max_output_tokens: m.max_output_tokens,
            supports_streaming: m.supports_streaming,
            supports_json_mode: m.supports_json_mode,
            supports_tools: m.supports_tools,
            supports_vision: m.supports_vision,
            created_at: Utc::now(),
        }) {
            state
                .ai_models
                .upsert(&model)
                .await
                .map_err(ApiError::from)?;
            models_restored += 1;
        }
    }

    // 3. Restore AI Task Routes
    let mut routes_restored = 0;
    for r in request.archive.payload.ai_task_routes {
        if let Some(task) = AiTaskKind::parse(&r.task)
            && let Ok(route) = AiTaskRoute::new(NewAiTaskRoute {
                task,
                primary: r.primary,
                fallbacks: r.fallbacks,
                timeout_ms: r.timeout_ms,
                max_attempts: r.max_attempts,
                concurrency: r.concurrency,
                max_output_tokens: r.max_output_tokens,
                created_at: Utc::now(),
            })
        {
            state
                .ai_task_routes
                .upsert(&route)
                .await
                .map_err(ApiError::from)?;
            routes_restored += 1;
        }
    }

    let message = format!(
        "Restore completed successfully: {watchlists_restored} watchlists, {profiles_restored} AI profiles, {models_restored} AI models, and {routes_restored} AI task routes restored."
    );

    Ok(Json(RestoreBackupResultDto {
        restored_at: Utc::now(),
        checksum_verified: true,
        watchlists_restored,
        ai_provider_profiles_restored: profiles_restored,
        ai_models_restored: models_restored,
        ai_task_routes_restored: routes_restored,
        message,
    }))
}

async fn collect_diagnostics(state: &AppState) -> Result<SystemDiagnosticsDto, ApiError> {
    let now = Utc::now();
    let uptime_seconds = (now - state.start_time).num_seconds().max(0) as u64;

    let snapshots = state.services.list().await.map_err(ApiError::from)?;
    let workers = snapshots
        .into_iter()
        .map(|s| WorkerDiagnosticsDto {
            service_instance_id: s.service_instance_id,
            observed_state: s.observed_state.as_str().to_owned(),
            readiness: s.readiness.as_str().to_owned(),
            process_alive: s.process_alive.unwrap_or(false),
            restart_count: s.restart_count,
            last_heartbeat_at: s.last_heartbeat_at,
        })
        .collect();

    let outbox_summary = state
        .notification_outbox
        .summary(None, now)
        .await
        .map_err(ApiError::from)?;

    Ok(SystemDiagnosticsDto {
        generated_at: now,
        version: ApiVersionInfo::current(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        uptime_seconds,
        storage: StorageDiagnosticsDto {
            storage_kind: state.storage_kind.clone(),
            connected: true,
            database_name: state.database_name.clone(),
        },
        workers,
        notifications: NotificationDiagnosticsDto {
            telegram_configured: state.telegram_client.is_some(),
            outbox_queued: outbox_summary.queued,
            outbox_dead: outbox_summary.dead,
        },
    })
}
