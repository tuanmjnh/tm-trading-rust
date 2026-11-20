use chrono::{DateTime, Utc};
use thiserror::Error;
use url::Url;

use super::provider::{is_loopback_host, is_restricted_host};

/// Schema version used by the legacy `news_sources` collection.
pub const NEWS_SOURCE_SCHEMA_VERSION: &str = "1";
/// Maximum accepted source identifier length.
pub const MAX_NEWS_SOURCE_ID_CHARS: usize = 128;
/// Maximum accepted source display-name length.
pub const MAX_NEWS_SOURCE_NAME_CHARS: usize = 160;
/// Default polling interval used for new and legacy-compatible sources.
pub const DEFAULT_NEWS_POLL_INTERVAL_SEC: u32 = 600;
/// Default retention window used for new and legacy-compatible sources.
pub const DEFAULT_NEWS_RETENTION_DAYS: u32 = 7;
/// Minimum accepted polling interval.
pub const MIN_NEWS_POLL_INTERVAL_SEC: u32 = 60;
/// Maximum accepted polling interval.
pub const MAX_NEWS_POLL_INTERVAL_SEC: u32 = 86_400;
/// Maximum accepted retention window.
pub const MAX_NEWS_RETENTION_DAYS: u32 = 365;

/// Transport used to collect a news source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsTransport {
    Rss,
    Api,
    Licensed,
    Manual,
}

impl NewsTransport {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rss => "rss",
            Self::Api => "api",
            Self::Licensed => "licensed",
            Self::Manual => "manual",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "rss" => Some(Self::Rss),
            "api" => Some(Self::Api),
            "licensed" => Some(Self::Licensed),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

/// Editorial trust tier declared for a news source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsTrustTier {
    T0Primary,
    T1LicensedNews,
    T2Secondary,
    T3UnverifiedSocial,
}

impl NewsTrustTier {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::T0Primary => "T0_PRIMARY",
            Self::T1LicensedNews => "T1_LICENSED_NEWS",
            Self::T2Secondary => "T2_SECONDARY",
            Self::T3UnverifiedSocial => "T3_UNVERIFIED_SOCIAL",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "T0_PRIMARY" => Some(Self::T0Primary),
            "T1_LICENSED_NEWS" => Some(Self::T1LicensedNews),
            "T2_SECONDARY" => Some(Self::T2Secondary),
            "T3_UNVERIFIED_SOCIAL" => Some(Self::T3UnverifiedSocial),
            _ => None,
        }
    }
}

/// License verification state for a news source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsLicenseStatus {
    Ok,
    Unknown,
    Revoked,
}

impl NewsLicenseStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Unknown => "unknown",
            Self::Revoked => "revoked",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ok" => Some(Self::Ok),
            "unknown" => Some(Self::Unknown),
            "revoked" => Some(Self::Revoked),
            _ => None,
        }
    }
}

/// Result of the fail-closed source policy check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsLicenseGate {
    Allowed,
    SourceDisabled,
    LicenseRevoked,
    LicenseNotVerified,
    StorageNotPermitted,
}

/// Last observed health of a news source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsHealthState {
    Ok,
    Degraded,
    Down,
    Unknown,
}

impl NewsHealthState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Degraded => "degraded",
            Self::Down => "down",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ok" => Some(Self::Ok),
            "degraded" => Some(Self::Degraded),
            "down" => Some(Self::Down),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

/// Rights snapshot copied to normalized news records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewsRights {
    pub allowed_store_metadata: bool,
    pub allowed_display: bool,
    pub allowed_summarize: bool,
    pub allowed_training: bool,
}

/// Observed fetch health for one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsSourceHealth {
    pub state: NewsHealthState,
    pub last_fetch_at: Option<DateTime<Utc>>,
    pub last_ok_at: Option<DateTime<Utc>>,
    pub last_item_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub consecutive_errors: u32,
}

impl Default for NewsSourceHealth {
    fn default() -> Self {
        Self {
            state: NewsHealthState::Unknown,
            last_fetch_at: None,
            last_ok_at: None,
            last_item_at: None,
            last_error: None,
            consecutive_errors: 0,
        }
    }
}

/// Policy fields needed to insert one source into the registry.
#[derive(Debug, Clone)]
pub struct NewNewsSource {
    pub source_id: String,
    pub name: String,
    pub provider: Option<String>,
    pub transport: NewsTransport,
    pub feed_url: Option<String>,
    pub trust_tier: NewsTrustTier,
    pub market_groups: Vec<String>,
    pub languages: Vec<String>,
    pub license_status: NewsLicenseStatus,
    pub rights: NewsRights,
    pub enabled: bool,
    pub poll_interval_sec: u32,
    pub retention_days: u32,
    pub created_at: DateTime<Utc>,
}

