use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Macro crypto regime snapshot derived from Altcoin Season Index, Fear & Greed, and market dominance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MarketRegimeDto {
    pub season: String,
    pub asi_d30: Option<u32>,
    pub asi_d90: Option<u32>,
    pub asi_d365: Option<u32>,
    pub fng_value: Option<u32>,
    pub fng_classification: Option<String>,
    pub fng_avg30: Option<u32>,
    pub btc_dominance: Option<f64>,
    pub eth_dominance: Option<f64>,
}

/// Top 24h market mover discovered by the scanner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ScannerMoverDto {
    pub symbol: String,
    pub last_price: f64,
    pub pct_24h: f64,
    pub quote_volume: f64,
}

/// Multi-factor confluence score for a tracked symbol in `[-1.0, 1.0]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ConfluenceScoreDto {
    pub symbol: String,
    pub total_score: f64,
    pub method_score: f64,
    pub regime_score: f64,
    pub funding_score: f64,
    pub zone_score: f64,
    pub bias: String,
}

/// Observed cross-exchange price/funding spread (market context, not guaranteed arbitrage).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CrossScanRowDto {
    pub instrument: String,
    pub pair: String,
    pub abs_spread: f64,
    pub rel_spread_pct: f64,
    pub funding_spread: Option<f64>,
    pub warning: String,
}

/// Latest observed FRED macro series value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MacroSeriesDto {
    pub series_id: String,
    pub name: String,
    pub unit: String,
    pub value: f64,
    pub date: String,
}

/// Summary of alert-rule evaluation against recent research/news events.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, ToSchema)]
pub struct AlertRuleEvaluationDto {
    pub evaluated_rules: usize,
    pub matched_events: usize,
    pub latest_match_title: Option<String>,
}

/// Candle reconciliation verdict for a symbol/timeframe against venue REST bars.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ReconcileStatusDto {
    pub symbol: String,
    pub timeframe: String,
    pub verdict: String,
    pub checked: usize,
    pub mismatched: usize,
    pub missing_local: usize,
}

/// Platform maintenance summary (`reconcile`, `news_extract`, `retention`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, ToSchema)]
pub struct PlatformMaintenanceDto {
    pub reconcile: Vec<ReconcileStatusDto>,
    pub news_extract_eligible: usize,
    pub retention_days: u32,
}

/// Combined market intelligence snapshot (`regime`, `scanner`, `confluence`, `cross_scans`, `macro_series`, `alert_evaluation`, `platform`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MarketIntelligenceDto {
    pub generated_at: DateTime<Utc>,
    pub regime: MarketRegimeDto,
    pub scanner_movers: Vec<ScannerMoverDto>,
    pub confluence: Vec<ConfluenceScoreDto>,
    #[serde(default)]
    pub cross_scans: Vec<CrossScanRowDto>,
    #[serde(default)]
    pub macro_series: Vec<MacroSeriesDto>,
    #[serde(default)]
    pub alert_evaluation: AlertRuleEvaluationDto,
    #[serde(default)]
    pub platform: PlatformMaintenanceDto,
}
