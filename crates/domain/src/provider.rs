use chrono::{DateTime, Utc};
use thiserror::Error;
use url::Url;

/// Longest accepted provider display name, in characters.
pub const MAX_PROVIDER_DISPLAY_NAME_CHARS: usize = 80;
/// Shortest accepted per-request timeout for one provider health probe.
pub const MIN_PROVIDER_TIMEOUT_MS: u64 = 500;
/// Longest accepted per-request timeout for one provider health probe.
pub const MAX_PROVIDER_TIMEOUT_MS: u64 = 30_000;

/// Last recorded health of a provider instance.
///
/// `Unknown` means no health probe has completed yet; it is never reported as healthy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderHealthState {
    Unknown,
    Healthy,
    Unhealthy,
}

impl ProviderHealthState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::Healthy => "HEALTHY",
            Self::Unhealthy => "UNHEALTHY",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "UNKNOWN" => Some(Self::Unknown),
            "HEALTHY" => Some(Self::Healthy),
            "UNHEALTHY" => Some(Self::Unhealthy),
            _ => None,
        }
    }
}

/// Everything needed to create a provider instance.
#[derive(Debug, Clone)]
pub struct NewProviderInstance {
    pub id: String,
    pub provider_definition_id: String,
    pub display_name: String,
    pub base_url: String,
    pub timeout_ms: u64,
    pub created_at: DateTime<Utc>,
}

/// Everything needed to rehydrate a provider instance from durable storage.
///
/// Unlike [`NewProviderInstance`], this carries the observed health so a restart does not
/// appear to reset the last probe.
#[derive(Debug, Clone)]
pub struct StoredProviderInstance {
    pub id: String,
    pub provider_definition_id: String,
    pub display_name: String,
    pub enabled: bool,
    pub base_url: String,
    pub timeout_ms: u64,
    pub health_state: ProviderHealthState,
    pub last_health_check_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_latency_ms: Option<u64>,
    pub last_error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One configured instance of a provider catalog definition.
///
/// Configuration and observed health are kept apart: `updated_at` changes only when the
/// operator edits the instance, while the `last_*` fields describe the newest probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderInstance {
    id: String,
    provider_definition_id: String,
    display_name: String,
    enabled: bool,
    base_url: String,
    timeout_ms: u64,
    health_state: ProviderHealthState,
    last_health_check_at: Option<DateTime<Utc>>,
    last_success_at: Option<DateTime<Utc>>,
    last_latency_ms: Option<u64>,
    last_error_code: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl ProviderInstance {
    pub fn new(input: NewProviderInstance) -> Result<Self, ProviderInstanceError> {
        Ok(Self {
            id: required("id", input.id)?,
            provider_definition_id: required("provider definition", input.provider_definition_id)?,
            display_name: normalize_display_name(&input.display_name)?,
            enabled: true,
            base_url: normalize_base_url(&input.base_url)?,
            timeout_ms: check_timeout(input.timeout_ms)?,
            health_state: ProviderHealthState::Unknown,
            last_health_check_at: None,
            last_success_at: None,
            last_latency_ms: None,
            last_error_code: None,
            created_at: input.created_at,
            updated_at: input.created_at,
        })
    }

    /// Rebuilds an instance from durable storage, validating it like a fresh record.
    pub fn from_stored(input: StoredProviderInstance) -> Result<Self, ProviderInstanceError> {
        Ok(Self {
            id: required("id", input.id)?,
            provider_definition_id: required("provider definition", input.provider_definition_id)?,
            display_name: normalize_display_name(&input.display_name)?,
            enabled: input.enabled,
            base_url: normalize_base_url(&input.base_url)?,
            timeout_ms: check_timeout(input.timeout_ms)?,
            health_state: input.health_state,
            last_health_check_at: input.last_health_check_at,
            last_success_at: input.last_success_at,
            last_latency_ms: input.last_latency_ms,
            last_error_code: input.last_error_code,
            created_at: input.created_at,
            updated_at: input.updated_at,
        })
    }

    /// Returns a copy with edited settings and a new update time.
    ///
    /// Observed health is intentionally preserved: editing a display name does not
    /// claim that the provider was just probed.
    pub fn with_settings(
        self,
        display_name: &str,
        base_url: &str,
        timeout_ms: u64,
        enabled: bool,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, ProviderInstanceError> {
        Ok(Self {
            display_name: normalize_display_name(display_name)?,
            base_url: normalize_base_url(base_url)?,
            timeout_ms: check_timeout(timeout_ms)?,
            enabled,
            updated_at,
            ..self
        })
    }

    /// Records the outcome of one bounded health probe.
    ///
    /// A failure keeps the last successful probe time but clears the latency, because a
    /// failed request has no meaningful duration to report.
    #[must_use]
    pub fn with_health(
        self,
        observed_at: DateTime<Utc>,
        healthy: bool,
        latency_ms: Option<u64>,
        error_code: Option<&str>,
    ) -> Self {
        Self {
            health_state: if healthy {
                ProviderHealthState::Healthy
            } else {
                ProviderHealthState::Unhealthy
            },
            last_health_check_at: Some(observed_at),
            last_success_at: if healthy {
                Some(observed_at)
            } else {
                self.last_success_at
            },
            last_latency_ms: if healthy { latency_ms } else { None },
            last_error_code: if healthy {
                None
            } else {
                error_code.map(str::to_owned)
            },
            ..self
        }
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn provider_definition_id(&self) -> &str {
        &self.provider_definition_id
    }

    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    #[must_use]
    pub const fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    #[must_use]
    pub const fn health_state(&self) -> ProviderHealthState {
        self.health_state
    }

    #[must_use]
    pub const fn last_health_check_at(&self) -> Option<DateTime<Utc>> {
        self.last_health_check_at
    }

    #[must_use]
    pub const fn last_success_at(&self) -> Option<DateTime<Utc>> {
        self.last_success_at
    }

    #[must_use]
    pub const fn last_latency_ms(&self) -> Option<u64> {
        self.last_latency_ms
    }

    #[must_use]
    pub fn last_error_code(&self) -> Option<&str> {
        self.last_error_code.as_deref()
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

/// Trims a display name and enforces the length limit.
pub fn normalize_display_name(name: &str) -> Result<String, ProviderInstanceError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ProviderInstanceError::EmptyDisplayName);
    }
    if name.chars().count() > MAX_PROVIDER_DISPLAY_NAME_CHARS {
        return Err(ProviderInstanceError::DisplayNameTooLong);
    }
    Ok(name.to_owned())
}

/// Validates a provider base URL.
///
/// Plain HTTP is accepted only for a loopback host so a local test server or proxy can be
/// used, while a remote provider always requires TLS.
pub fn normalize_base_url(value: &str) -> Result<String, ProviderInstanceError> {
    let value = value.trim().trim_end_matches('/');
    let url = Url::parse(value).map_err(|_| ProviderInstanceError::InvalidBaseUrl)?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err(ProviderInstanceError::InvalidBaseUrl);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ProviderInstanceError::CredentialsInUrl);
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(ProviderInstanceError::InvalidBaseUrl);
    }
    let host = url
        .host_str()
        .ok_or(ProviderInstanceError::InvalidBaseUrl)?;
    if url.scheme() == "http" && !is_loopback_host(host) {
        return Err(ProviderInstanceError::InsecureBaseUrl);
    }
    if is_restricted_host(host) {
        return Err(ProviderInstanceError::RestrictedAddress);
    }
    Ok(value.to_owned())
}