/// Complete source record loaded from durable storage.
#[derive(Debug, Clone)]
pub struct StoredNewsSource {
    pub source_id: String,
    pub name: String,
    pub provider: Option<String>,
    pub transport: NewsTransport,
    pub feed_url: Option<String>,
    pub trust_tier: NewsTrustTier,
    pub market_groups: Vec<String>,
    pub languages: Vec<String>,
    pub license_status: NewsLicenseStatus,
    pub rights: NewsRights,
    pub enabled: bool,
    pub poll_interval_sec: u32,
    pub retention_days: u32,
    pub retry_requested_at: Option<DateTime<Utc>>,
    pub health: NewsSourceHealth,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One policy-backed news source and its observed fetch health.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsSource {
    source_id: String,
    name: String,
    provider: Option<String>,
    transport: NewsTransport,
    feed_url: Option<String>,
    trust_tier: NewsTrustTier,
    market_groups: Vec<String>,
    languages: Vec<String>,
    license_status: NewsLicenseStatus,
    rights: NewsRights,
    enabled: bool,
    poll_interval_sec: u32,
    retention_days: u32,
    retry_requested_at: Option<DateTime<Utc>>,
    health: NewsSourceHealth,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl NewsSource {
    /// Creates and validates a source registry record.
    pub fn new(input: NewNewsSource) -> Result<Self, NewsSourceError> {
        let source_id = normalize_source_id(&input.source_id)?;
        let name = normalize_name(&input.name)?;
        let provider = input
            .provider
            .map(|value| normalize_optional_text(value, "provider"))
            .transpose()?;
        let feed_url = input
            .feed_url
            .map(|value| normalize_feed_url(&value))
            .transpose()?;
        let market_groups = normalize_list(input.market_groups, "market group")?;
        let languages = normalize_list(input.languages, "language")?;
        let poll_interval_sec = input.poll_interval_sec;
        if !(MIN_NEWS_POLL_INTERVAL_SEC..=MAX_NEWS_POLL_INTERVAL_SEC).contains(&poll_interval_sec) {
            return Err(NewsSourceError::InvalidPollInterval);
        }
        if input.retention_days == 0 || input.retention_days > MAX_NEWS_RETENTION_DAYS {
            return Err(NewsSourceError::InvalidRetentionDays);
        }

        Ok(Self {
            source_id,
            name,
            provider,
            transport: input.transport,
            feed_url,
            trust_tier: input.trust_tier,
            market_groups,
            languages,
            license_status: input.license_status,
            rights: input.rights,
            enabled: input.enabled,
            poll_interval_sec,
            retention_days: input.retention_days,
            retry_requested_at: None,
            health: NewsSourceHealth::default(),
            created_at: input.created_at,
            updated_at: input.created_at,
        })
    }

    /// Rebuilds a source from durable storage.
    ///
    /// Legacy rows may contain operator-edited edge values that the legacy schema
    /// accepted. They are normalized to safe defaults instead of making one row
    /// unreadable and hiding the complete registry. An unsafe stored feed URL is
    /// removed and disables the source.
    pub fn from_stored(input: StoredNewsSource) -> Result<Self, NewsSourceError> {
        let provider = input
            .provider
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let stored_feed_url = input
            .feed_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let (feed_url, enabled) = match stored_feed_url {
            Some(feed_url) => match normalize_feed_url(feed_url) {
                Ok(feed_url) => (Some(feed_url), input.enabled),
                Err(_) => (None, false),
            },
            None => (None, input.enabled),
        };
        let poll_interval_sec = if (MIN_NEWS_POLL_INTERVAL_SEC..=MAX_NEWS_POLL_INTERVAL_SEC)
            .contains(&input.poll_interval_sec)
        {
            input.poll_interval_sec
        } else {
            DEFAULT_NEWS_POLL_INTERVAL_SEC
        };
        let retention_days = if (1..=MAX_NEWS_RETENTION_DAYS).contains(&input.retention_days) {
            input.retention_days
        } else {
            DEFAULT_NEWS_RETENTION_DAYS
        };

        let mut health = input.health;
        health.last_error = health.last_error.as_deref().map(sanitize_news_fetch_error);
        let mut source = Self::new(NewNewsSource {
            source_id: input.source_id,
            name: input.name,
            provider,
            transport: input.transport,
            feed_url,
            trust_tier: input.trust_tier,
            market_groups: normalize_stored_list(input.market_groups, "crypto"),
            languages: normalize_stored_list(input.languages, "en"),
            license_status: input.license_status,
            rights: input.rights,
            enabled,
            poll_interval_sec,
            retention_days,
            created_at: input.created_at,
        })?;
        source.retry_requested_at = input.retry_requested_at;
        source.health = health;
        source.updated_at = input.updated_at;
        Ok(source)
    }

    /// Returns a copy with a new update timestamp.
    #[must_use]
    pub fn with_updated_at(self, updated_at: DateTime<Utc>) -> Self {
        Self { updated_at, ..self }
    }

    /// Returns a copy with the operator's enable switch changed.
    #[must_use]
    pub fn with_enabled(self, enabled: bool, updated_at: DateTime<Utc>) -> Self {
        Self {
            enabled,
            updated_at,
            ..self
        }
    }

    /// Marks the source for an immediate retry on the next collection cycle.
    #[must_use]
    pub fn with_retry_requested(
        self,
        requested_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            retry_requested_at: Some(requested_at),
            updated_at,
            ..self
        }
    }

    /// Records one fetch result without changing policy or operator settings.
    #[must_use]
    pub fn with_fetch_result(
        mut self,
        observed_at: DateTime<Utc>,
        item_count: u32,
        error: Option<&str>,
    ) -> Self {
        self.health.last_fetch_at = Some(observed_at);
        if let Some(error) = error {
            self.health.state = NewsHealthState::Down;
            self.health.last_error = Some(sanitize_news_fetch_error(error));
            self.health.consecutive_errors = self.health.consecutive_errors.saturating_add(1);
        } else {
            self.health.state = if item_count > 0 {
                NewsHealthState::Ok
            } else {
                NewsHealthState::Degraded
            };
            self.health.last_ok_at = Some(observed_at);
            self.health.last_error = None;
            self.health.consecutive_errors = 0;
            if item_count > 0 {
                self.health.last_item_at = Some(observed_at);
            }
            self.retry_requested_at = None;
        }
        self.updated_at = observed_at;
        self
    }

    #[must_use]
    pub fn source_id(&self) -> &str {
        &self.source_id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }

    #[must_use]
    pub const fn transport(&self) -> NewsTransport {
        self.transport
    }

    #[must_use]
    pub fn feed_url(&self) -> Option<&str> {
        self.feed_url.as_deref()
    }

    #[must_use]
    pub const fn trust_tier(&self) -> NewsTrustTier {
        self.trust_tier
    }

    #[must_use]
    pub fn market_groups(&self) -> &[String] {
        &self.market_groups
    }

    #[must_use]
    pub fn languages(&self) -> &[String] {
        &self.languages
    }

    #[must_use]
    pub const fn license_status(&self) -> NewsLicenseStatus {
        self.license_status
    }

    /// Checks whether this source may be fetched and stored.
    ///
    /// The check fails closed for disabled, unverified, revoked, or storage-restricted
    /// sources, matching the legacy news-ingestion gate.
    #[must_use]
    pub const fn license_gate(&self) -> NewsLicenseGate {
        if !self.enabled {
            return NewsLicenseGate::SourceDisabled;
        }
        match self.license_status {
            NewsLicenseStatus::Revoked => NewsLicenseGate::LicenseRevoked,
            NewsLicenseStatus::Unknown => NewsLicenseGate::LicenseNotVerified,
            NewsLicenseStatus::Ok if !self.rights.allowed_store_metadata => {
                NewsLicenseGate::StorageNotPermitted
            }
            NewsLicenseStatus::Ok => NewsLicenseGate::Allowed,
        }
    }

    #[must_use]
    pub const fn rights(&self) -> NewsRights {
        self.rights
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub const fn poll_interval_sec(&self) -> u32 {
        self.poll_interval_sec
    }

    #[must_use]
    pub const fn retention_days(&self) -> u32 {
        self.retention_days
    }

    #[must_use]
    pub fn retry_requested_at(&self) -> Option<DateTime<Utc>> {
        self.retry_requested_at
    }

    #[must_use]
    pub const fn health(&self) -> &NewsSourceHealth {
        &self.health
    }

    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

fn normalize_source_id(value: &str) -> Result<String, NewsSourceError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(NewsSourceError::EmptySourceId);
    }
    if value.chars().count() > MAX_NEWS_SOURCE_ID_CHARS {
        return Err(NewsSourceError::SourceIdTooLong);
    }
    if value.chars().any(char::is_control) {
        return Err(NewsSourceError::InvalidSourceId);
    }
    Ok(value.to_owned())
}

fn normalize_name(value: &str) -> Result<String, NewsSourceError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(NewsSourceError::EmptyName);
    }
    if value.chars().count() > MAX_NEWS_SOURCE_NAME_CHARS {
        return Err(NewsSourceError::NameTooLong);
    }
    Ok(value.to_owned())
}

