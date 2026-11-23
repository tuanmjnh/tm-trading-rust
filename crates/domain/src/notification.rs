use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Schema version for outbox queue documents.
pub const OUTBOX_SCHEMA_VERSION: &str = "notify-outbox.v1";

/// Schema version for delivery audit logs.
pub const ROUTER_SCHEMA_VERSION: &str = "notify-router.v1";

/// Default outbox exponential backoff base in milliseconds (30 seconds).
pub const DEFAULT_OUTBOX_BASE_MS: u64 = 30_000;

/// Default multiplier for exponential backoff.
pub const DEFAULT_OUTBOX_FACTOR: u64 = 2;

/// Default maximum backoff cap in milliseconds (1 hour).
pub const DEFAULT_OUTBOX_MAX_MS: u64 = 3_600_000;

/// Default maximum attempts before moving an item to dead-letter state.
pub const DEFAULT_OUTBOX_MAX_ATTEMPTS: u32 = 6;

/// Default jitter ratio (±20%).
pub const DEFAULT_OUTBOX_JITTER_RATIO: f64 = 0.2;

/// Maximum title length in characters.
pub const MAX_NOTIFICATION_TITLE_CHARS: usize = 500;

/// Maximum body length in characters.
pub const MAX_NOTIFICATION_BODY_CHARS: usize = 4_000;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NotificationError {
    #[error("unknown notification channel: {0}")]
    UnknownChannel(String),

    #[error("unknown notification priority: {0}")]
    UnknownPriority(String),

    #[error("unknown notification state: {0}")]
    UnknownState(String),

    #[error("notify_id cannot be empty")]
    EmptyNotifyId,

    #[error("kind cannot be empty")]
    EmptyKind,

    #[error("title exceeds maximum allowed length of {0} chars")]
    TitleTooLong(usize),

    #[error("body exceeds maximum allowed length of {0} chars")]
    BodyTooLong(usize),
}

/// Destination delivery channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NotificationChannel {
    Telegram,
    WebPush,
    Electron,
}

impl NotificationChannel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::WebPush => "webPush",
            Self::Electron => "electron",
        }
    }
}

impl fmt::Display for NotificationChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for NotificationChannel {
    type Err = NotificationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "telegram" => Ok(Self::Telegram),
            "webPush" | "web_push" => Ok(Self::WebPush),
            "electron" => Ok(Self::Electron),
            _ => Err(NotificationError::UnknownChannel(s.to_owned())),
        }
    }
}

/// Severity ladder for notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum NotificationPriority {
    Info,
    Normal,
    Important,
    Critical,
}

impl NotificationPriority {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Normal => "normal",
            Self::Important => "important",
            Self::Critical => "critical",
        }
    }
}

impl fmt::Display for NotificationPriority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for NotificationPriority {
    type Err = NotificationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "info" => Ok(Self::Info),
            "normal" => Ok(Self::Normal),
            "important" => Ok(Self::Important),
            "critical" => Ok(Self::Critical),
            _ => Err(NotificationError::UnknownPriority(s.to_owned())),
        }
    }
}

/// State of a notification delivery attempt in the durable outbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationState {
    /// Waiting to be claimed and dispatched.
    Queued,
    /// Dispatched successfully, but without an external message id acknowledgment.
    Sent,
    /// Confirmed accepted by the downstream provider with an explicit message id.
    Accepted,
    /// Terminal failure after retry budget exhausted or permanent error.
    Failed,
}

impl NotificationState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Sent => "sent",
            Self::Accepted => "accepted",
            Self::Failed => "failed",
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Accepted | Self::Failed)
    }
}

impl fmt::Display for NotificationState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for NotificationState {
    type Err = NotificationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "queued" => Ok(Self::Queued),
            "sent" => Ok(Self::Sent),
            "accepted" => Ok(Self::Accepted),
            "failed" => Ok(Self::Failed),
            _ => Err(NotificationError::UnknownState(s.to_owned())),
        }
    }
}

/// Pure calculation of retry backoff delay with bounded jitter.
///
/// `attempt` is 1-based (the failed attempt count).
/// `jitter_fraction` is in range `[-1.0, 1.0]`, allowing deterministic testing.
#[must_use]
pub fn calculate_backoff_ms(
    attempt: u32,
    base_ms: u64,
    factor: u64,
    max_ms: u64,
    jitter_ratio: f64,
    jitter_fraction: f64,
) -> u64 {
    let n = attempt.max(1);
    let exponent = n.saturating_sub(1);
    let raw = (base_ms as f64) * (factor as f64).powi(exponent as i32);
    let capped = raw.clamp(0.0, max_ms as f64);
    let spread = capped * jitter_ratio.clamp(0.0, 1.0);
    let offset = jitter_fraction.clamp(-1.0, 1.0) * spread;
    (capped + offset).clamp(0.0, max_ms as f64).round() as u64
}

