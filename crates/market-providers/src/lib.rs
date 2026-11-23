//! Public market-data adapters.
//!
//! Every adapter here is read-only: it fetches public bars, normalizes them into
//! [`tm_trading_domain::Candle`], and can run one bounded health request. No
//! adapter holds credentials, and none may reach an order endpoint.

mod binance;
mod okx;

pub use binance::{
    BINANCE_FUTURES_BASE_URL, BINANCE_SPOT_BASE_URL, BinanceProvider, fetch_open_interest,
    fetch_premium_index,
};
pub use okx::{OKX_BASE_URL, OkxProvider};

use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;
use tm_trading_domain::{Candle, CandleError};
use url::Url;

/// Bar lengths every public adapter in this crate accepts.
pub const SUPPORTED_TIMEFRAMES: &[&str] = &["1m", "5m", "15m", "1h", "4h", "1d", "1w"];

/// Static metadata for one provider adapter, independent of any configured instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderDefinition {
    pub id: &'static str,
    pub display_name: &'static str,
    pub default_base_url: &'static str,
    pub timeframes: &'static [&'static str],
}

/// The adapters compiled into this build.
pub const PROVIDER_DEFINITIONS: &[ProviderDefinition] = &[
    ProviderDefinition {
        id: "binance",
        display_name: "Binance Public",
        default_base_url: BINANCE_SPOT_BASE_URL,
        timeframes: SUPPORTED_TIMEFRAMES,
    },
    ProviderDefinition {
        id: "okx",
        display_name: "OKX Public",
        default_base_url: OKX_BASE_URL,
        timeframes: SUPPORTED_TIMEFRAMES,
    },
];

#[must_use]
pub fn provider_definition(id: &str) -> Option<&'static ProviderDefinition> {
    PROVIDER_DEFINITIONS
        .iter()
        .find(|definition| definition.id == id)
}

/// Builds the adapter named by `definition_id`.
///
/// `base_url` and `timeout` come from the configured instance, so a test probe uses the
/// operator's settings instead of a hardcoded default.
pub fn build_provider(
    definition_id: &str,
    base_url: &str,
    timeout: Duration,
) -> Result<Box<dyn MarketDataProvider>, ProviderError> {
    validate_provider_url(definition_id, base_url)?;
    match definition_id {
        "binance" => Ok(Box::new(BinanceProvider::new(base_url, timeout)?)),
        "okx" => Ok(Box::new(OkxProvider::new(base_url, timeout)?)),
        other => Err(ProviderError::UnknownDefinition(other.to_owned())),
    }
}

/// Validates a provider endpoint against the adapter's host allow-list.
///
/// Loopback endpoints remain available for a local test server. Production adapters accept
/// only their official public host, so a configured URL cannot turn the worker into an
/// arbitrary server-side request proxy.
pub fn validate_provider_url(definition_id: &str, base_url: &str) -> Result<(), ProviderError> {
    let provider = match definition_id {
        "binance" => "binance",
        "okx" => "okx",
        other => return Err(ProviderError::UnknownDefinition(other.to_owned())),
    };
    let url =
        Url::parse(base_url.trim()).map_err(|_| ProviderError::InvalidBaseUrl { provider })?;
    let host = url
        .host_str()
        .ok_or(ProviderError::InvalidBaseUrl { provider })?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ProviderError::InvalidBaseUrl { provider });
    }
    let loopback = is_loopback_host(host);
    let allowed = match definition_id {
        "binance" => {
            host.eq_ignore_ascii_case("api.binance.com")
                || host.eq_ignore_ascii_case("fapi.binance.com")
        }
        "okx" => host.eq_ignore_ascii_case("www.okx.com"),
        _ => false,
    };
    if allowed {
        if url.scheme() != "https" {
            return Err(ProviderError::InsecureBaseUrl { provider });
        }
        Ok(())
    } else if loopback {
        Ok(())
    } else {
        Err(ProviderError::ForbiddenHost { provider })
    }
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

/// Catalog identity of a public market-data provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderId {
    Binance,
    Okx,
}

impl ProviderId {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Binance => "binance",
            Self::Okx => "okx",
        }
    }
}

