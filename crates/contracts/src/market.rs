use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Read-model state of one market panel or one symbol inside it.
///
/// The states are explicit so a caller can tell "no data yet" from "data exists but is
/// old" without inferring it from empty arrays or timestamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MarketStateDto {
    Ok,
    Partial,
    Stale,
    Empty,
}

/// One symbol's closed-candle summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MarketSymbolDto {
    pub symbol: String,
    pub state: MarketStateDto,
    pub candle_count: usize,
    pub last_close: Option<f64>,
    pub change_percent: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub volume: Option<f64>,
    pub last_open_time: Option<DateTime<Utc>>,
    pub last_close_time: Option<DateTime<Utc>>,
    pub data_age_seconds: Option<i64>,
}

/// Closed-candle overview for a bounded set of symbols.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MarketOverviewDto {
    pub generated_at: DateTime<Utc>,
    pub timeframe: String,
    pub state: MarketStateDto,
    pub stale_after_seconds: i64,
    pub symbols: Vec<MarketSymbolDto>,
}

/// One symbol ranked by movement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MarketMoverDto {
    pub symbol: String,
    pub state: MarketStateDto,
    pub last_close: Option<f64>,
    pub change_percent: Option<f64>,
    pub volume_ratio: Option<f64>,
    pub volatility_percent: Option<f64>,
    pub candle_count: usize,
}

/// Ranked movers and activity for the same bounded symbol set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MarketPulseDto {
    pub generated_at: DateTime<Utc>,
    pub timeframe: String,
    pub state: MarketStateDto,
    pub stale_after_seconds: i64,
    pub movers: Vec<MarketMoverDto>,
}
