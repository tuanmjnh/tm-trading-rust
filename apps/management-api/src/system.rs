use axum::Json;
use axum::extract::State;
use chrono::Utc;
use tm_trading_contracts::{
    ApiVersionInfo, NotificationDiagnosticsDto, StorageDiagnosticsDto, SupportBundleDto,
    SystemDiagnosticsDto, WorkerDiagnosticsDto,
};
use uuid::Uuid;

use crate::auth::{AuthContext, CAPABILITY_SERVICE_READ};
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
