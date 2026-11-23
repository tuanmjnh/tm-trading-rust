use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use tm_trading_contracts::NotificationOutboxSummaryDto;
use tm_trading_domain::{
    DEFAULT_OUTBOX_MAX_ATTEMPTS, NotificationLogItem, NotificationOutboxItem, NotificationState,
};

use crate::notifications::{
    EnqueueNotificationOutcome, NotificationLogQuery, NotificationLogRepository,
    NotificationOutboxQuery, NotificationOutboxRepository, RecordNotificationLogOutcome,
};
use crate::RepositoryError;

type OutboxMap = HashMap<String, NotificationOutboxItem>;
type LeaseMap = HashMap<String, (String, DateTime<Utc>)>;
type LogMap = HashMap<String, NotificationLogItem>;

#[derive(Default)]
pub struct InMemoryNotificationOutboxRepository {
    items: Mutex<OutboxMap>,
    leases: Mutex<LeaseMap>,
}

impl InMemoryNotificationOutboxRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> Result<std::sync::MutexGuard<'_, OutboxMap>, RepositoryError> {
        self.items.lock().map_err(|_| RepositoryError::LockPoisoned)
    }

    fn leases_state(&self) -> Result<std::sync::MutexGuard<'_, LeaseMap>, RepositoryError> {
        self.leases
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)
    }
}

#[async_trait]
impl NotificationOutboxRepository for InMemoryNotificationOutboxRepository {
    async fn enqueue(
        &self,
        item: NotificationOutboxItem,
    ) -> Result<EnqueueNotificationOutcome, RepositoryError> {
        let mut state = self.state()?;
        if let Some(existing) = state.get(&item.id) {
            return Ok(EnqueueNotificationOutcome::Existing(existing.clone()));
        }
        state.insert(item.id.clone(), item.clone());
        Ok(EnqueueNotificationOutcome::Enqueued(item))
    }

    async fn get(&self, id: &str) -> Result<Option<NotificationOutboxItem>, RepositoryError> {
        let state = self.state()?;
        Ok(state.get(id).cloned())
    }

    async fn list(
        &self,
        query: &NotificationOutboxQuery,
    ) -> Result<Vec<NotificationOutboxItem>, RepositoryError> {
        let state = self.state()?;
        let mut matching: Vec<NotificationOutboxItem> = state
            .values()
            .filter(|item| {
                if let Some(owner) = &query.owner_id
                    && item.owner_id.as_deref() != Some(owner)
                {
                    return false;
                }
                if let Some(channel) = query.channel
                    && item.channel != channel
                {
                    return false;
                }
                if let Some(st) = query.state
                    && item.state != st
                {
                    return false;
                }
                if let Some(dead) = query.dead
                    && item.dead != dead
                {
                    return false;
                }
                true
            })
            .cloned()
            .collect();

        // Sort by next_attempt_at ascending, then created_at
        matching.sort_by(|a, b| {
            a.next_attempt_at
                .cmp(&b.next_attempt_at)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });
        if query.limit > 0 {
            matching.truncate(query.limit);
        }
        Ok(matching)
    }

