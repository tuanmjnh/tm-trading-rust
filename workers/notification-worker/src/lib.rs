use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use thiserror::Error;
use tm_trading_agent_protocol::{WORKER_SCHEMA_VERSION, WorkerEvidence, WorkerMessage};
use tm_trading_domain::{
    NotificationAttemptRecord, NotificationChannel, NotificationLogItem, NotificationOutboxItem,
    NotificationPriority, NotificationState, ROUTER_SCHEMA_VERSION, calculate_backoff_ms,
};
use tm_trading_storage::{
    NotificationLogRepository, NotificationOutboxRepository, RepositoryError,
};
use tm_trading_telegram_client::{TelegramClient, TelegramError};

/// Default claim delay between two outbox checks.
pub const DEFAULT_CLAIM_INTERVAL: Duration = Duration::from_secs(5);

/// Default lease duration for a claimed outbox item.
pub const DEFAULT_OUTBOX_LEASE: Duration = Duration::from_secs(60);

#[derive(Debug, Error)]
pub enum DrainError {
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error(transparent)]
    Telegram(#[from] TelegramError),
}

/// Result of draining one item from the outbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrainOutcome {
    pub item_id: String,
    pub channel: NotificationChannel,
    pub state: NotificationState,
    pub accepted: bool,
    pub retryable: bool,
    pub error: Option<String>,
}

/// Orchestrates outbox claim, transport dispatch, state update, and audit logging.
pub struct NotificationDrainer {
    outbox_repo: Arc<dyn NotificationOutboxRepository>,
    log_repo: Arc<dyn NotificationLogRepository>,
    telegram_client: Option<TelegramClient>,
    default_telegram_chat_id: Option<String>,
}

impl NotificationDrainer {
    pub fn new(
        outbox_repo: Arc<dyn NotificationOutboxRepository>,
        log_repo: Arc<dyn NotificationLogRepository>,
        telegram_client: Option<TelegramClient>,
        default_telegram_chat_id: Option<String>,
    ) -> Self {
        Self {
            outbox_repo,
            log_repo,
            telegram_client,
            default_telegram_chat_id,
        }
    }

    /// Formats notification title and body into an alert text string.
    #[must_use]
    pub fn format_message_text(item: &NotificationOutboxItem) -> String {
        let prefix = match item.priority {
            NotificationPriority::Critical => "[CRITICAL]",
            NotificationPriority::Important => "[IMPORTANT]",
            NotificationPriority::Normal => "[NORMAL]",
            NotificationPriority::Info => "[INFO]",
        };

        let mut text = String::new();
        text.push_str(prefix);

        if let Some(title) = &item.title {
            text.push(' ');
            text.push_str(title);
        } else if let Some(name) = &item.name {
            text.push(' ');
            text.push_str(name);
        } else {
            text.push(' ');
            text.push_str(&item.kind);
        }

        if let Some(symbol) = &item.symbol {
            text.push_str(&format!(" ({symbol})"));
        }

        if let Some(body) = &item.body {
            text.push_str("\n\n");
            text.push_str(body);
        }

        text
    }

