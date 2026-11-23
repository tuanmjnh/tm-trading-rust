use axum::Json;
use axum::extract::{Query, State};
use chrono::Utc;
use tm_trading_contracts::{
    NotificationAttemptDto, NotificationChannelDto, NotificationLogItemDto, NotificationLogListDto,
    NotificationLogQueryDto, NotificationOutboxItemDto, NotificationOutboxListDto,
    NotificationOutboxQueryDto, NotificationOutboxSummaryDto, NotificationPriorityDto,
    NotificationStateDto, NotificationSummaryQueryDto, SendTestNotificationRequest,
    SendTestNotificationResponse,
};
use tm_trading_domain::{
    NotificationAttemptRecord, NotificationChannel, NotificationLogItem, NotificationOutboxItem,
    NotificationPriority, NotificationState,
};
use tm_trading_storage::{NotificationLogQuery, NotificationOutboxQuery};

use crate::auth::{AuthContext, CAPABILITY_NOTIFICATIONS_MANAGE, CAPABILITY_NOTIFICATIONS_READ};
use crate::error::ApiError;
use crate::state::AppState;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

/// Lists durable notification outbox entries matching query filters.
pub async fn list_outbox(
    State(state): State<AppState>,
    auth: AuthContext,
    Query(query): Query<NotificationOutboxQueryDto>,
) -> Result<Json<NotificationOutboxListDto>, ApiError> {
    auth.require_capability(CAPABILITY_NOTIFICATIONS_READ)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > MAX_LIMIT {
        return Err(ApiError::UnprocessableEntity(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }
    let storage_query = NotificationOutboxQuery {
        owner_id: clean_filter(query.owner_id),
        channel: query.channel.map(channel_from_dto),
        state: query.state.map(state_from_dto),
        dead: query.dead,
        limit,
    };
    let items = state
        .notification_outbox
        .list(&storage_query)
        .await
        .map_err(ApiError::from)?;
    let dtos = items.into_iter().map(outbox_to_dto).collect::<Vec<_>>();
    let count = dtos.len();
    Ok(Json(NotificationOutboxListDto {
        generated_at: Utc::now(),
        count,
        items: dtos,
    }))
}

/// Returns summary counts of the durable outbox queue.
pub async fn summary_outbox(
    State(state): State<AppState>,
    auth: AuthContext,
    Query(query): Query<NotificationSummaryQueryDto>,
) -> Result<Json<NotificationOutboxSummaryDto>, ApiError> {
    auth.require_capability(CAPABILITY_NOTIFICATIONS_READ)?;
    let owner_id = clean_filter(query.owner_id);
    let summary = state
        .notification_outbox
        .summary(owner_id.as_deref(), Utc::now())
        .await
        .map_err(ApiError::from)?;
    Ok(Json(summary))
}

/// Lists delivery audit logs matching query filters.
pub async fn list_logs(
    State(state): State<AppState>,
    auth: AuthContext,
    Query(query): Query<NotificationLogQueryDto>,
) -> Result<Json<NotificationLogListDto>, ApiError> {
    auth.require_capability(CAPABILITY_NOTIFICATIONS_READ)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > MAX_LIMIT {
        return Err(ApiError::UnprocessableEntity(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }
    let storage_query = NotificationLogQuery {
        owner_id: clean_filter(query.owner_id),
        kind: clean_filter(query.kind),
        priority: query.priority.map(priority_from_dto),
        limit,
    };
    let logs = state
        .notification_logs
        .list(&storage_query)
        .await
        .map_err(ApiError::from)?;
    let dtos = logs.into_iter().map(log_to_dto).collect::<Vec<_>>();
    let count = dtos.len();
    Ok(Json(NotificationLogListDto {
        generated_at: Utc::now(),
        count,
        logs: dtos,
    }))
}

/// Sends a test notification message through configured delivery channel.
pub async fn send_test_notification(
    State(state): State<AppState>,
    auth: AuthContext,
    Json(request): Json<SendTestNotificationRequest>,
) -> Result<Json<SendTestNotificationResponse>, ApiError> {
    auth.require_capability(CAPABILITY_NOTIFICATIONS_MANAGE)?;
    let now = Utc::now();
    let target_chat_id = request
        .chat_id
        .filter(|s| !s.trim().is_empty())
        .or_else(|| state.default_telegram_chat_id.clone());

    let target_chat_id = match target_chat_id {
        Some(chat_id) => chat_id,
        None => {
            return Ok(Json(SendTestNotificationResponse {
                ok: false,
                accepted: false,
                message_id: None,
                error: Some(
                    "No telegram chat_id provided in request and no default configured".to_owned(),
                ),
                sent_at: now,
            }));
        }
    };

    let client = match &state.telegram_client {
        Some(client) => client,
        None => {
            return Ok(Json(SendTestNotificationResponse {
                ok: false,
                accepted: false,
                message_id: None,
                error: Some(
                    "Telegram client is not configured (TM_TRADING_TELEGRAM_BOT_TOKEN unset)"
                        .to_owned(),
                ),
                sent_at: now,
            }));
        }
    };

    let text = request
        .message
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| {
            "🔔 [TM Trading] Test notification: verification ping successful.".to_owned()
        });

    match client.send_message(&target_chat_id, &text).await {
        Ok(outcome) => Ok(Json(SendTestNotificationResponse {
            ok: outcome.ok,
            accepted: outcome.accepted,
            message_id: outcome.message_id,
            error: outcome.description,
            sent_at: Utc::now(),
        })),
        Err(err) => Ok(Json(SendTestNotificationResponse {
            ok: false,
            accepted: false,
            message_id: None,
            error: Some(err.to_string()),
            sent_at: Utc::now(),
        })),
    }
}

fn clean_filter(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let trimmed = v.trim().to_owned();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn channel_to_dto(channel: NotificationChannel) -> NotificationChannelDto {
    match channel {
        NotificationChannel::Telegram => NotificationChannelDto::Telegram,
        NotificationChannel::WebPush => NotificationChannelDto::WebPush,
        NotificationChannel::Electron => NotificationChannelDto::Electron,
    }
}

fn channel_from_dto(dto: NotificationChannelDto) -> NotificationChannel {
    match dto {
        NotificationChannelDto::Telegram => NotificationChannel::Telegram,
        NotificationChannelDto::WebPush => NotificationChannel::WebPush,
        NotificationChannelDto::Electron => NotificationChannel::Electron,
    }
}

fn priority_to_dto(priority: NotificationPriority) -> NotificationPriorityDto {
    match priority {
        NotificationPriority::Info => NotificationPriorityDto::Info,
        NotificationPriority::Normal => NotificationPriorityDto::Normal,
        NotificationPriority::Important => NotificationPriorityDto::Important,
        NotificationPriority::Critical => NotificationPriorityDto::Critical,
    }
}

fn priority_from_dto(dto: NotificationPriorityDto) -> NotificationPriority {
    match dto {
        NotificationPriorityDto::Info => NotificationPriority::Info,
        NotificationPriorityDto::Normal => NotificationPriority::Normal,
        NotificationPriorityDto::Important => NotificationPriority::Important,
        NotificationPriorityDto::Critical => NotificationPriority::Critical,
    }
}

fn state_to_dto(state: NotificationState) -> NotificationStateDto {
    match state {
        NotificationState::Queued => NotificationStateDto::Queued,
        NotificationState::Sent => NotificationStateDto::Sent,
        NotificationState::Accepted => NotificationStateDto::Accepted,
        NotificationState::Failed => NotificationStateDto::Failed,
    }
}

fn state_from_dto(dto: NotificationStateDto) -> NotificationState {
    match dto {
        NotificationStateDto::Queued => NotificationState::Queued,
        NotificationStateDto::Sent => NotificationState::Sent,
        NotificationStateDto::Accepted => NotificationState::Accepted,
        NotificationStateDto::Failed => NotificationState::Failed,
    }
}

fn attempt_to_dto(attempt: NotificationAttemptRecord) -> NotificationAttemptDto {
    NotificationAttemptDto {
        channel: channel_to_dto(attempt.channel),
        ok: attempt.ok,
        error: attempt.error,
    }
}

fn outbox_to_dto(item: NotificationOutboxItem) -> NotificationOutboxItemDto {
    NotificationOutboxItemDto {
        id: item.id,
        notify_id: item.notify_id,
        dedupe_key: item.dedupe_key,
        channel: channel_to_dto(item.channel),
        owner_id: item.owner_id,
        category: item.category,
        kind: item.kind,
        priority: priority_to_dto(item.priority),
        preset: item.preset,
        name: item.name,
        symbol: item.symbol,
        title: item.title,
        body: item.body,
        at: item.at,
        state: state_to_dto(item.state),
        attempts: item.attempts,
        next_attempt_at: item.next_attempt_at,
        last_error: item.last_error,
        message_id: item.message_id,
        dead: item.dead,
        outcome_at: item.outcome_at,
        created_at: item.created_at,
        updated_at: item.updated_at,
    }
}

fn log_to_dto(item: NotificationLogItem) -> NotificationLogItemDto {
    NotificationLogItemDto {
        id: item.id,
        notify_id: item.notify_id,
        owner_id: item.owner_id,
        dedupe_key: item.dedupe_key,
        kind: item.kind,
        priority: priority_to_dto(item.priority),
        preset: item.preset,
        name: item.name,
        symbol: item.symbol,
        title: item.title,
        body: item.body,
        at: item.at,
        sent_at: item.sent_at,
        attempts: item.attempts.into_iter().map(attempt_to_dto).collect(),
        delivered: item.delivered.into_iter().map(channel_to_dto).collect(),
        deduped: item.deduped,
        router_version: item.router_version,
    }
}
