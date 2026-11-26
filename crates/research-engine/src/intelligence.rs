//! Pure market intelligence calculations ported from legacy `regime`, `scanner`, and `confluence` services.
//!
//! No I/O, no wall clock, no provider SDKs.

use serde::{Deserialize, Serialize};

use crate::Bar;

/// Overall market season derived from the Altcoin Season Index (90-day score).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarketSeason {
    Alt,
    Btc,
    Neutral,
}

impl MarketSeason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Alt => "alt",
            Self::Btc => "btc",
            Self::Neutral => "neutral",
        }
    }
}

/// Altcoin Season Index scores across 30d, 90d, and 365d windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AsiScores {
    pub d30: Option<u32>,
    pub d90: Option<u32>,
    pub d365: Option<u32>,
}

/// Fear & Greed snapshot parsed from Alternative.me API data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FearGreedSnapshot {
    pub value: u32,
    pub classification: String,
    pub avg30: Option<u32>,
}

/// Classifies market season from the 90-day Altcoin Season Index (`>=75` alt, `<=25` btc).
#[must_use]
pub const fn classify_season(asi_d90: Option<u32>) -> MarketSeason {
    match asi_d90 {
        Some(v) if v >= 75 => MarketSeason::Alt,
        Some(v) if v <= 25 => MarketSeason::Btc,
        _ => MarketSeason::Neutral,
    }
}

/// Parses Altcoin Season Index scores from blockchaincenter.net HTML.
///
/// Looks for `latestScores":{"30":x,"90":y,"365":z}` first, then falls back to
/// `<n>% · It is ... Altcoin Season` in `og:description`.
#[must_use]
pub fn parse_asi_html(html: &str) -> Option<AsiScores> {
    if html.is_empty() {
        return None;
    }
    let mut out = AsiScores::default();
    if let Some(idx) = html.find("latestScores") {
        let end = html.len().min(idx + 200);
        let seg = &html[idx..end];
        out.d30 = extract_json_u32(seg, "30");
        out.d90 = extract_json_u32(seg, "90");
        out.d365 = extract_json_u32(seg, "365");
    }
    if out.d90.is_none()
        && let Some(pos) = html.find("% · It is")
    {
        let start = pos.saturating_sub(4);
        let prefix = &html[start..pos];
        let digits: String = prefix
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect();
        let digits: String = digits.chars().rev().collect();
        if let Ok(val) = digits.parse::<u32>() {
            out.d90 = Some(val);
        }
    }
    if out.d30.is_none() && out.d90.is_none() && out.d365.is_none() {
        None
    } else {
        Some(out)
    }
}

fn extract_json_u32(segment: &str, key: &str) -> Option<u32> {
    for pattern in [format!("\"{key}\":"), format!("\\\"{key}\\\":")] {
        if let Some(pos) = segment.find(&pattern) {
            let rest = segment[pos + pattern.len()..].trim_start();
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(val) = digits.parse::<u32>() {
                return Some(val);
            }
        }
    }
    None
}

/// Parses Fear & Greed entries (`(value, classification)` pairs, newest first).
#[must_use]
pub fn parse_fng_entries(entries: &[(u32, &str)]) -> Option<FearGreedSnapshot> {
    let first = entries.first()?;
    let sum: u64 = entries.iter().map(|(v, _)| u64::from(*v)).sum();
    let count = u64::try_from(entries.len()).ok()?;
    let avg30 = u32::try_from((sum + count / 2) / count).ok();
    Some(FearGreedSnapshot {
        value: first.0,
        classification: first.1.to_owned(),
        avg30,
    })
}

/// Normalized 24h ticker row for scanner ranking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScannerTicker {
    pub symbol: String,
    pub pct24h: f64,
    pub quote_volume: f64,
    pub last_price: f64,
}

