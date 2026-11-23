use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Delivery channel for outbox and log contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum NotificationChannelDto {
    Telegram,
    WebPush,
    Electron,
}

/// Notification severity ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotificationPriorityDto {
    Info,
    Normal,
    Important,
    Critical,
}

/// Outbox item delivery state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotificationStateDto {
    Queued,
    Sent,
    Accepted,
    Failed,
}

/// Summary counts of the durable outbox state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NotificationOutboxSummaryDto {
    pub queued: usize,
    pub sent: usize,
    pub accepted: usize,
    pub failed: usize,
    pub dead: usize,
    pub generated_at: DateTime<Utc>,
}

/// One item in the durable outbox queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NotificationOutboxItemDto {
    pub id: String,
    pub notify_id: String,
    pub dedupe_key: Option<String>,
    pub channel: NotificationChannelDto,
    pub owner_id: Option<String>,
    pub category: Option<String>,
    pub kind: String,
    pub priority: NotificationPriorityDto,
    pub preset: Option<String>,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub at: DateTime<Utc>,
    pub state: NotificationStateDto,
    pub attempts: u32,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub message_id: Option<String>,
    pub dead: bool,
    pub outcome_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A bounded list of outbox items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NotificationOutboxListDto {
    pub generated_at: DateTime<Utc>,
    pub count: usize,
    pub items: Vec<NotificationOutboxItemDto>,
}

/// Single channel attempt outcome recorded in delivery logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NotificationAttemptDto {
    pub channel: NotificationChannelDto,
    pub ok: bool,
    pub error: Option<String>,
}

/// One immutable record in the notification delivery audit ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NotificationLogItemDto {
    pub id: String,
    pub notify_id: String,
    pub owner_id: Option<String>,
    pub dedupe_key: Option<String>,
    pub kind: String,
    pub priority: NotificationPriorityDto,
    pub preset: Option<String>,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub at: DateTime<Utc>,
    pub sent_at: DateTime<Utc>,
    pub attempts: Vec<NotificationAttemptDto>,
    pub delivered: Vec<NotificationChannelDto>,
    pub deduped: bool,
    pub router_version: String,
}

/// A bounded list of delivery log records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NotificationLogListDto {
    pub generated_at: DateTime<Utc>,
    pub count: usize,
    pub logs: Vec<NotificationLogItemDto>,
}

/// Optional filters for listing notification outbox entries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NotificationOutboxQueryDto {
    pub owner_id: Option<String>,
    pub channel: Option<NotificationChannelDto>,
    pub state: Option<NotificationStateDto>,
    pub dead: Option<bool>,
    pub limit: Option<usize>,
}

/// Optional filters for requesting notification outbox summary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NotificationSummaryQueryDto {
    pub owner_id: Option<String>,
}

/// Optional filters for listing delivery logs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct NotificationLogQueryDto {
    pub owner_id: Option<String>,
    pub kind: Option<String>,
    pub priority: Option<NotificationPriorityDto>,
    pub limit: Option<usize>,
}

/// Request to send a test notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SendTestNotificationRequest {
    /// Optional target chat ID override. If omitted, uses the configured default chat ID.
    pub chat_id: Option<String>,
    /// Optional custom text to send. If omitted, uses a standard verification test message.
    pub message: Option<String>,
}

/// Response from sending a test notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SendTestNotificationResponse {
    pub ok: bool,
    pub accepted: bool,
    pub message_id: Option<String>,
    pub error: Option<String>,
    pub sent_at: DateTime<Utc>,
}
