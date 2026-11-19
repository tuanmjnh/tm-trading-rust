use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::Value;
use tm_trading_domain::{Candle, NewCandle};

use crate::{
    MarketDataProvider, ProviderError, ProviderHealth, ProviderId, http_client, request_error,
};

pub const BINANCE_SPOT_BASE_URL: &str = "https://api.binance.com";

const PROVIDER: &str = "binance";
const MARKET: &str = "spot";
const MAX_BARS: u32 = 1000;

/// Binance spot public-data adapter (`/api/v3`).
///
/// Only public endpoints are reachable from here: bars and a ping.
pub struct BinanceProvider {
    http: reqwest::Client,
    base_url: String,
}

impl BinanceProvider {
    pub fn new(base_url: impl Into<String>, timeout: Duration) -> Result<Self, ProviderError> {
        Ok(Self {
            http: http_client(PROVIDER, timeout)?,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
        })
    }
}

#[async_trait]
impl MarketDataProvider for BinanceProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Binance
    }

    async fn closed_candles(
        &self,
        symbol: &str,
        timeframe: &str,
        limit: u32,
        now_ms: i64,
    ) -> Result<Vec<Candle>, ProviderError> {
        let timeframe = crate::normalized_timeframe(timeframe)?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        // One extra row: the newest bar may still be forming and is dropped.
        let requested = limit.saturating_add(1).min(MAX_BARS);
        let response = self
            .http
            .get(format!("{}/api/v3/klines", self.base_url))
            .query(&[
                ("symbol", symbol),
                ("interval", timeframe.as_str()),
                ("limit", &requested.to_string()),
            ])
            .send()
            .await
            .map_err(|error| request_error(PROVIDER, error))?;
        if !response.status().is_success() {
            return Err(ProviderError::Status {
                provider: PROVIDER,
                status: response.status().as_u16(),
            });
        }
        let payload: Value = response
            .json()
            .await
            .map_err(|error| ProviderError::Payload {
                provider: PROVIDER,
                what: if error.is_decode() {
                    "invalid JSON"
                } else {
                    "unreadable body"
                },
            })?;
        let mut candles = parse_klines(symbol, &timeframe, &payload, now_ms)?;
        candles.truncate(limit as usize);
        Ok(candles)
    }

    async fn health(&self) -> Result<ProviderHealth, ProviderError> {
        let started = Instant::now();
        let response = self
            .http
            .get(format!("{}/api/v3/ping", self.base_url))
            .send()
            .await
            .map_err(|error| request_error(PROVIDER, error))?;
        if !response.status().is_success() {
            return Err(ProviderError::Status {
                provider: PROVIDER,
                status: response.status().as_u16(),
            });
        }
        Ok(ProviderHealth {
            provider: ProviderId::Binance,
            latency_ms: elapsed_millis(started),
        })
    }
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Maps `/api/v3/klines` rows to closed candles, oldest first.
///
/// Each row is `[openTime, open, high, low, close, volume, closeTime, ...]` with
/// numbers encoded as strings. A row that has not closed at `now_ms` is skipped,
/// and a row that cannot be read rejects the whole payload rather than being
/// silently dropped.
fn parse_klines(
    symbol: &str,
    timeframe: &str,
    payload: &Value,
    now_ms: i64,
) -> Result<Vec<Candle>, ProviderError> {
    let rows = payload.as_array().ok_or(ProviderError::Payload {
        provider: PROVIDER,
        what: "not an array",
    })?;
    let mut candles = Vec::with_capacity(rows.len());
    for row in rows {
        let fields = row.as_array().ok_or(ProviderError::Payload {
            provider: PROVIDER,
            what: "row is not an array",
        })?;
        let open_time = integer_field(fields, 0)?;
        let close_time = integer_field(fields, 6)?;
        if close_time >= now_ms {
            continue;
        }
        candles.push(Candle::new(NewCandle {
            symbol: symbol.to_owned(),
            timeframe: timeframe.to_owned(),
            market: Some(MARKET.to_owned()),
            source: ProviderId::Binance.as_str().to_owned(),
            open: number_field(fields, 1)?,
            high: number_field(fields, 2)?,
            low: number_field(fields, 3)?,
            close: number_field(fields, 4)?,
            volume: number_field(fields, 5)?,
            open_time,
            close_time,
            event_time: None,
            // The exchange close time is the only event time Binance gives us.
            ingest_time: None,
        })?);
    }
    candles.sort_by_key(|candle| candle.open_time());
    Ok(candles)
}

