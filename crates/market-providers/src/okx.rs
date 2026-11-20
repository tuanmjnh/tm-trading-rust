use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Deserialize;
use tm_trading_domain::{Candle, NewCandle};

use crate::{
    MarketDataProvider, ProviderError, ProviderHealth, ProviderId, http_client, request_error,
};

pub const OKX_BASE_URL: &str = "https://www.okx.com";

const PROVIDER: &str = "okx";
const MARKET: &str = "futures";
const MAX_BARS: u32 = 300;

/// OKX public swap-candle adapter (`/api/v5`).
///
/// Only public endpoints are reachable from here: candles and a server-time ping.
pub struct OkxProvider {
    http: reqwest::Client,
    base_url: String,
}

impl OkxProvider {
    pub fn new(base_url: impl Into<String>, timeout: Duration) -> Result<Self, ProviderError> {
        let base_url = base_url.into();
        crate::validate_provider_url(PROVIDER, &base_url)?;
        Ok(Self {
            http: http_client(PROVIDER, timeout)?,
            base_url: base_url.trim_end_matches('/').to_owned(),
        })
    }
}

#[derive(Deserialize)]
struct CandlesEnvelope {
    code: String,
    #[serde(default)]
    data: Vec<Vec<String>>,
}

/// `/api/v5/public/time` only needs the status code; its `data` is an object.
#[derive(Deserialize)]
struct StatusEnvelope {
    code: String,
}

