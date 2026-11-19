//! Public market-data adapters.
//!
//! Every adapter here is read-only: it fetches public bars, normalizes them into
//! [`tm_trading_domain::Candle`], and can run one bounded health request. No
//! adapter holds credentials, and none may reach an order endpoint.

mod binance;
mod okx;

pub use binance::{BINANCE_SPOT_BASE_URL, BinanceProvider};
pub use okx::{OKX_BASE_URL, OkxProvider};

use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;
use tm_trading_domain::{Candle, CandleError};

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