/// Result of one bounded health request: the provider answered, and how long it took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderHealth {
    pub provider: ProviderId,
    pub latency_ms: u64,
}

#[async_trait]
pub trait MarketDataProvider: Send + Sync {
    fn id(&self) -> ProviderId;

    /// Closed bars for `symbol` at `timeframe`, oldest first.
    ///
    /// `now_ms` is the caller's observation time, so a bar that is still forming
    /// at the provider is dropped instead of being recorded as a fact.
    async fn closed_candles(
        &self,
        symbol: &str,
        timeframe: &str,
        limit: u32,
        now_ms: i64,
    ) -> Result<Vec<Candle>, ProviderError>;

    /// One small unauthenticated request with the adapter's own timeout.
    async fn health(&self) -> Result<ProviderHealth, ProviderError>;
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("failed to build the {provider} HTTP client: {message}")]
    Client {
        provider: &'static str,
        message: String,
    },
    #[error("{provider} request failed: {message}")]
    Request {
        provider: &'static str,
        message: String,
    },
    #[error("{provider} request timed out")]
    Timeout { provider: &'static str },
    #[error("unknown provider definition '{0}'")]
    UnknownDefinition(String),
    #[error("{provider} base URL is invalid")]
    InvalidBaseUrl { provider: &'static str },
    #[error("{provider} base URL must use https unless it targets the loopback interface")]
    InsecureBaseUrl { provider: &'static str },
    #[error("{provider} base URL host is not allowed")]
    ForbiddenHost { provider: &'static str },
    #[error("{provider} returned HTTP {status}")]
    Status { provider: &'static str, status: u16 },
    #[error("{provider} returned an unexpected payload: {what}")]
    Payload {
        provider: &'static str,
        what: &'static str,
    },
    #[error("unsupported timeframe '{0}'")]
    Timeframe(String),
    #[error(transparent)]
    Candle(#[from] CandleError),
}

impl ProviderError {
    /// Stable, sanitized class recorded with a provider health result.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Client { .. }
            | Self::UnknownDefinition(_)
            | Self::InvalidBaseUrl { .. }
            | Self::InsecureBaseUrl { .. }
            | Self::ForbiddenHost { .. } => "configuration",
            Self::Request { .. } => "unavailable",
            Self::Timeout { .. } => "timeout",
            Self::Status { .. } => "http_status",
            Self::Payload { .. } | Self::Candle(_) => "invalid_payload",
            Self::Timeframe(_) => "unsupported_capability",
        }
    }
}

pub(crate) fn request_error(provider: &'static str, error: reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::Timeout { provider }
    } else {
        ProviderError::Request {
            provider,
            message: error.to_string(),
        }
    }
}

/// Bar length in milliseconds for a canonical timeframe such as `1m` or `4h`.
pub fn timeframe_millis(timeframe: &str) -> Result<i64, ProviderError> {
    let (digits, unit) = timeframe.split_at(timeframe.len().saturating_sub(1));
    let count: i64 = digits
        .parse()
        .map_err(|_| ProviderError::Timeframe(timeframe.to_owned()))?;
    let unit_ms = match unit {
        "m" => 60_000,
        "h" | "H" => 3_600_000,
        "d" | "D" => 86_400_000,
        "w" | "W" => 604_800_000,
        _ => return Err(ProviderError::Timeframe(timeframe.to_owned())),
    };
    if count <= 0 {
        return Err(ProviderError::Timeframe(timeframe.to_owned()));
    }
    Ok(count * unit_ms)
}

/// Canonical lowercase spelling used for stored bar identity and Binance intervals.
pub(crate) fn normalized_timeframe(timeframe: &str) -> Result<String, ProviderError> {
    timeframe_millis(timeframe)?;
    Ok(timeframe.to_ascii_lowercase())
}

/// Builds the bounded HTTP client shared by the adapters.
pub(crate) fn http_client(
    provider: &'static str,
    timeout: Duration,
) -> Result<reqwest::Client, ProviderError> {
    reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!(
            "tm-trading-market-providers/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|error| ProviderError::Client {
            provider,
            message: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_canonical_timeframes() {
        assert_eq!(timeframe_millis("1m").unwrap(), 60_000);
        assert_eq!(timeframe_millis("4h").unwrap(), 14_400_000);
        assert_eq!(timeframe_millis("1D").unwrap(), 86_400_000);
    }

    #[test]
    fn the_catalog_lists_every_compiled_adapter() {
        assert_eq!(
            provider_definition("binance").unwrap().default_base_url,
            BINANCE_SPOT_BASE_URL
        );
        assert_eq!(
            provider_definition("okx").unwrap().default_base_url,
            OKX_BASE_URL
        );
        assert!(provider_definition("unknown").is_none());
    }

    #[test]
    fn provider_urls_are_limited_to_the_adapter_host_or_loopback() {
        assert!(validate_provider_url("binance", "https://api.binance.com").is_ok());
        assert!(validate_provider_url("okx", "https://www.okx.com").is_ok());
        assert!(validate_provider_url("binance", "http://127.0.0.1:8787").is_ok());
        assert!(matches!(
            validate_provider_url("binance", "https://example.com"),
            Err(ProviderError::ForbiddenHost {
                provider: "binance"
            })
        ));
    }

    #[test]
    fn provider_urls_reject_credentials_queries_fragments_and_wrong_schemes() {
        for base_url in [
            "https://user:secret@api.binance.com",
            "https://api.binance.com?token=secret",
            "https://api.binance.com#fragment",
            "ftp://api.binance.com",
        ] {
            assert!(
                matches!(
                    validate_provider_url("binance", base_url),
                    Err(ProviderError::InvalidBaseUrl {
                        provider: "binance"
                    })
                ),
                "base URL should be rejected: {base_url}"
            );
        }
    }

    #[tokio::test]
    async fn provider_redirects_are_not_followed() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::thread;
        use std::time::Duration;

        let target = TcpListener::bind("127.0.0.1:0").expect("bind redirect target");
        let target_address = target.local_addr().expect("read target address");
        target
            .set_nonblocking(true)
            .expect("make target listener non-blocking");
        let target_hit = Arc::new(AtomicBool::new(false));
        let target_hit_server = Arc::clone(&target_hit);
        let target_thread = thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(1);
            while std::time::Instant::now() < deadline {
                match target.accept() {
                    Ok((mut stream, _)) => {
                        target_hit_server.store(true, Ordering::SeqCst);
                        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("redirect target failed: {error}"),
                }
            }
        });

        let redirect = TcpListener::bind("127.0.0.1:0").expect("bind redirect source");
        let redirect_address = redirect.local_addr().expect("read redirect address");
        let redirect_thread = thread::spawn(move || {
            let (mut stream, _) = redirect.accept().expect("accept redirect request");
            let mut request = [0_u8; 2_048];
            let _ = stream.read(&mut request);
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{target_address}/api/v3/ping\r\nContent-Length: 0\r\n\r\n"
            );
            stream
                .write_all(response.as_bytes())
                .expect("write redirect response");
        });

        let client = http_client("binance", Duration::from_secs(1)).expect("build HTTP client");
        let response = client
            .get(format!("http://{redirect_address}/api/v3/ping"))
            .send()
            .await
            .expect("receive redirect response");
        assert_eq!(response.status(), 302);
        redirect_thread.join().expect("join redirect source");
        target_thread.join().expect("join redirect target");
        assert!(!target_hit.load(Ordering::SeqCst));
    }

    #[test]
    fn an_unknown_definition_is_a_configuration_error() {
        let error = build_provider("unknown", "https://example.com", Duration::from_secs(1))
            .err()
            .expect("unknown definition fails");

        assert_eq!(error.code(), "configuration");
    }

    #[test]
    fn rejects_an_unknown_or_empty_timeframe() {
        assert!(matches!(
            timeframe_millis("1y"),
            Err(ProviderError::Timeframe(_))
        ));
        assert!(matches!(
            timeframe_millis(""),
            Err(ProviderError::Timeframe(_))
        ));
        assert!(matches!(
            timeframe_millis("0m"),
            Err(ProviderError::Timeframe(_))
        ));
    }
}