fn normalize_optional_text(value: String, _field: &'static str) -> Result<String, NewsSourceError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(NewsSourceError::InvalidOptionalText);
    }
    Ok(value.to_owned())
}

fn normalize_feed_url(value: &str) -> Result<String, NewsSourceError> {
    let value = value.trim();
    let url = Url::parse(value).map_err(|_| NewsSourceError::InvalidFeedUrl)?;
    if !matches!(url.scheme(), "http" | "https") || url.fragment().is_some() {
        return Err(NewsSourceError::InvalidFeedUrl);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(NewsSourceError::CredentialsInFeedUrl);
    }
    if url
        .query_pairs()
        .any(|(key, _)| is_sensitive_query_key(&key))
    {
        return Err(NewsSourceError::SecretInFeedUrl);
    }
    let host = url.host_str().ok_or(NewsSourceError::InvalidFeedUrl)?;
    if is_restricted_host(host) {
        return Err(NewsSourceError::RestrictedFeedAddress);
    }
    if url.scheme() == "http" && !is_loopback_host(host) {
        return Err(NewsSourceError::InsecureFeedUrl);
    }
    Ok(value.to_owned())
}

fn is_sensitive_query_key(key: &str) -> bool {
    let key = key
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    let key = key
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    matches!(
        key.as_str(),
        "api_key"
            | "apikey"
            | "accesskeyid"
            | "awsaccesskeyid"
            | "auth"
            | "authorization"
            | "credential"
            | "credentials"
            | "jwt"
            | "password"
            | "secret"
            | "sig"
            | "signature"
            | "token"
    ) || key.ends_with("_api_key")
        || key.ends_with("_auth")
        || key.ends_with("_credential")
        || key.ends_with("_credentials")
        || key.ends_with("_password")
        || key.ends_with("_secret")
        || key.ends_with("_signature")
        || key.ends_with("_token")
}

