//! Domain models and calculations for real-time market intelligence.
//!
//! Faithful port of legacy `services/funding.mjs`, `services/liquidation.mjs`, and
//! `services/scanner.mjs`. These are pure calculations for funding extremes and
//! liquidation zone heatmaps.

use serde::{Deserialize, Serialize};

/// Default funding rate alert threshold: 0.0005 (0.05% per 8h settlement period).
pub const DEFAULT_FUNDING_ALERT_RATE: f64 = 0.0005;

/// Default maintenance margin rate (MMR) assumed at 0.4% (0.004).
pub const DEFAULT_MMR: f64 = 0.004;

/// One funding rate record with price and optional open interest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FundingRateRecord {
    pub symbol: String,
    pub rate: f64,
    pub pct: f64,
    pub next_funding_time: i64,
    pub price: f64,
    pub oi_contracts: Option<f64>,
    pub oi_notional_usd: Option<f64>,
    pub score: f64,
}

impl FundingRateRecord {
    #[must_use]
    pub fn new(
        symbol: impl Into<String>,
        rate: f64,
        next_funding_time: i64,
        price: f64,
        oi_contracts: Option<f64>,
        oi_notional_usd: Option<f64>,
    ) -> Self {
        let abs_rate = rate.abs();
        let oi_factor = oi_notional_usd
            .map(|usd| ((usd + 1.0).log10() / 6.0).clamp(0.0, 1.0))
            .unwrap_or(0.0);
        let score = abs_rate * (1.0 + oi_factor);

        Self {
            symbol: symbol.into(),
            rate,
            pct: (rate * 100.0 * 10_000.0).round() / 10_000.0,
            next_funding_time,
            price,
            oi_contracts,
            oi_notional_usd,
            score,
        }
    }
}

/// Leverage band with assumed market weight for liquidation zone estimation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LeverageBand {
    pub lev: f64,
    pub w: f64,
}

pub const DEFAULT_LEVERAGE_BANDS: &[LeverageBand] = &[
    LeverageBand { lev: 3.0, w: 0.10 },
    LeverageBand { lev: 5.0, w: 0.12 },
    LeverageBand { lev: 10.0, w: 0.20 },
    LeverageBand { lev: 20.0, w: 0.20 },
    LeverageBand { lev: 25.0, w: 0.14 },
    LeverageBand { lev: 50.0, w: 0.14 },
    LeverageBand {
        lev: 100.0,
        w: 0.10,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiquidationSide {
    Long,
    Short,
}

impl LiquidationSide {
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Long => "long",
            Self::Short => "short",
        }
    }
}

/// Estimated liquidation zone for a given leverage tier and direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiquidationZone {
    pub lev: f64,
    pub w: f64,
    pub side: LiquidationSide,
    pub price: f64,
    pub pct: f64,
    pub usd: f64,
}

/// Evaluates funding rates: sorts by |rate| descending, takes top N for snapshot,
/// and flags extremes where |rate| >= threshold.
#[must_use]
pub fn evaluate_funding(
    rows: &[FundingRateRecord],
    snapshot_n: usize,
    max_extremes: usize,
    threshold: f64,
) -> (Vec<FundingRateRecord>, Vec<FundingRateRecord>) {
    let mut sorted = rows.to_vec();
    sorted.sort_by(|a, b| {
        b.rate
            .abs()
            .partial_cmp(&a.rate.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let snapshot = sorted.iter().take(snapshot_n).cloned().collect();
    let extremes = sorted
        .iter()
        .filter(|r| r.rate.abs() >= threshold)
        .take(max_extremes)
        .cloned()
        .collect();

    (snapshot, extremes)
}

/// Estimates theoretical liquidation price zones from mark price and open interest.
///
/// Long liquidation happens below mark price: `mark * (1 - (1/lev - mmr))`.
/// Short liquidation happens above mark price: `mark * (1 + (1/lev - mmr))`.
#[must_use]
pub fn estimate_liquidation_zones(
    mark_px: f64,
    oi_usd: f64,
    bands: &[LeverageBand],
    mmr: f64,
) -> Vec<LiquidationZone> {
    if mark_px <= 0.0 || oi_usd <= 0.0 {
        return Vec::new();
    }

    let mut zones = Vec::with_capacity(bands.len() * 2);
    for b in bands {
        let dist = 1.0 / b.lev - mmr;
        if dist <= 0.0 {
            continue;
        }

        let notional = (oi_usd * b.w).round();
        // Long liquidation zone (below mark)
        zones.push(LiquidationZone {
            lev: b.lev,
            w: b.w,
            side: LiquidationSide::Long,
            price: mark_px * (1.0 - dist),
            pct: -dist * 100.0,
            usd: notional,
        });

        // Short liquidation zone (above mark)
        zones.push(LiquidationZone {
            lev: b.lev,
            w: b.w,
            side: LiquidationSide::Short,
            price: mark_px * (1.0 + dist),
            pct: dist * 100.0,
            usd: notional,
        });
    }

    zones
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_funding_extremes_and_snapshots() {
        let rows = vec![
            FundingRateRecord::new("BTCUSDT", 0.0001, 1700000000000, 60000.0, None, None),
            FundingRateRecord::new("ETHUSDT", -0.0008, 1700000000000, 3000.0, None, None),
            FundingRateRecord::new("SOLUSDT", 0.0012, 1700000000000, 150.0, None, None),
            FundingRateRecord::new("DOGEUSDT", 0.0002, 1700000000000, 0.15, None, None),
        ];

        let (snapshot, extremes) = evaluate_funding(&rows, 3, 5, DEFAULT_FUNDING_ALERT_RATE);
        assert_eq!(snapshot.len(), 3);
        assert_eq!(snapshot[0].symbol, "SOLUSDT");
        assert_eq!(snapshot[1].symbol, "ETHUSDT");
        assert_eq!(snapshot[2].symbol, "DOGEUSDT");

        assert_eq!(extremes.len(), 2);
        assert_eq!(extremes[0].symbol, "SOLUSDT");
        assert_eq!(extremes[1].symbol, "ETHUSDT");
    }

    #[test]
    fn estimates_liquidation_zones_properly() {
        let mark_px = 50_000.0;
        let oi_usd = 1_000_000.0;
        let zones =
            estimate_liquidation_zones(mark_px, oi_usd, DEFAULT_LEVERAGE_BANDS, DEFAULT_MMR);

        assert_eq!(zones.len(), DEFAULT_LEVERAGE_BANDS.len() * 2);

        // Find 10x long liquidation: dist = 1/10 - 0.004 = 0.096 (9.6%)
        let long_10x = zones
            .iter()
            .find(|z| (z.lev - 10.0).abs() < 1e-4 && z.side == LiquidationSide::Long)
            .expect("10x long zone");
        assert!((long_10x.price - 45_200.0).abs() < 1.0);
        assert!((long_10x.usd - 200_000.0).abs() < 1.0);

        // Find 10x short liquidation: dist = 1/10 - 0.004 = 0.096 (9.6%)
        let short_10x = zones
            .iter()
            .find(|z| (z.lev - 10.0).abs() < 1e-4 && z.side == LiquidationSide::Short)
            .expect("10x short zone");
        assert!((short_10x.price - 54_800.0).abs() < 1.0);
    }
}