/// Durable outbox entry representing a single message targeted to a specific channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationOutboxItem {
    pub id: String,
    pub outbox_version: String,
    pub notify_id: String,
    pub dedupe_key: Option<String>,
    pub channel: NotificationChannel,
    pub owner_id: Option<String>,
    pub category: Option<String>,
    pub kind: String,
    pub priority: NotificationPriority,
    pub preset: Option<String>,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub at: DateTime<Utc>,
    pub state: NotificationState,
    pub attempts: u32,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub message_id: Option<String>,
    pub dead: bool,
    pub outcome_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl NotificationOutboxItem {
    /// Constructs the canonical outbox row identifier `${notify_id}:${channel}`.
    #[must_use]
    pub fn build_id(notify_id: &str, channel: NotificationChannel) -> String {
        format!("{notify_id}:{}", channel.as_str())
    }

    /// Creates a new queued outbox entry.
    pub fn new(
        notify_id: String,
        channel: NotificationChannel,
        kind: String,
        priority: NotificationPriority,
        at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Self, NotificationError> {
        if notify_id.trim().is_empty() {
            return Err(NotificationError::EmptyNotifyId);
        }
        if kind.trim().is_empty() {
            return Err(NotificationError::EmptyKind);
        }

        let id = Self::build_id(&notify_id, channel);
        Ok(Self {
            id,
            outbox_version: OUTBOX_SCHEMA_VERSION.to_owned(),
            notify_id,
            dedupe_key: None,
            channel,
            owner_id: None,
            category: None,
            kind,
            priority,
            preset: None,
            name: None,
            symbol: None,
            title: None,
            body: None,
            at,
            state: NotificationState::Queued,
            attempts: 0,
            next_attempt_at: Some(at),
            last_error: None,
            message_id: None,
            dead: false,
            outcome_at: None,
            created_at: now,
            updated_at: now,
        })
    }
}

/// An individual delivery attempt outcome per channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationAttemptRecord {
    pub channel: NotificationChannel,
    pub ok: bool,
    pub error: Option<String>,
}

/// Immutable delivery audit ledger row recording routing and channel outcomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationLogItem {
    pub id: String,
    pub notify_id: String,
    pub owner_id: Option<String>,
    pub dedupe_key: Option<String>,
    pub kind: String,
    pub priority: NotificationPriority,
    pub preset: Option<String>,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub at: DateTime<Utc>,
    pub sent_at: DateTime<Utc>,
    pub attempts: Vec<NotificationAttemptRecord>,
    pub delivered: Vec<NotificationChannel>,
    pub deduped: bool,
    pub router_version: String,
}

impl NotificationLogItem {
    /// Constructs the canonical ledger identifier `${notify_id}:${at_ms}`.
    #[must_use]
    pub fn build_id(notify_id: &str, at: DateTime<Utc>) -> String {
        format!("{notify_id}:{}", at.timestamp_millis())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_channel_priority_and_state_roundtrip() {
        assert_eq!(
            "telegram".parse::<NotificationChannel>().unwrap(),
            NotificationChannel::Telegram
        );
        assert_eq!(NotificationChannel::Telegram.to_string(), "telegram");

        assert_eq!(
            "critical".parse::<NotificationPriority>().unwrap(),
            NotificationPriority::Critical
        );
        assert_eq!(NotificationPriority::Critical.to_string(), "critical");

        assert_eq!(
            "queued".parse::<NotificationState>().unwrap(),
            NotificationState::Queued
        );
        assert_eq!(NotificationState::Accepted.to_string(), "accepted");
    }

    #[test]
    fn outbox_id_format_matches_legacy_specification() {
        let id = NotificationOutboxItem::build_id("evt_12345", NotificationChannel::Telegram);
        assert_eq!(id, "evt_12345:telegram");
    }

    #[test]
    fn log_id_format_matches_legacy_specification() {
        let at = DateTime::from_timestamp_millis(1_700_000_000_000).unwrap();
        let id = NotificationLogItem::build_id("evt_12345", at);
        assert_eq!(id, "evt_12345:1700000000000");
    }

    #[test]
    fn deterministic_backoff_calculation_clamps_to_max() {
        // Attempt 1: 30_000 ms
        let b1 = calculate_backoff_ms(
            1,
            DEFAULT_OUTBOX_BASE_MS,
            DEFAULT_OUTBOX_FACTOR,
            DEFAULT_OUTBOX_MAX_MS,
            0.0,
            0.0,
        );
        assert_eq!(b1, 30_000);

        // Attempt 2: 60_000 ms
        let b2 = calculate_backoff_ms(
            2,
            DEFAULT_OUTBOX_BASE_MS,
            DEFAULT_OUTBOX_FACTOR,
            DEFAULT_OUTBOX_MAX_MS,
            0.0,
            0.0,
        );
        assert_eq!(b2, 60_000);

        // Attempt 3: 120_000 ms
        let b3 = calculate_backoff_ms(
            3,
            DEFAULT_OUTBOX_BASE_MS,
            DEFAULT_OUTBOX_FACTOR,
            DEFAULT_OUTBOX_MAX_MS,
            0.0,
            0.0,
        );
        assert_eq!(b3, 120_000);

        // High attempt capped at 1h (3_600_000 ms)
        let b_high = calculate_backoff_ms(
            10,
            DEFAULT_OUTBOX_BASE_MS,
            DEFAULT_OUTBOX_FACTOR,
            DEFAULT_OUTBOX_MAX_MS,
            0.0,
            0.0,
        );
        assert_eq!(b_high, 3_600_000);
    }

    #[test]
    fn backoff_with_jitter_respects_ratio() {
        // Attempt 1 with +20% jitter
        let b_plus = calculate_backoff_ms(
            1,
            DEFAULT_OUTBOX_BASE_MS,
            DEFAULT_OUTBOX_FACTOR,
            DEFAULT_OUTBOX_MAX_MS,
            0.2,
            1.0,
        );
        assert_eq!(b_plus, 36_000);

        // Attempt 1 with -20% jitter
        let b_minus = calculate_backoff_ms(
            1,
            DEFAULT_OUTBOX_BASE_MS,
            DEFAULT_OUTBOX_FACTOR,
            DEFAULT_OUTBOX_MAX_MS,
            0.2,
            -1.0,
        );
        assert_eq!(b_minus, 24_000);
    }
}