    async fn summary(
        &self,
        owner_id: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<NotificationOutboxSummaryDto, RepositoryError> {
        let state = self.state()?;
        let mut queued = 0;
        let mut sent = 0;
        let mut accepted = 0;
        let mut failed = 0;
        let mut dead = 0;

        for item in state.values() {
            if let Some(owner) = owner_id
                && item.owner_id.as_deref() != Some(owner)
            {
                continue;
            }
            if item.dead {
                dead += 1;
            }
            match item.state {
                NotificationState::Queued => queued += 1,
                NotificationState::Sent => sent += 1,
                NotificationState::Accepted => accepted += 1,
                NotificationState::Failed => failed += 1,
            }
        }

        Ok(NotificationOutboxSummaryDto {
            queued,
            sent,
            accepted,
            failed,
            dead,
            generated_at: now,
        })
    }

    async fn claim_next(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
        lease_duration: Duration,
    ) -> Result<Option<NotificationOutboxItem>, RepositoryError> {
        let mut state = self.state()?;
        let mut leases = self.leases_state()?;

        // Clean expired leases
        leases.retain(|_, (_, expiry)| *expiry > now);

        // Find candidate: state == Queued, !dead, next_attempt_at <= now, and not currently leased
        let mut candidates: Vec<NotificationOutboxItem> = state
            .values()
            .filter(|item| {
                item.state == NotificationState::Queued
                    && !item.dead
                    && item.next_attempt_at.is_none_or(|t| t <= now)
                    && !leases.contains_key(&item.id)
            })
            .cloned()
            .collect();

        candidates.sort_by(|a, b| {
            a.next_attempt_at
                .cmp(&b.next_attempt_at)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });

        if let Some(chosen) = candidates.first() {
            let lease_expiry = now + lease_duration;
            leases.insert(chosen.id.clone(), (worker_id.to_owned(), lease_expiry));

            let item = state.get_mut(&chosen.id).unwrap();
            item.updated_at = now;
            Ok(Some(item.clone()))
        } else {
            Ok(None)
        }
    }

    async fn mark_accepted(
        &self,
        id: &str,
        message_id: &str,
        now: DateTime<Utc>,
    ) -> Result<NotificationOutboxItem, RepositoryError> {
        let mut state = self.state()?;
        let mut leases = self.leases_state()?;
        leases.remove(id);

        let item = state
            .get_mut(id)
            .ok_or(RepositoryError::NotificationOutboxNotFound)?;
        item.state = NotificationState::Accepted;
        item.message_id = Some(message_id.to_owned());
        item.outcome_at = Some(now);
        item.dead = false;
        item.updated_at = now;
        Ok(item.clone())
    }

    async fn mark_sent(
        &self,
        id: &str,
        now: DateTime<Utc>,
    ) -> Result<NotificationOutboxItem, RepositoryError> {
        let mut state = self.state()?;
        let mut leases = self.leases_state()?;
        leases.remove(id);

        let item = state
            .get_mut(id)
            .ok_or(RepositoryError::NotificationOutboxNotFound)?;
        item.state = NotificationState::Sent;
        item.outcome_at = Some(now);
        item.updated_at = now;
        Ok(item.clone())
    }

    async fn mark_failed(
        &self,
        id: &str,
        error: &str,
        retryable: bool,
        next_attempt_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<NotificationOutboxItem, RepositoryError> {
        let mut state = self.state()?;
        let mut leases = self.leases_state()?;
        leases.remove(id);

        let item = state
            .get_mut(id)
            .ok_or(RepositoryError::NotificationOutboxNotFound)?;
        item.attempts += 1;
        item.last_error = Some(error.to_owned());
        item.updated_at = now;

        if !retryable || item.attempts >= DEFAULT_OUTBOX_MAX_ATTEMPTS {
            item.state = NotificationState::Failed;
            item.dead = true;
            item.next_attempt_at = None;
            item.outcome_at = Some(now);
        } else {
            item.state = NotificationState::Queued;
            item.dead = false;
            item.next_attempt_at = next_attempt_at;
        }

        Ok(item.clone())
    }
}

#[derive(Default)]
pub struct InMemoryNotificationLogRepository {
    logs: Mutex<LogMap>,
}

impl InMemoryNotificationLogRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> Result<std::sync::MutexGuard<'_, LogMap>, RepositoryError> {
        self.logs.lock().map_err(|_| RepositoryError::LockPoisoned)
    }
}

#[async_trait]
impl NotificationLogRepository for InMemoryNotificationLogRepository {
    async fn record(
        &self,
        item: NotificationLogItem,
    ) -> Result<RecordNotificationLogOutcome, RepositoryError> {
        let mut state = self.state()?;
        if let Some(existing) = state.get(&item.id) {
            return Ok(RecordNotificationLogOutcome::Existing(existing.clone()));
        }
        state.insert(item.id.clone(), item.clone());
        Ok(RecordNotificationLogOutcome::Recorded(item))
    }

    async fn get(&self, id: &str) -> Result<Option<NotificationLogItem>, RepositoryError> {
        let state = self.state()?;
        Ok(state.get(id).cloned())
    }