/// Filters raw tickers to USDT perpetuals meeting minimum quote volume and positive price.
#[must_use]
pub fn pick_tickers(rows: Vec<ScannerTicker>, min_quote_vol: f64) -> Vec<ScannerTicker> {
    rows.into_iter()
        .filter(|r| {
            r.symbol.ends_with("USDT")
                && r.quote_volume.is_finite()
                && r.quote_volume >= min_quote_vol
                && r.last_price.is_finite()
                && r.last_price > 0.0
        })
        .collect()
}

/// Returns top `limit` tickers ranked by absolute 24h percentage change.
#[must_use]
pub fn top_movers(mut tickers: Vec<ScannerTicker>, limit: usize) -> Vec<ScannerTicker> {
    tickers.sort_by(|a, b| b.pct24h.abs().total_cmp(&a.pct24h.abs()));
    tickers.truncate(limit);
    tickers
}

/// Scanner volatility and flow statistics computed over closed bars.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScannerStats {
    pub last_price: f64,
    pub chg_4h: f64,
    pub price_delta_48: f64,
    pub atr_14: f64,
    pub atr_pct: f64,
    pub range_pct: f64,
    pub vol_ratio: f64,
    pub cmf: f64,
    pub obv_norm: f64,
}

/// Computes scanner statistics from at least 60 closed bars.
#[must_use]
pub fn compute_scanner_stats(bars: &[Bar]) -> Option<ScannerStats> {
    let n = bars.len();
    if n < 60 {
        return None;
    }
    let last = bars[n - 1];
    if last.close <= 0.0 || bars[n - 5].close <= 0.0 || bars[n - 49].close <= 0.0 {
        return None;
    }
    let chg_4h = (last.close / bars[n - 5].close - 1.0) * 100.0;
    let price_delta_48 = (last.close / bars[n - 49].close - 1.0) * 100.0;

    let mut tr_sum = 0.0;
    for i in (n - 14)..n {
        let pc = bars[i - 1].close;
        let tr = (bars[i].high - bars[i].low)
            .max((bars[i].high - pc).abs())
            .max((bars[i].low - pc).abs());
        tr_sum += tr;
    }
    let atr_14 = tr_sum / 14.0;
    let atr_pct = (atr_14 / last.close) * 100.0;

    let w48 = &bars[(n - 48)..];
    let hi48 = w48.iter().map(|b| b.high).fold(f64::NEG_INFINITY, f64::max);
    let lo48 = w48.iter().map(|b| b.low).fold(f64::INFINITY, f64::min);
    let range_pct = if lo48 > 0.0 {
        ((hi48 - lo48) / lo48) * 100.0
    } else {
        0.0
    };

    let recent_vol: f64 = bars[(n - 5)..].iter().map(|b| b.volume).sum::<f64>() / 5.0;
    let base_vol: f64 = bars[(n - 25)..(n - 6)]
        .iter()
        .map(|b| b.volume)
        .sum::<f64>()
        / 19.0;
    let vol_ratio = if base_vol > 0.0 {
        recent_vol / base_vol
    } else {
        1.0
    };

    let w20 = &bars[(n - 20)..];
    let vol_sum: f64 = w20.iter().map(|b| b.volume).sum();
    let mut mfv = 0.0;
    for b in w20 {
        let rng = b.high - b.low;
        if rng > 0.0 {
            mfv += (((b.close - b.low) - (b.high - b.close)) / rng) * b.volume;
        }
    }
    let cmf = if vol_sum > 0.0 { mfv / vol_sum } else { 0.0 };

    let mut obv = 0.0;
    let mut obv_series = Vec::with_capacity(n);
    obv_series.push(0.0);
    for i in 1..n {
        let diff = bars[i].close - bars[i - 1].close;
        let sign = if diff > 0.0 {
            1.0
        } else if diff < 0.0 {
            -1.0
        } else {
            0.0
        };
        obv += sign * bars[i].volume;
        obv_series.push(obv);
    }
    let obv_delta = obv_series[n - 1] - obv_series[n - 21];
    let obv_norm = if base_vol > 0.0 {
        obv_delta / base_vol
    } else {
        0.0
    };

    Some(ScannerStats {
        last_price: last.close,
        chg_4h,
        price_delta_48,
        atr_14,
        atr_pct,
        range_pct,
        vol_ratio,
        cmf,
        obv_norm,
    })
}