    /// Claims and processes at most one due item from the outbox.
    pub async fn drain_one(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
        lease_duration: Duration,
    ) -> Result<Option<DrainOutcome>, DrainError> {
        let claimed = self
            .outbox_repo
            .claim_next(worker_id, now, lease_duration)
            .await?;

        let Some(item) = claimed else {
            return Ok(None);
        };

        match item.channel {
            NotificationChannel::Telegram => self.dispatch_telegram(item, now).await.map(Some),
            NotificationChannel::WebPush | NotificationChannel::Electron => {
                // Non-Telegram channels are unconfigured in initial Phase 8 scope
                let err_desc = format!("channel {} not supported in current release", item.channel);
                let updated = self
                    .outbox_repo
                    .mark_failed(&item.id, &err_desc, false, None, now)
                    .await?;

                let log_id = NotificationLogItem::build_id(&item.notify_id, now);
                let log_row = NotificationLogItem {
                    id: log_id,
                    notify_id: item.notify_id.clone(),
                    owner_id: item.owner_id.clone(),
                    dedupe_key: item.dedupe_key.clone(),
                    kind: item.kind.clone(),
                    priority: item.priority,
                    preset: item.preset.clone(),
                    name: item.name.clone(),
                    symbol: item.symbol.clone(),
                    title: item.title.clone(),
                    body: item.body.clone(),
                    at: item.at,
                    sent_at: now,
                    attempts: vec![NotificationAttemptRecord {
                        channel: item.channel,
                        ok: false,
                        error: Some(err_desc.clone()),
                    }],
                    delivered: vec![],
                    deduped: false,
                    router_version: ROUTER_SCHEMA_VERSION.to_owned(),
                };
                let _ = self.log_repo.record(log_row).await;

                Ok(Some(DrainOutcome {
                    item_id: item.id,
                    channel: item.channel,
                    state: updated.state,
                    accepted: false,
                    retryable: false,
                    error: Some(err_desc),
                }))
            }
        }
    }

    async fn dispatch_telegram(
        &self,
        item: NotificationOutboxItem,
        now: DateTime<Utc>,
    ) -> Result<DrainOutcome, DrainError> {
        let text = Self::format_message_text(&item);

        if let (Some(client), Some(chat_id)) =
            (&self.telegram_client, &self.default_telegram_chat_id)
        {
            let outcome = client.send_message(chat_id, &text).await?;

            let (updated_state, accepted) = if outcome.ok {
                if let Some(msg_id) = &outcome.message_id {
                    let updated = self
                        .outbox_repo
                        .mark_accepted(&item.id, msg_id, now)
                        .await?;
                    (updated.state, true)
                } else {
                    let updated = self.outbox_repo.mark_sent(&item.id, now).await?;
                    (updated.state, false)
                }
            } else {
                let err_desc = outcome
                    .description
                    .as_deref()
                    .unwrap_or("telegram delivery failed");
                let attempt = item.attempts + 1;
                let next_attempt_at = if outcome.retryable {
                    let backoff_ms = outcome.retry_after_ms.unwrap_or_else(|| {
                        calculate_backoff_ms(
                            attempt,
                            tm_trading_domain::DEFAULT_OUTBOX_BASE_MS,
                            tm_trading_domain::DEFAULT_OUTBOX_FACTOR,
                            tm_trading_domain::DEFAULT_OUTBOX_MAX_MS,
                            tm_trading_domain::DEFAULT_OUTBOX_JITTER_RATIO,
                            0.0,
                        )
                    });
                    Some(now + Duration::from_millis(backoff_ms))
                } else {
                    None
                };

                let updated = self
                    .outbox_repo
                    .mark_failed(&item.id, err_desc, outcome.retryable, next_attempt_at, now)
                    .await?;
                (updated.state, false)
            };

            // Record audit log
            let log_id = NotificationLogItem::build_id(&item.notify_id, now);
            let log_row = NotificationLogItem {
                id: log_id,
                notify_id: item.notify_id.clone(),
                owner_id: item.owner_id.clone(),
                dedupe_key: item.dedupe_key.clone(),
                kind: item.kind.clone(),
                priority: item.priority,
                preset: item.preset.clone(),
                name: item.name.clone(),
                symbol: item.symbol.clone(),
                title: item.title.clone(),
                body: item.body.clone(),
                at: item.at,
                sent_at: now,
                attempts: vec![NotificationAttemptRecord {
                    channel: NotificationChannel::Telegram,
                    ok: outcome.ok,
                    error: outcome.description.clone(),
                }],
                delivered: if outcome.ok {
                    vec![NotificationChannel::Telegram]
                } else {
                    vec![]
                },
                deduped: false,
                router_version: ROUTER_SCHEMA_VERSION.to_owned(),
            };
            let _ = self.log_repo.record(log_row).await;

            Ok(DrainOutcome {
                item_id: item.id,
                channel: NotificationChannel::Telegram,
                state: updated_state,
                accepted,
                retryable: outcome.retryable,
                error: outcome.description,
            })
        } else {
            // Telegram not configured
            let err_desc = "telegram client or chat ID not configured";
            let updated = self
                .outbox_repo
                .mark_failed(&item.id, err_desc, false, None, now)
                .await?;

            let log_id = NotificationLogItem::build_id(&item.notify_id, now);
            let log_row = NotificationLogItem {
                id: log_id,
                notify_id: item.notify_id.clone(),
                owner_id: item.owner_id.clone(),
                dedupe_key: item.dedupe_key.clone(),
                kind: item.kind.clone(),
                priority: item.priority,
                preset: item.preset.clone(),
                name: item.name.clone(),
                symbol: item.symbol.clone(),
                title: item.title.clone(),
                body: item.body.clone(),
                at: item.at,
                sent_at: now,
                attempts: vec![NotificationAttemptRecord {
                    channel: NotificationChannel::Telegram,
                    ok: false,
                    error: Some(err_desc.to_owned()),
                }],
                delivered: vec![],
                deduped: false,
                router_version: ROUTER_SCHEMA_VERSION.to_owned(),
            };
            let _ = self.log_repo.record(log_row).await;

            Ok(DrainOutcome {
                item_id: item.id,
                channel: NotificationChannel::Telegram,
                state: updated.state,
                accepted: false,
                retryable: false,
                error: Some(err_desc.to_owned()),
            })
        }
    }
}