pub(crate) fn is_loopback_host(host: &str) -> bool {
    // The URL parser keeps the brackets around an IPv6 literal.
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

pub(crate) fn is_restricted_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let Ok(address) = host.parse::<std::net::IpAddr>() else {
        return false;
    };
    if address.is_loopback() {
        return false;
    }
    match address {
        std::net::IpAddr::V4(address) => {
            address.is_private()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_multicast()
                || address.is_broadcast()
                || (address.octets()[0] == 100 && (address.octets()[1] & 0xc0) == 64)
        }
        std::net::IpAddr::V6(address) => {
            let segments = address.segments();
            address.is_unspecified()
                || address.is_multicast()
                || (segments[0] & 0xffc0) == 0xfe80
                || (segments[0] & 0xfe00) == 0xfc00
        }
    }
}

fn check_timeout(timeout_ms: u64) -> Result<u64, ProviderInstanceError> {
    if (MIN_PROVIDER_TIMEOUT_MS..=MAX_PROVIDER_TIMEOUT_MS).contains(&timeout_ms) {
        Ok(timeout_ms)
    } else {
        Err(ProviderInstanceError::TimeoutOutOfRange)
    }
}

fn required(field: &'static str, value: String) -> Result<String, ProviderInstanceError> {
    if value.trim().is_empty() {
        return Err(ProviderInstanceError::EmptyField(field));
    }
    Ok(value)
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProviderInstanceError {
    #[error("provider instance {0} must not be empty")]
    EmptyField(&'static str),
    #[error("provider display name must not be empty")]
    EmptyDisplayName,
    #[error("provider display name is too long")]
    DisplayNameTooLong,
    #[error("provider base URL is not a valid http(s) URL")]
    InvalidBaseUrl,
    #[error("provider base URL must not carry credentials")]
    CredentialsInUrl,
    #[error("provider base URL must use https unless it targets the loopback interface")]
    InsecureBaseUrl,
    #[error("provider base URL must not target a private, link-local or metadata address")]
    RestrictedAddress,
    #[error("provider timeout is outside the allowed range")]
    TimeoutOutOfRange,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 19, 0, 0, 0).unwrap()
    }

    fn new(base_url: &str) -> Result<ProviderInstance, ProviderInstanceError> {
        ProviderInstance::new(NewProviderInstance {
            id: "6512a1f4c8e3b2a91f0d4e77".to_owned(),
            provider_definition_id: "binance".to_owned(),
            display_name: " Binance Public ".to_owned(),
            base_url: base_url.to_owned(),
            timeout_ms: 5_000,
            created_at: at(),
        })
    }

    #[test]
    fn normalizes_identity_and_starts_unknown() {
        let instance = new("https://api.binance.com/").unwrap();

        assert_eq!(instance.display_name(), "Binance Public");
        assert_eq!(instance.base_url(), "https://api.binance.com");
        assert!(instance.enabled());
        assert_eq!(instance.health_state(), ProviderHealthState::Unknown);
    }

    #[test]
    fn accepts_plain_http_only_for_loopback() {
        assert!(new("http://127.0.0.1:8787").is_ok());
        assert!(new("http://localhost:8787").is_ok());
        assert!(new("http://[::1]:8787").is_ok());
        assert_eq!(
            new("http://api.binance.com"),
            Err(ProviderInstanceError::InsecureBaseUrl)
        );
    }

    #[test]
    fn rejects_private_link_local_and_metadata_ip_literals() {
        for base_url in [
            "https://10.0.0.1",
            "https://169.254.169.254",
            "https://192.168.1.10",
            "https://[fe80::1]",
            "https://[fc00::1]",
        ] {
            assert_eq!(
                new(base_url),
                Err(ProviderInstanceError::RestrictedAddress),
                "base URL should be rejected: {base_url}"
            );
        }
    }

    #[test]
    fn rejects_a_url_with_embedded_credentials() {
        assert_eq!(
            new("https://user:secret@api.binance.com"),
            Err(ProviderInstanceError::CredentialsInUrl)
        );
    }

    #[test]
    fn rejects_an_unsupported_scheme_or_query() {
        assert_eq!(
            new("ws://api.binance.com"),
            Err(ProviderInstanceError::InvalidBaseUrl)
        );
        assert_eq!(
            new("https://api.binance.com?token=x"),
            Err(ProviderInstanceError::InvalidBaseUrl)
        );
    }

    #[test]
    fn rejects_a_timeout_outside_the_bounded_range() {
        for timeout_ms in [0, MIN_PROVIDER_TIMEOUT_MS - 1, MAX_PROVIDER_TIMEOUT_MS + 1] {
            let result = ProviderInstance::new(NewProviderInstance {
                id: "id".to_owned(),
                provider_definition_id: "okx".to_owned(),
                display_name: "OKX".to_owned(),
                base_url: "https://www.okx.com".to_owned(),
                timeout_ms,
                created_at: at(),
            });

            assert_eq!(result, Err(ProviderInstanceError::TimeoutOutOfRange));
        }
    }

    #[test]
    fn settings_update_keeps_the_observed_health() {
        let instance =
            new("https://api.binance.com")
                .unwrap()
                .with_health(at(), true, Some(42), None);

        let edited = instance
            .clone()
            .with_settings(
                "Renamed",
                "https://api.binance.com",
                9_000,
                false,
                at() + chrono::Duration::seconds(5),
            )
            .unwrap();

        assert_eq!(edited.display_name(), "Renamed");
        assert!(!edited.enabled());
        assert_eq!(edited.health_state(), ProviderHealthState::Healthy);
        assert_eq!(edited.last_latency_ms(), Some(42));
    }

    #[test]
    fn a_failed_probe_clears_the_latency_but_keeps_the_last_success_time() {
        let first_success =
            new("https://api.binance.com")
                .unwrap()
                .with_health(at(), true, Some(42), None);
        let failed = first_success.clone().with_health(
            at() + chrono::Duration::seconds(60),
            false,
            None,
            Some("timeout"),
        );

        assert_eq!(failed.health_state(), ProviderHealthState::Unhealthy);
        assert_eq!(failed.last_success_at(), Some(at()));
        assert_eq!(failed.last_latency_ms(), None);
        assert_eq!(failed.last_error_code(), Some("timeout"));
    }
}
