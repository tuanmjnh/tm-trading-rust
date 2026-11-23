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

/// Funding rate record for one symbol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct FundingRateDto {
    pub symbol: String,
    pub rate: f64,
    pub pct: f64,
    pub next_funding_time: i64,
    pub price: f64,
    pub oi_contracts: Option<f64>,
    pub oi_notional_usd: Option<f64>,
    pub score: f64,
}

/// Market funding rates response including snapshot and extreme funding rates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MarketFundingDto {
    pub generated_at: DateTime<Utc>,
    pub threshold: f64,
    pub snapshot: Vec<FundingRateDto>,
    pub extremes: Vec<FundingRateDto>,
}

/// One liquidation price zone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct LiquidationZoneDto {
    pub lev: f64,
    pub w: f64,
    pub side: String,
    pub price: f64,
    pub pct: f64,
    pub usd: f64,
}

/// Estimated liquidation zones for a symbol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MarketLiquidationDto {
    pub generated_at: DateTime<Utc>,
    pub symbol: String,
    pub mark_price: f64,
    pub open_interest_usd: f64,
    pub zones: Vec<LiquidationZoneDto>,
}

/// Inbound TradingView webhook payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TradingViewAlertPayload {
    pub v: u32,
    pub action: String,
    pub side: String,
    pub price: f64,
    pub sl: f64,
    pub tps: Vec<f64>,
    pub ts: String,
    pub symbol: String,
    pub timeframe: Option<String>,
    pub strategy: Option<String>,
}

/// Webhook intake acknowledgement response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct WebhookResponse {
    pub ok: bool,
    pub deduplicated: bool,
    pub notify_id: Option<String>,
    pub error: Option<String>,
}