/// Weights for combining the 4 confluence components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConfluenceWeights {
    pub method: f64,
    pub regime: f64,
    pub funding: f64,
    pub zone: f64,
}

impl Default for ConfluenceWeights {
    fn default() -> Self {
        Self {
            method: 0.4,
            regime: 0.2,
            funding: 0.2,
            zone: 0.2,
        }
    }
}

/// Individual confluence component scores, each bounded in `[-1.0, 1.0]`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ConfluenceParts {
    pub method: f64,
    pub regime: f64,
    pub funding: f64,
    pub zone: f64,
}

#[must_use]
pub fn clamp11(x: f64) -> f64 {
    if x.is_finite() {
        x.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Combines the 4 confluence components into a single score in `[-1.0, 1.0]`.
#[must_use]
pub fn combine_score(parts: ConfluenceParts, weights: ConfluenceWeights) -> f64 {
    clamp11(
        weights.method * clamp11(parts.method)
            + weights.regime * clamp11(parts.regime)
            + weights.funding * clamp11(parts.funding)
            + weights.zone * clamp11(parts.zone),
    )
}

const MAJORS: [&str; 4] = ["BTCUSDT", "ETHUSDT", "BNBUSDT", "XRPUSDT"];

/// Scores the macro regime contribution for a given symbol in `[-1.0, 1.0]`.
#[must_use]
pub fn score_regime(season: MarketSeason, fng_value: Option<u32>, symbol: &str) -> f64 {
    let mut s = 0.0;
    let is_major = MAJORS.contains(&symbol);
    match season {
        MarketSeason::Alt => s += if is_major { -0.05 } else { 0.2 },
        MarketSeason::Btc => {
            s += if symbol == "BTCUSDT" {
                0.2
            } else if is_major {
                0.05
            } else {
                -0.1
            };
        }
        MarketSeason::Neutral => {}
    }
    if let Some(fng) = fng_value {
        if fng <= 25 {
            s += 0.15;
        } else if fng >= 75 {
            s -= 0.15;
        }
    }
    clamp11(s)
}

/// Scores funding rate contrarian pressure (`-rate / 0.0003` clamped to `[-1.0, 1.0]`).
#[must_use]
pub fn score_funding(rate: f64) -> f64 {
    if !rate.is_finite() {
        return 0.0;
    }
    clamp11(-rate / 0.0003)
}

/// Scores nearest liquidation zone proximity (`pct` in `[-2.5, 0.0]` indicates close long liquidation below).
#[must_use]
pub fn score_zone_nearest_pct(nearest_pct: Option<f64>) -> f64 {
    match nearest_pct {
        Some(pct) if pct.is_finite() && (-2.5..=0.0).contains(&pct) => 0.2,
        Some(pct) if pct.is_finite() && (0.0..=2.5).contains(&pct) => -0.2,
        _ => 0.0,
    }
}

// ---------------------------------------------------------------------------
// Cross-exchange scanner (legacy `market/crossScanner.mjs`)
// ---------------------------------------------------------------------------

pub const CROSS_SCAN_WARNING: &str =
    "observed differences — market context, not guaranteed arbitrage";

/// Observed price/funding snapshot for one instrument on one venue.
#[derive(Debug, Clone, PartialEq)]
pub struct VenueSnapshot {
    pub venue: String,
    pub last_price: Option<f64>,
    pub funding_rate: Option<f64>,
}

/// Pairwise observed spread between two venues.
#[derive(Debug, Clone, PartialEq)]
pub struct CrossSpreadSummary {
    pub instrument: String,
    pub pair: String,
    pub abs_spread: f64,
    pub rel_spread_pct: f64,
    pub funding_spread: Option<f64>,
}

/// Relative spread `|a - b| / a` as a ratio, or `None` when invalid or zero base.
#[must_use]
pub fn rel_spread(a: f64, b: f64) -> Option<f64> {
    if !a.is_finite() || !b.is_finite() || a == 0.0 {
        return None;
    }
    Some((a - b).abs() / a)
}

/// Absolute spread `|a - b|`, or `None` when either value is non-finite.
#[must_use]
pub fn abs_spread(a: f64, b: f64) -> Option<f64> {
    if !a.is_finite() || !b.is_finite() {
        return None;
    }
    Some((a - b).abs())
}

/// Computes pairwise observed spreads across all valid venue snapshots for an instrument.
#[must_use]
pub fn compute_cross_spreads(
    instrument: &str,
    venues: &[VenueSnapshot],
) -> Vec<CrossSpreadSummary> {
    let mut out = Vec::new();
    for i in 0..venues.len() {
        for j in (i + 1)..venues.len() {
            let (v1, v2) = (&venues[i], &venues[j]);
            if let (Some(p1), Some(p2)) = (v1.last_price, v2.last_price)
                && let (Some(abs), Some(rel)) = (abs_spread(p1, p2), rel_spread(p1, p2))
            {
                let funding_spread = match (v1.funding_rate, v2.funding_rate) {
                    (Some(f1), Some(f2)) => abs_spread(f1, f2),
                    _ => None,
                };
                out.push(CrossSpreadSummary {
                    instrument: instrument.to_owned(),
                    pair: format!("{}-{}", v1.venue, v2.venue),
                    abs_spread: abs,
                    rel_spread_pct: rel * 100.0,
                    funding_spread,
                });
            }
        }
    }
    out.sort_by(|a, b| b.rel_spread_pct.total_cmp(&a.rel_spread_pct));
    out
}

// ---------------------------------------------------------------------------
// Macro series (legacy `services/macro.mjs`)
// ---------------------------------------------------------------------------

/// Canonical FRED macro series metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroSeriesSpec {
    pub series_id: &'static str,
    pub name: &'static str,
    pub unit: &'static str,
    pub frequency: &'static str,
}

pub const MACRO_SERIES_SPECS: [MacroSeriesSpec; 6] = [
    MacroSeriesSpec {
        series_id: "FEDFUNDS",
        name: "Effective Federal Funds Rate",
        unit: "%",
        frequency: "m",
    },
    MacroSeriesSpec {
        series_id: "DGS10",
        name: "10-Year Treasury Constant Maturity Rate",
        unit: "%",
        frequency: "d",
    },
    MacroSeriesSpec {
        series_id: "CPIAUCSL",
        name: "Consumer Price Index (1982-84=100)",
        unit: "index",
        frequency: "m",
    },
    MacroSeriesSpec {
        series_id: "PAYEMS",
        name: "All Employees: Total Nonfarm Payrolls",
        unit: "thousands",
        frequency: "m",
    },
    MacroSeriesSpec {
        series_id: "UNRATE",
        name: "Unemployment Rate",
        unit: "%",
        frequency: "m",
    },
    MacroSeriesSpec {
        series_id: "WALCL",
        name: "Fed Balance Sheet: Total Assets",
        unit: "$M",
        frequency: "w",
    },
];

/// Parsed latest observation for a FRED macro series.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroObservationSnapshot {
    pub series_id: String,
    pub name: String,
    pub unit: String,
    pub value: f64,
    pub date: String,
}