#[async_trait]
impl MarketDataProvider for OkxProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Okx
    }

    async fn closed_candles(
        &self,
        symbol: &str,
        timeframe: &str,
        limit: u32,
        now_ms: i64,
    ) -> Result<Vec<Candle>, ProviderError> {
        let timeframe = crate::normalized_timeframe(timeframe)?;
        let bar = okx_bar(&timeframe)?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        // One extra row: the newest bar may still be forming and is dropped.
        let requested = limit.saturating_add(1).min(MAX_BARS);
        let response = self
            .http
            .get(format!("{}/api/v5/market/candles", self.base_url))
            .query(&[
                ("instId", symbol),
                ("bar", bar.as_str()),
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
        let envelope: CandlesEnvelope =
            response.json().await.map_err(|_| ProviderError::Payload {
                provider: PROVIDER,
                what: "invalid JSON",
            })?;
        if envelope.code != "0" {
            return Err(ProviderError::Payload {
                provider: PROVIDER,
                what: "response code was not 0",
            });
        }
        let mut candles = parse_candles(symbol, &timeframe, &envelope.data, now_ms)?;
        candles.truncate(limit as usize);
        Ok(candles)
    }

    async fn health(&self) -> Result<ProviderHealth, ProviderError> {
        let started = Instant::now();
        let response = self
            .http
            .get(format!("{}/api/v5/public/time", self.base_url))
            .send()
            .await
            .map_err(|error| request_error(PROVIDER, error))?;
        if !response.status().is_success() {
            return Err(ProviderError::Status {
                provider: PROVIDER,
                status: response.status().as_u16(),
            });
        }
        let envelope: StatusEnvelope =
            response.json().await.map_err(|_| ProviderError::Payload {
                provider: PROVIDER,
                what: "invalid JSON",
            })?;
        if envelope.code != "0" {
            return Err(ProviderError::Payload {
                provider: PROVIDER,
                what: "response code was not 0",
            });
        }
        Ok(ProviderHealth {
            provider: ProviderId::Okx,
            latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }
}

/// OKX spells hours, days, and weeks in upper case. Minutes must stay lower case:
/// `1M` is a monthly bar, not a one-minute bar.
fn okx_bar(timeframe: &str) -> Result<String, ProviderError> {
    crate::timeframe_millis(timeframe)?;
    let (digits, unit) = timeframe.split_at(timeframe.len() - 1);
    let unit = match unit {
        "m" => "m",
        "h" => "H",
        "d" => "D",
        "w" => "W",
        _ => {
            // `normalized_timeframe` lowercased the input, so upper-case units only
            // arrive here when a caller skips it.
            return Err(ProviderError::Timeframe(timeframe.to_owned()));
        }
    };
    Ok(format!("{digits}{unit}"))
}

/// Maps `/api/v5/market/candles` rows to closed candles, oldest first.
///
/// Each row is `[ts, open, high, low, close, volume, ..., confirm]`, newest
/// first, where `confirm` is `1` only once the bar is closed. OKX reports the
/// open time alone, so the close time is derived from the bar length.
fn parse_candles(
    symbol: &str,
    timeframe: &str,
    rows: &[Vec<String>],
    now_ms: i64,
) -> Result<Vec<Candle>, ProviderError> {
    let bar_millis = crate::timeframe_millis(timeframe)?;
    let mut candles = Vec::with_capacity(rows.len());
    for row in rows {
        if row.len() < 9 {
            return Err(unreadable("row has too few fields"));
        }
        if row[8] != "1" {
            continue;
        }
        let open_time = integer_field(&row[0])?;
        let close_time = open_time + bar_millis - 1;
        if close_time >= now_ms {
            continue;
        }
        candles.push(Candle::new(NewCandle {
            symbol: symbol.to_owned(),
            timeframe: timeframe.to_owned(),
            market: Some(MARKET.to_owned()),
            source: ProviderId::Okx.as_str().to_owned(),
            open: number_field(&row[1])?,
            high: number_field(&row[2])?,
            low: number_field(&row[3])?,
            close: number_field(&row[4])?,
            volume: number_field(&row[5])?,
            open_time,
            close_time,
            event_time: None,
            // The caller's observation time records when this worker received the bar.
            ingest_time: Some(now_ms),
        })?);
    }
    candles.sort_by_key(|candle| candle.open_time());
    Ok(candles)
}

fn number_field(value: &str) -> Result<f64, ProviderError> {
    value
        .parse::<f64>()
        .map_err(|_| unreadable("field is not numeric"))
}

fn integer_field(value: &str) -> Result<i64, ProviderError> {
    value
        .parse::<i64>()
        .map_err(|_| unreadable("timestamp is not an integer"))
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

    const NOW: i64 = 1_700_000_400_000;

    fn row(open_time: i64, confirm: &str) -> Vec<String> {
        vec![
            open_time.to_string(),
            "2000.5".to_owned(),
            "2010".to_owned(),
            "1990".to_owned(),
            "2005".to_owned(),
            "150".to_owned(),
            "150".to_owned(),
            "300750".to_owned(),
            confirm.to_owned(),
        ]
    }

    #[test]
    fn maps_a_confirmed_bar_and_derives_the_close_time() {
        let candles =
            parse_candles("BTC-USDT-SWAP", "5m", &[row(1_700_000_000_000, "1")], NOW).unwrap();

        let candle = &candles[0];
        assert_eq!(candle.id(), "BTC-USDT-SWAP:5m:1700000000000");
        assert_eq!(candle.source(), "okx");
        assert_eq!(candle.market(), Some("futures"));
        assert_eq!(candle.close_time(), 1_700_000_299_999);
        assert_eq!(candle.ingest_time(), NOW);
    }

    #[test]
    fn drops_an_unconfirmed_bar() {
        let candles = parse_candles(
            "BTC-USDT-SWAP",
            "1m",
            &[row(1_700_000_000_000, "1"), row(1_700_000_060_000, "0")],
            NOW,
        )
        .unwrap();

        assert_eq!(candles.len(), 1);
        assert_eq!(candles[0].open_time(), 1_700_000_000_000);
    }

    #[test]
    fn sorts_rows_oldest_first() {
        let candles = parse_candles(
            "BTC-USDT-SWAP",
            "1m",
            &[row(1_700_000_060_000, "1"), row(1_700_000_000_000, "1")],
            NOW,
        )
        .unwrap();

        assert_eq!(
            candles.iter().map(Candle::open_time).collect::<Vec<_>>(),
            vec![1_700_000_000_000, 1_700_000_060_000]
        );
    }

    #[test]
    fn rejects_a_row_that_is_too_short() {
        assert!(matches!(
            parse_candles(
                "BTC-USDT-SWAP",
                "1m",
                &[vec!["1700000000000".to_owned()]],
                NOW
            ),
            Err(ProviderError::Payload { .. })
        ));
    }

    #[test]
    fn maps_timeframes_to_okx_bar_names() {
        assert_eq!(okx_bar("1m").unwrap(), "1m");
        assert_eq!(okx_bar("4h").unwrap(), "4H");
        assert_eq!(okx_bar("1d").unwrap(), "1D");
        assert!(okx_bar("1y").is_err());
    }
}