/// Builds heartbeat and readiness evidence for the Host Agent.
#[must_use]
pub fn build_worker_evidence(
    last_success_at: Option<DateTime<Utc>>,
    last_output_count: usize,
    sink_writable: bool,
    telegram_configured: bool,
) -> WorkerEvidence {
    WorkerEvidence {
        last_success_at,
        last_output_count: Some(last_output_count as u64),
        sink_writable: Some(sink_writable),
        dependencies_reachable: Some(true),
        capability_verified: Some(telegram_configured),
        outbox_last_consumed_at: last_success_at,
        ..WorkerEvidence::default()
    }
}

/// Constructs the initial Hello handshake message.
#[must_use]
pub fn hello_message(
    instance_id: &str,
    at: DateTime<Utc>,
    pid: u32,
    executable_path: &str,
    executable_version: &str,
) -> WorkerMessage {
    WorkerMessage::Hello {
        instance_id: instance_id.to_owned(),
        at,
        pid,
        executable_path: executable_path.to_owned(),
        executable_version: executable_version.to_owned(),
        schema_version: WORKER_SCHEMA_VERSION.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tm_trading_storage::{
        InMemoryNotificationLogRepository, InMemoryNotificationOutboxRepository,
    };

    #[tokio::test]
    async fn drains_item_with_unconfigured_telegram_reports_terminal_failure() {
        let outbox = Arc::new(InMemoryNotificationOutboxRepository::new());
        let logs = Arc::new(InMemoryNotificationLogRepository::new());
        let drainer = NotificationDrainer::new(outbox.clone(), logs.clone(), None, None);

        let now = Utc::now();
        let item = NotificationOutboxItem::new(
            "evt_drain_01".to_owned(),
            NotificationChannel::Telegram,
            "signal".to_owned(),
            NotificationPriority::Critical,
            now,
            now,
        )
        .unwrap();

        outbox.enqueue(item.clone()).await.unwrap();

        let outcome = drainer
            .drain_one("worker-1", now, Duration::from_secs(30))
            .await
            .unwrap();

        assert!(outcome.is_some());
        let outcome = outcome.unwrap();
        assert_eq!(outcome.state, NotificationState::Failed);
        assert!(!outcome.retryable);

        let item_after = outbox.get(&item.id).await.unwrap().unwrap();
        assert!(item_after.dead);
        assert_eq!(item_after.state, NotificationState::Failed);
    }
}