fn number_field(fields: &[Value], index: usize) -> Result<f64, ProviderError> {
    match fields.get(index) {
        Some(Value::String(value)) => value
            .parse::<f64>()
            .map_err(|_| unreadable("number field is not numeric")),
        Some(Value::Number(value)) => value
            .as_f64()
            .ok_or(unreadable("number field is out of range")),
        _ => Err(unreadable("missing number field")),
    }
}

fn integer_field(fields: &[Value], index: usize) -> Result<i64, ProviderError> {
    let value = number_field(fields, index)?;
    if !value.is_finite() || value.fract() != 0.0 || value.abs() > i64::MAX as f64 {
        return Err(unreadable("timestamp is not an integer"));
    }
    Ok(value as i64)
}

fn unreadable(what: &'static str) -> ProviderError {
    ProviderError::Payload {
        provider: PROVIDER,
        what,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_700_000_120_000;

    fn row(open_time: i64) -> Value {
        json!([
            open_time.to_string(),
            "100.5",
            "110.0",
            "90.0",
            "105.25",
            "12.5",
            (open_time + 59_999).to_string(),
            "0",
            42,
            "0",
            "0",
            "0"
        ])
    }

    #[test]
    fn maps_a_closed_bar() {
        let candles = parse_klines("BTCUSDT", "1m", &json!([row(1_700_000_000_000)]), NOW).unwrap();

        assert_eq!(candles.len(), 1);
        let candle = &candles[0];
        assert_eq!(candle.id(), "BTCUSDT:1m:1700000000000");
        assert_eq!(candle.source(), "binance");
        assert_eq!(candle.market(), Some("spot"));
        assert_eq!(candle.close(), 105.25);
        assert_eq!(candle.event_time(), 1_700_000_059_999);
    }

    #[test]
    fn drops_a_still_forming_bar() {
        let forming = 1_700_000_120_000 - 30_000;

        let candles = parse_klines(
            "BTCUSDT",
            "1m",
            &json!([row(1_700_000_000_000), row(forming)]),
            NOW,
        )
        .unwrap();

        assert_eq!(candles.len(), 1);
        assert_eq!(candles[0].open_time(), 1_700_000_000_000);
    }

    #[test]
    fn sorts_rows_oldest_first() {
        let candles = parse_klines(
            "BTCUSDT",
            "1m",
            &json!([row(1_700_000_060_000), row(1_700_000_000_000)]),
            NOW,
        )
        .unwrap();

        assert_eq!(
            candles.iter().map(Candle::open_time).collect::<Vec<_>>(),
            vec![1_700_000_000_000, 1_700_000_060_000]
        );
    }

    #[test]
    fn rejects_a_row_that_is_not_a_list() {
        assert!(matches!(
            parse_klines("BTCUSDT", "1m", &json!({ "code": -1121 }), NOW),
            Err(ProviderError::Payload { .. })
        ));
    }

    #[test]
    fn rejects_a_non_numeric_price() {
        let mut broken = row(1_700_000_000_000);
        broken[4] = json!("n/a");

        assert!(matches!(
            parse_klines("BTCUSDT", "1m", &json!([broken]), NOW),
            Err(ProviderError::Payload { .. })
        ));
    }

    #[test]
    fn a_high_below_the_close_is_rejected_by_the_domain() {
        let mut broken = row(1_700_000_000_000);
        broken[2] = json!("100.0");

        assert!(matches!(
            parse_klines("BTCUSDT", "1m", &json!([broken]), NOW),
            Err(ProviderError::Candle(_))
        ));
    }
}
