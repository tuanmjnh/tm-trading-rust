use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use tm_trading_contracts::NotificationOutboxSummaryDto;
use tm_trading_domain::{
    NotificationChannel, NotificationLogItem, NotificationOutboxItem, NotificationPriority,
    NotificationState,
};

use crate::RepositoryError;

/// Default limit for notification queries.
pub const DEFAULT_NOTIFICATION_QUERY_LIMIT: usize = 50;

/// Query filter for durable outbox entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationOutboxQuery {
    pub owner_id: Option<String>,
    pub channel: Option<NotificationChannel>,
    pub state: Option<NotificationState>,
    pub dead: Option<bool>,
    pub limit: usize,
}

impl Default for NotificationOutboxQuery {
    fn default() -> Self {
        Self {
            owner_id: None,
            channel: None,
            state: None,
            dead: None,
            limit: DEFAULT_NOTIFICATION_QUERY_LIMIT,
        }
    }
}

/// Outcome of enqueueing an item in the outbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueNotificationOutcome {
    Enqueued(NotificationOutboxItem),
    Existing(NotificationOutboxItem),
}

/// Durable outbox repository.
#[async_trait]
pub trait NotificationOutboxRepository: Send + Sync {
    /// Inserts a new outbox entry if `${notify_id}:${channel}` does not already exist.
    async fn enqueue(
        &self,
        item: NotificationOutboxItem,
    ) -> Result<EnqueueNotificationOutcome, RepositoryError>;

    /// Gets an outbox entry by its canonical `${notify_id}:${channel}` ID.
    async fn get(&self, id: &str) -> Result<Option<NotificationOutboxItem>, RepositoryError>;

    /// Lists outbox entries matching the filter, ordered by next attempt or creation time.
    async fn list(
        &self,
        query: &NotificationOutboxQuery,
    ) -> Result<Vec<NotificationOutboxItem>, RepositoryError>;

    /// Aggregates state and dead-letter counts for an owner or system.
    async fn summary(
        &self,
        owner_id: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<NotificationOutboxSummaryDto, RepositoryError>;

    /// Claims the next due, un-dead queued item for dispatch.
    async fn claim_next(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
        lease_duration: Duration,
    ) -> Result<Option<NotificationOutboxItem>, RepositoryError>;

    /// Marks the item accepted with an explicit provider message ID.
    async fn mark_accepted(
        &self,
        id: &str,
        message_id: &str,
        now: DateTime<Utc>,
    ) -> Result<NotificationOutboxItem, RepositoryError>;

    /// Marks the item sent without an external message ID.
    async fn mark_sent(
        &self,
        id: &str,
        now: DateTime<Utc>,
    ) -> Result<NotificationOutboxItem, RepositoryError>;

    /// Folds a failure into retry state or dead-letter state.
    async fn mark_failed(
        &self,
        id: &str,
        error: &str,
        retryable: bool,
        next_attempt_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<NotificationOutboxItem, RepositoryError>;
}

/// Query filter for immutable delivery logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationLogQuery {
    pub owner_id: Option<String>,
    pub kind: Option<String>,
    pub priority: Option<NotificationPriority>,
    pub limit: usize,
}

impl Default for NotificationLogQuery {
    fn default() -> Self {
        Self {
            owner_id: None,
            kind: None,
            priority: None,
            limit: DEFAULT_NOTIFICATION_QUERY_LIMIT,
        }
    }
}

/// Outcome of recording a delivery log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordNotificationLogOutcome {
    Recorded(NotificationLogItem),
    Existing(NotificationLogItem),
}

/// Immutable delivery audit ledger repository.
#[async_trait]
pub trait NotificationLogRepository: Send + Sync {
    /// Appends a new delivery log row (first-write-wins on `${notify_id}:${at}`).
    async fn record(
        &self,
        item: NotificationLogItem,
    ) -> Result<RecordNotificationLogOutcome, RepositoryError>;

    /// Gets a log row by its canonical `${notify_id}:${at}` ID.
    async fn get(&self, id: &str) -> Result<Option<NotificationLogItem>, RepositoryError>;

    /// Lists delivery log rows, newest first.
    async fn list(
        &self,
        query: &NotificationLogQuery,
    ) -> Result<Vec<NotificationLogItem>, RepositoryError>;
}