/// Reduces a provider error to a non-secret diagnostic code.
#[must_use]
pub fn sanitize_news_fetch_error(error: &str) -> String {
    let error = error.trim().to_ascii_lowercase().replace('-', "_");
    match error.as_str() {
        "connection_error" | "dns_error" | "http_error" | "invalid_feed" | "invalid_payload"
        | "permission_denied" | "rate_limited" | "timeout" | "tls_error" | "unavailable"
        | "unknown" => error,
        _ => "fetch failed".to_owned(),
    }
}

fn normalize_stored_list(values: Vec<String>, default: &str) -> Vec<String> {
    let mut normalized = Vec::with_capacity(values.len());
    for value in values {
        let value = value.trim();
        if value.is_empty() || normalized.iter().any(|existing| existing == value) {
            continue;
        }
        normalized.push(value.to_owned());
    }
    if normalized.is_empty() {
        normalized.push(default.to_owned());
    }
    normalized
}

fn normalize_list(values: Vec<String>, kind: &'static str) -> Result<Vec<String>, NewsSourceError> {
    if values.is_empty() {
        return Err(if kind == "language" {
            NewsSourceError::NoLanguages
        } else {
            NewsSourceError::NoMarketGroups
        });
    }
    let mut normalized = Vec::with_capacity(values.len());
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            return Err(if kind == "language" {
                NewsSourceError::InvalidLanguage
            } else {
                NewsSourceError::InvalidMarketGroup
            });
        }
        if !normalized.contains(&value.to_owned()) {
            normalized.push(value.to_owned());
        }
    }
    Ok(normalized)
}

/// Invalid news-source registry data.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NewsSourceError {
    #[error("news source id must not be empty")]
    EmptySourceId,
    #[error("news source id is too long")]
    SourceIdTooLong,
    #[error("news source id contains an invalid character")]
    InvalidSourceId,
    #[error("news source name must not be empty")]
    EmptyName,
    #[error("news source name is too long")]
    NameTooLong,
    #[error("news source optional text is invalid")]
    InvalidOptionalText,
    #[error("news source feed URL is invalid")]
    InvalidFeedUrl,
    #[error("news source feed URL must not carry credentials")]
    CredentialsInFeedUrl,
    #[error("news source feed URL must not carry a secret")]
    SecretInFeedUrl,
    #[error("news source feed URL must not target a private, link-local or metadata address")]
    RestrictedFeedAddress,
    #[error("news source feed URL must use https unless it targets the loopback interface")]
    InsecureFeedUrl,
    #[error("news source must declare at least one market group")]
    NoMarketGroups,
    #[error("news source market group is invalid")]
    InvalidMarketGroup,
    #[error("news source must declare at least one language")]
    NoLanguages,
    #[error("news source language is invalid")]
    InvalidLanguage,
    #[error("news source poll interval is out of range")]
    InvalidPollInterval,
    #[error("news source retention window is out of range")]
    InvalidRetentionDays,
}