    async fn list(
        &self,
        query: &NotificationLogQuery,
    ) -> Result<Vec<NotificationLogItem>, RepositoryError> {
        let state = self.state()?;
        let mut matching: Vec<NotificationLogItem> = state
            .values()
            .filter(|log| {
                if let Some(owner) = &query.owner_id
                    && log.owner_id.as_deref() != Some(owner)
                {
                    return false;
                }
                if let Some(kind) = &query.kind
                    && &log.kind != kind
                {
                    return false;
                }
                if let Some(priority) = query.priority
                    && log.priority != priority
                {
                    return false;
                }
                true
            })
            .cloned()
            .collect();

        // Sort by sent_at descending
        matching.sort_by_key(|a| std::cmp::Reverse(a.sent_at));
        if query.limit > 0 {
            matching.truncate(query.limit);
        }
        Ok(matching)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tm_trading_domain::{NotificationChannel, NotificationPriority};

    #[tokio::test]
    async fn enqueue_and_idempotency_roundtrip() {
        let repo = InMemoryNotificationOutboxRepository::new();
        let now = Utc::now();
        let item = NotificationOutboxItem::new(
            "evt_01".to_owned(),
            NotificationChannel::Telegram,
            "signal".to_owned(),
            NotificationPriority::Critical,
            now,
            now,
        )
        .unwrap();

        let outcome = repo.enqueue(item.clone()).await.unwrap();
        assert_eq!(outcome, EnqueueNotificationOutcome::Enqueued(item.clone()));

        // Second enqueue with same id returns Existing
        let outcome2 = repo.enqueue(item.clone()).await.unwrap();
        assert_eq!(outcome2, EnqueueNotificationOutcome::Existing(item));
    }

    #[tokio::test]
    async fn claim_and_state_transitions() {
        let repo = InMemoryNotificationOutboxRepository::new();
        let now = Utc::now();
        let item = NotificationOutboxItem::new(
            "evt_02".to_owned(),
            NotificationChannel::Telegram,
            "market".to_owned(),
            NotificationPriority::Important,
            now,
            now,
        )
        .unwrap();

        repo.enqueue(item.clone()).await.unwrap();

        // Claim
        let claimed = repo
            .claim_next("worker-1", now, Duration::from_secs(60))
            .await
            .unwrap();
        assert!(claimed.is_some());
        assert_eq!(claimed.unwrap().id, item.id);

        // Immediate next claim returns None (leased)
        let none = repo
            .claim_next("worker-2", now, Duration::from_secs(60))
            .await
            .unwrap();
        assert!(none.is_none());

        // Mark accepted
        let accepted = repo
            .mark_accepted(&item.id, "msg_999", now)
            .await
            .unwrap();
        assert_eq!(accepted.state, NotificationState::Accepted);
        assert_eq!(accepted.message_id.as_deref(), Some("msg_999"));
    }

    #[tokio::test]
    async fn record_logs_first_write_wins() {
        let repo = InMemoryNotificationLogRepository::new();
        let now = Utc::now();
        let id = NotificationLogItem::build_id("evt_03", now);
        let log_item = NotificationLogItem {
            id: id.clone(),
            notify_id: "evt_03".to_owned(),
            owner_id: Some("owner_1".to_owned()),
            dedupe_key: None,
            kind: "signal".to_owned(),
            priority: NotificationPriority::Normal,
            preset: None,
            name: None,
            symbol: Some("BTCUSDT".to_owned()),
            title: Some("VSA Signal".to_owned()),
            body: Some("Spring observed on volume".to_owned()),
            at: now,
            sent_at: now,
            attempts: vec![],
            delivered: vec![NotificationChannel::Telegram],
            deduped: false,
            router_version: "notify-router.v1".to_owned(),
        };

        let outcome = repo.record(log_item.clone()).await.unwrap();
        assert_eq!(outcome, RecordNotificationLogOutcome::Recorded(log_item.clone()));

        let outcome2 = repo.record(log_item.clone()).await.unwrap();
        assert_eq!(outcome2, RecordNotificationLogOutcome::Existing(log_item));
    }
}