/// Extracts the latest valid numeric observation from `(date, raw_value)` pairs.
#[must_use]
pub fn parse_fred_latest(
    spec: MacroSeriesSpec,
    observations: &[(&str, &str)],
) -> Option<MacroObservationSnapshot> {
    observations.iter().rev().find_map(|(date, raw)| {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed == "." {
            return None;
        }
        let value = trimmed.parse::<f64>().ok()?;
        if !value.is_finite() {
            return None;
        }
        Some(MacroObservationSnapshot {
            series_id: spec.series_id.to_owned(),
            name: spec.name.to_owned(),
            unit: spec.unit.to_owned(),
            value,
            date: (*date).to_owned(),
        })
    })
}

// ---------------------------------------------------------------------------
// Alert rules matcher (legacy `engine/alertRules.mjs`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertRuleLevel {
    High,
    Med,
    Low,
}

impl AlertRuleLevel {
    #[must_use]
    const fn rank(self) -> u8 {
        match self {
            Self::High => 0,
            Self::Med => 1,
            Self::Low => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertRuleDirection {
    Pos,
    Neg,
    Flat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertRuleSpec {
    pub id: String,
    pub name: String,
    pub min_level: Option<AlertRuleLevel>,
    pub dirs: Vec<AlertRuleDirection>,
    pub symbols: Vec<String>,
    pub min_score: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertCandidateEvent {
    pub key: String,
    pub symbol: Option<String>,
    pub level: Option<AlertRuleLevel>,
    pub direction: Option<AlertRuleDirection>,
    pub score: Option<f64>,
    pub title: String,
}

/// Evaluates whether a candidate event satisfies all filters of an alert rule.
#[must_use]
pub fn match_alert_rule(rule: &AlertRuleSpec, event: &AlertCandidateEvent) -> bool {
    if let Some(min_level) = rule.min_level {
        let Some(ev_level) = event.level else {
            return false;
        };
        if ev_level.rank() > min_level.rank() {
            return false;
        }
    }
    if !rule.dirs.is_empty() {
        let Some(ev_dir) = event.direction else {
            return false;
        };
        if !rule.dirs.contains(&ev_dir) {
            return false;
        }
    }
    if !rule.symbols.is_empty() {
        let Some(ev_sym) = event.symbol.as_deref() else {
            return false;
        };
        if !rule.symbols.iter().any(|s| s.eq_ignore_ascii_case(ev_sym)) {
            return false;
        }
    }
    if let Some(min_score) = rule.min_score {
        let Some(ev_score) = event.score else {
            return false;
        };
        if !ev_score.is_finite() || ev_score.abs() < min_score {
            return false;
        }
    }
    true
}

/// Deterministic deduplication key for a `(rule_id, event_key)` pair.
#[must_use]
pub fn rule_event_dedupe_key(rule_id: &str, event_key: &str) -> String {
    format!("alert-rule:{rule_id}:{event_key}")
}

// ---------------------------------------------------------------------------
// Candle reconciliation (legacy `market/reconcile.mjs`)
// ---------------------------------------------------------------------------

pub const DEFAULT_PRICE_TOL_PCT: f64 = 0.0005;
pub const DEFAULT_VOLUME_TOL_PCT: f64 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileVerdict {
    Ok,
    Gap,
    Suspect,
    Unknown,
}

impl ReconcileVerdict {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Gap => "gap",
            Self::Suspect => "suspect",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReconcileReport {
    pub verdict: ReconcileVerdict,
    pub checked: usize,
    pub matched: usize,
    pub mismatched: usize,
    pub missing_local: usize,
}

fn rel_diff(a: f64, b: f64) -> f64 {
    if b == 0.0 {
        if a == 0.0 { 0.0 } else { 1.0 }
    } else {
        (a - b).abs() / b.abs()
    }
}

/// Reconciles local closed bars against venue REST bars by open timestamp.
#[must_use]
pub fn reconcile_bars(
    local: &[Bar],
    venue: &[Bar],
    price_tol_pct: f64,
    volume_tol_pct: f64,
) -> ReconcileReport {
    if local.is_empty() && venue.is_empty() {
        return ReconcileReport {
            verdict: ReconcileVerdict::Unknown,
            checked: 0,
            matched: 0,
            mismatched: 0,
            missing_local: 0,
        };
    }
    let mut matched = 0usize;
    let mut mismatched = 0usize;
    let mut missing_local = 0usize;

    for v in venue {
        let Some(l) = local.iter().find(|b| b.time == v.time) else {
            missing_local += 1;
            continue;
        };
        let price_ok = rel_diff(l.open, v.open) <= price_tol_pct
            && rel_diff(l.high, v.high) <= price_tol_pct
            && rel_diff(l.low, v.low) <= price_tol_pct
            && rel_diff(l.close, v.close) <= price_tol_pct;
        let vol_ok = rel_diff(l.volume, v.volume) <= volume_tol_pct;
        if price_ok && vol_ok {
            matched += 1;
        } else {
            mismatched += 1;
        }
    }

    let verdict = if venue.is_empty() {
        ReconcileVerdict::Unknown
    } else if mismatched > 0 {
        ReconcileVerdict::Suspect
    } else if missing_local > 0 {
        ReconcileVerdict::Gap
    } else {
        ReconcileVerdict::Ok
    };

    ReconcileReport {
        verdict,
        checked: venue.len(),
        matched,
        mismatched,
        missing_local,
    }
}

// ---------------------------------------------------------------------------
// News extraction eligibility & retention (legacy `newsExtract.mjs`, `retention`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsExtractCandidate {
    pub id: String,
    pub is_high_level: bool,
    pub is_multi_source: bool,
    pub already_assessed: bool,
}

/// Picks up to `budget` unassessed high-level or multi-source news items.
#[must_use]
pub fn pick_news_extract_eligible(
    candidates: &[NewsExtractCandidate],
    budget: usize,
) -> Vec<String> {
    let mut ranked: Vec<&NewsExtractCandidate> = candidates
        .iter()
        .filter(|c| !c.already_assessed && (c.is_high_level || c.is_multi_source))
        .collect();
    ranked.sort_by_key(|c| {
        let score = u8::from(c.is_high_level) * 2 + u8::from(c.is_multi_source) * 2;
        std::cmp::Reverse(score)
    });
    ranked
        .into_iter()
        .take(budget)
        .map(|c| c.id.clone())
        .collect()
}

/// Returns `true` when an item's age exceeds `retention_days`.
#[must_use]
pub const fn is_retention_expired(age_seconds: i64, retention_days: u32) -> bool {
    age_seconds > (retention_days as i64) * 86_400
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_asi_from_latest_scores_and_fallback() {
        let html = r#"<script>window.__DATA={"latestScores":{"30":42,"90":78,"365":61}}</script>"#;
        let parsed = parse_asi_html(html).expect("asi scores");
        assert_eq!(parsed.d30, Some(42));
        assert_eq!(parsed.d90, Some(78));
        assert_eq!(parsed.d365, Some(61));
        assert_eq!(classify_season(parsed.d90), MarketSeason::Alt);

        let og = r#"<meta property="og:description" content="22% · It is not Altcoin Season">"#;
        let fallback = parse_asi_html(og).expect("og fallback");
        assert_eq!(fallback.d90, Some(22));
        assert_eq!(classify_season(fallback.d90), MarketSeason::Btc);
    }

    #[test]
    fn computes_scanner_stats_and_confluence() {
        let bars: Vec<Bar> = (0..65)
            .map(|i| Bar {
                time: i * 3_600_000,
                open: 100.0 + (i as f64) * 0.5,
                high: 102.0 + (i as f64) * 0.5,
                low: 99.0 + (i as f64) * 0.5,
                close: 101.0 + (i as f64) * 0.5,
                volume: 1_000.0 + (i as f64) * 10.0,
            })
            .collect();
        let stats = compute_scanner_stats(&bars).expect("stats");
        assert!(stats.chg_4h > 0.0);
        assert!(stats.atr_pct > 0.0);
        assert!(stats.obv_norm > 0.0);

        let score = combine_score(
            ConfluenceParts {
                method: 0.8,
                regime: score_regime(MarketSeason::Alt, Some(20), "SOLUSDT"),
                funding: score_funding(-0.0003),
                zone: score_zone_nearest_pct(Some(-1.2)),
            },
            ConfluenceWeights::default(),
        );
        assert!(score > 0.5 && score <= 1.0);
    }

    #[test]
    fn computes_cross_spreads_macro_and_alert_rules() {
        let spreads = compute_cross_spreads(
            "BTC/USDT",
            &[
                VenueSnapshot {
                    venue: "binance".to_owned(),
                    last_price: Some(60_000.0),
                    funding_rate: Some(0.0001),
                },
                VenueSnapshot {
                    venue: "okx".to_owned(),
                    last_price: Some(60_060.0),
                    funding_rate: Some(0.0002),
                },
            ],
        );
        assert_eq!(spreads.len(), 1);
        assert_eq!(spreads[0].pair, "binance-okx");
        assert!((spreads[0].rel_spread_pct - 0.1).abs() < 1e-6);

        let macro_obs = parse_fred_latest(
            MACRO_SERIES_SPECS[0],
            &[("2026-10-01", "."), ("2026-11-01", "4.50")],
        )
        .expect("macro obs");
        assert_eq!(macro_obs.series_id, "FEDFUNDS");
        assert!((macro_obs.value - 4.5).abs() < 1e-6);

        let rule = AlertRuleSpec {
            id: "r1".to_owned(),
            name: "High BTC".to_owned(),
            min_level: Some(AlertRuleLevel::Med),
            dirs: vec![AlertRuleDirection::Pos],
            symbols: vec!["BTCUSDT".to_owned()],
            min_score: Some(0.5),
        };
        let ev = AlertCandidateEvent {
            key: "ev1".to_owned(),
            symbol: Some("BTCUSDT".to_owned()),
            level: Some(AlertRuleLevel::High),
            direction: Some(AlertRuleDirection::Pos),
            score: Some(0.8),
            title: "Breakout".to_owned(),
        };
        assert!(match_alert_rule(&rule, &ev));
        assert_eq!(rule_event_dedupe_key("r1", "ev1"), "alert-rule:r1:ev1");

        let bar1 = Bar {
            time: 1_000,
            open: 100.0,
            high: 102.0,
            low: 99.0,
            close: 101.0,
            volume: 500.0,
        };
        let bar2 = Bar {
            time: 2_000,
            ..bar1
        };
        let report_ok = reconcile_bars(
            &[bar1, bar2],
            &[bar1, bar2],
            DEFAULT_PRICE_TOL_PCT,
            DEFAULT_VOLUME_TOL_PCT,
        );
        assert_eq!(report_ok.verdict, ReconcileVerdict::Ok);
        let report_gap = reconcile_bars(
            &[bar1],
            &[bar1, bar2],
            DEFAULT_PRICE_TOL_PCT,
            DEFAULT_VOLUME_TOL_PCT,
        );
        assert_eq!(report_gap.verdict, ReconcileVerdict::Gap);

        let eligible = pick_news_extract_eligible(
            &[
                NewsExtractCandidate {
                    id: "n1".to_owned(),
                    is_high_level: false,
                    is_multi_source: false,
                    already_assessed: false,
                },
                NewsExtractCandidate {
                    id: "n2".to_owned(),
                    is_high_level: true,
                    is_multi_source: true,
                    already_assessed: false,
                },
            ],
            10,
        );
        assert_eq!(eligible, vec!["n2"]);
        assert!(is_retention_expired(31 * 86_400, 30));
    }
}
