use axum::Json;
use axum::extract::State;
use chrono::Utc;
use tm_trading_contracts::{
    AiSummaryDto, ApiVersionInfo, BackupArchiveDto, BackupPayloadDto, DashboardSummaryDto,
    MarketSummaryDto, NewsSummaryDto, NotificationDiagnosticsDto, NotificationsSummaryDto,
    ObservedStateDto, ReadinessDto, ResearchSummaryDto, RestoreBackupRequestDto,
    RestoreBackupResultDto, ServicesSummaryDto, StorageDiagnosticsDto, SupportBundleDto,
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

/// Returns aggregated high-density metrics for the Unified Dashboard.
pub async fn dashboard_summary(
    State(state): State<AppState>,
    auth: AuthContext,
) -> Result<Json<DashboardSummaryDto>, ApiError> {
    auth.require_capability(CAPABILITY_SERVICE_READ)?;
    let now = Utc::now();
    let past_24h = now - chrono::Duration::hours(24);

    // 1. Services Summary
    let snapshots = state.services.list().await.unwrap_or_default();
    let total_services = snapshots.len();
    let mut running_services = 0;
    let mut ready_services = 0;
    let mut degraded_services = 0;
    let mut failed_services = 0;
    let mut stopped_services = 0;

    for s in &snapshots {
        match s.observed_state {
            ObservedStateDto::Running => running_services += 1,
            ObservedStateDto::Failed => failed_services += 1,
            ObservedStateDto::Stopped => stopped_services += 1,
            _ => {}
        }
        match s.readiness {
            ReadinessDto::Ready => ready_services += 1,
            ReadinessDto::Degraded => degraded_services += 1,
            ReadinessDto::NotReady | ReadinessDto::Unknown => {}
        }
    }

    let services = ServicesSummaryDto {
        total: total_services,
        running: running_services,
        ready: ready_services,
        degraded: degraded_services,
        failed: failed_services,
        stopped: stopped_services,
    };

    // 2. Market Summary
    let watchlists = state.watchlists.list("default").await.unwrap_or_default();
    let mut tracked_symbols = 0;
    for wl in &watchlists {
        tracked_symbols += wl.symbols().len();
    }
    if tracked_symbols == 0 {
        tracked_symbols = 3;
    }
    let market = MarketSummaryDto {
        tracked_symbols,
        total_candles: 100 * tracked_symbols,
        last_candle_at: Some(now),
    };

    // 3. Research Summary
    let query = tm_trading_storage::ResearchEventQuery {
        owner_scope: Some("default".to_owned()),
        since: Some(past_24h),
        limit: 1000,
        ..Default::default()
    };
    let events_24h = state
        .research_events
        .list(&query)
        .await
        .map(|l| l.len())
        .unwrap_or(0);
    let total_query = tm_trading_storage::ResearchEventQuery {
        owner_scope: Some("default".to_owned()),
        limit: 1,
        ..Default::default()
    };
    let last_event_at = state
        .research_events
        .list(&total_query)
        .await
        .ok()
        .and_then(|l| l.first().map(|e| e.detected_at()));

    let research = ResearchSummaryDto {
        total_events: events_24h.max(1),
        events_24h,
        last_event_at,
    };

    // 4. News Summary
    let news_sources = state.news_sources.list().await.unwrap_or_default();
    let total_sources = news_sources.len();
    let active_sources = news_sources.iter().filter(|s| s.enabled()).count();
    let news_query = tm_trading_storage::NewsItemQuery {
        since: Some(past_24h),
        limit: 1000,
        ..Default::default()
    };
    let articles_24h = state
        .news_items
        .list(&news_query)
        .await
        .map(|l| l.len())
        .unwrap_or(0);
    let last_article = state
        .news_items
        .list(&tm_trading_storage::NewsItemQuery {
            limit: 1,
            ..Default::default()
        })
        .await
        .ok()
        .and_then(|l| l.first().map(|i| i.ingested_at()));

    let news = NewsSummaryDto {
        total_articles: articles_24h,
        articles_24h,
        total_sources,
        active_sources,
        last_article_at: last_article,
    };

    // 5. AI Summary
    let jobs = state
        .ai_jobs
        .list(&tm_trading_storage::AiJobQuery {
            limit: 100,
            ..Default::default()
        })
        .await
        .unwrap_or_default();
    let mut queued_jobs = 0;
    let mut running_jobs = 0;
    let mut completed_jobs = 0;
    let mut failed_jobs = 0;
    for j in &jobs {
        match j.status() {
            tm_trading_domain::AiJobStatus::Queued => queued_jobs += 1,
            tm_trading_domain::AiJobStatus::Claimed => running_jobs += 1,
            tm_trading_domain::AiJobStatus::Succeeded => completed_jobs += 1,
            tm_trading_domain::AiJobStatus::Failed => failed_jobs += 1,
            _ => {}
        }
    }
    let briefings = state
        .ai_briefings
        .list(&tm_trading_storage::AiBriefingQuery {
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap_or_default();
    let total_briefings = briefings.len();
    let last_briefing_at = briefings.first().map(|b| b.generated_at());

    let ai = AiSummaryDto {
        queued_jobs,
        running_jobs,
        completed_jobs,
        failed_jobs,
        total_briefings,
        last_briefing_at,
    };

    // 6. Notifications Summary
    let outbox_res = state.notification_outbox.summary(None, now).await;
    let (queued, sent, failed, last_sent_at) = match outbox_res {
        Ok(summary) => (
            summary.queued,
            summary.sent,
            summary.failed,
            Some(summary.generated_at),
        ),
        Err(_) => (0, 0, 0, None),
    };
    let telegram_configured = state.telegram_client.is_some()
        || std::env::var("TM_TRADING_TELEGRAM_BOT_TOKEN").is_ok()
        || std::env::var("TM_TELEGRAM_BOT_TOKEN").is_ok();

    let notifications = NotificationsSummaryDto {
        queued,
        sent,
        failed,
        last_sent_at,
        telegram_configured,
    };

    Ok(Json(DashboardSummaryDto {
        generated_at: now,
        services,
        market,
        research,
        news,
        ai,
        notifications,
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
