//! Deterministic Liquidity Sweep method.
//!
//! Ported faithfully from `D:\Applications\tm-trading\engine\methods\sweep.mjs`.
//! Detects liquidity sweeps above confirmed swing highs or below confirmed swing lows
//! with rejection back into the range, combined with volume ratio confirmation.

use serde::{Deserialize, Serialize};

use crate::Bar;
use crate::ta::{atr, pivot_high, pivot_low, sma};

pub const ID: &str = "sweep";
pub const NAME: &str = "Liquidity Sweep";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SweepParams {
    /// Pivot swing period.
    pub piv_len: usize,
    /// Freshness limit for liquidity level in bars.
    pub lvl_fresh: usize,
    /// Minimum wick ratio to total candle range (default 0.5 = 50%).
    pub wick_ratio: f64,
    /// Lookback length for average volume.
    pub vol_len: usize,
    /// Minimum volume ratio on the sweep bar (e.g. 1.2x of MA volume).
    pub vol_sweep_min: f64,
    /// ATR period.
    pub atr_len: usize,
    /// Stop-loss buffer multiplier of ATR.
    pub sl_buf: f64,
    /// Fallback Risk:Reward ratio for take-profit.
    pub rr_fb: f64,
}

impl Default for SweepParams {
    fn default() -> Self {
        Self {
            piv_len: 14,
            lvl_fresh: 300,
            wick_ratio: 0.5,
            vol_len: 20,
            vol_sweep_min: 1.2,
            atr_len: 14,
            sl_buf: 0.5,
            rr_fb: 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SweepEventKind {
    SweepHighShort,
    SweepLowLong,
}

impl SweepEventKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::SweepHighShort => "SWEEP HIGH SHORT",
            Self::SweepLowLong => "SWEEP LOW LONG",
        }
    }

    pub const fn score(&self) -> f64 {
        match self {
            Self::SweepHighShort => -0.7,
            Self::SweepLowLong => 0.7,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepEvent {
    pub bar_index: usize,
    pub kind: SweepEventKind,
    pub score: f64,
    pub swept_level: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepSignal {
    pub bar_index: usize,
    pub direction: i32,
    pub entry: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SweepResult {
    pub events: Vec<SweepEvent>,
    pub signals: Vec<SweepSignal>,
    pub scores: Vec<f64>,
}

#[derive(Debug, Clone)]
struct ActiveLevel {
    level: f64,
    created_at_bar: usize,
    is_high: bool,
}

pub fn analyze_sweep(bars: &[Bar], params: &SweepParams) -> SweepResult {
    let n = bars.len();
    if n < params.piv_len.max(params.vol_len) {
        return SweepResult {
            events: Vec::new(),
            signals: Vec::new(),
            scores: vec![0.0; n],
        };
    }

    let mut scores = vec![0.0; n];
    let mut events = Vec::new();
    let mut signals = Vec::new();

    let highs: Vec<Option<f64>> = bars.iter().map(|b| Some(b.high)).collect();
    let lows: Vec<Option<f64>> = bars.iter().map(|b| Some(b.low)).collect();
    let volumes: Vec<Option<f64>> = bars.iter().map(|b| Some(b.volume)).collect();

    let ph_arr = pivot_high(&highs, params.piv_len, params.piv_len);
    let pl_arr = pivot_low(&lows, params.piv_len, params.piv_len);
    let atrs = atr(bars, params.atr_len);
    let vol_ma = sma(&volumes, params.vol_len);

    let mut levels: Vec<ActiveLevel> = Vec::new();

    for i in 1..n {
        let b = &bars[i];
        let a = atrs.get(i).copied().flatten();
        let v_avg = vol_ma.get(i).copied().flatten();

        // Register new confirmed swing high/low
        if let Some(ph) = ph_arr.get(i).copied().flatten() {
            levels.push(ActiveLevel {
                level: ph.value,
                created_at_bar: i,
                is_high: true,
            });
        }
        if let Some(pl) = pl_arr.get(i).copied().flatten() {
            levels.push(ActiveLevel {
                level: pl.value,
                created_at_bar: i,
                is_high: false,
            });
        }

        // Clean up levels older than lvl_fresh
        levels.retain(|lvl| i.saturating_sub(lvl.created_at_bar) <= params.lvl_fresh);

        let Some(atr_val) = a else { continue };
        let Some(avg_vol) = v_avg else { continue };

        let vol_ratio = if avg_vol > 0.0 {
            b.volume / avg_vol
        } else {
            1.0
        };
        let range = b.high - b.low;
        if range <= 0.0 {
            continue;
        }

        let up_wick = b.high - b.open.max(b.close);
        let dn_wick = b.open.min(b.close) - b.low;

        let mut record_event = |kind: SweepEventKind, lvl: f64| {
            let sc = kind.score();
            scores[i] = sc;
            events.push(SweepEvent {
                bar_index: i,
                kind,
                score: sc,
                swept_level: lvl,
            });
        };

        let mut record_plan = |dir: i32, sl: f64| {
            let entry = b.close;
            let risk = if dir == 1 { entry - sl } else { sl - entry };
            if risk > 0.0 {
                let tp = if dir == 1 {
                    entry + params.rr_fb * risk
                } else {
                    entry - params.rr_fb * risk
                };
                signals.push(SweepSignal {
                    bar_index: i,
                    direction: dir,
                    entry,
                    stop_loss: sl,
                    take_profit: tp,
                });
            }
        };

        // Sweep High (Liquidity Grab above swing high -> rejection short)
        if up_wick / range >= params.wick_ratio && vol_ratio >= params.vol_sweep_min {
            let mut swept_level = None;
            for lvl in levels.iter().filter(|l| l.is_high) {
                // High punctured the level but Close remained below the level
                if b.high > lvl.level && b.close < lvl.level {
                    swept_level = Some(lvl.level);
                    break;
                }
            }
            if let Some(lvl) = swept_level {
                record_event(SweepEventKind::SweepHighShort, lvl);
                record_plan(-1, b.high + params.sl_buf * atr_val);
            }
        }

        // Sweep Low (Liquidity Grab below swing low -> rejection long)
        if dn_wick / range >= params.wick_ratio && vol_ratio >= params.vol_sweep_min {
            let mut swept_level = None;
            for lvl in levels.iter().filter(|l| !l.is_high) {
                // Low punctured the level but Close remained above the level
                if b.low < lvl.level && b.close > lvl.level {
                    swept_level = Some(lvl.level);
                    break;
                }
            }
            if let Some(lvl) = swept_level {
                record_event(SweepEventKind::SweepLowLong, lvl);
                record_plan(1, b.low - params.sl_buf * atr_val);
            }
        }
    }

    SweepResult {
        events,
        signals,
        scores,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_bar(time: i64, o: f64, h: f64, l: f64, c: f64, vol: f64) -> Bar {
        Bar {
            time,
            open: o,
            high: h,
            low: l,
            close: c,
            volume: vol,
        }
    }

    #[test]
    fn detects_liquidity_sweep_low() {
        let mut bars = Vec::new();
        // Create baseline price around 100 with a swing low at 90 around bar 10
        for i in 0..10 {
            bars.push(mock_bar(i as i64 * 60, 100.0, 102.0, 98.0, 100.0, 100.0));
        }
        // Swing low: low drops to 90
        bars.push(mock_bar(10 * 60, 98.0, 99.0, 90.0, 96.0, 100.0));
        // Recover and build pivot confirmation (piv_len = 3)
        for i in 11..16 {
            bars.push(mock_bar(i as i64 * 60, 98.0, 101.0, 95.0, 100.0, 100.0));
        }
        // Sweep bar: dips to 88 (sweeps 90.0), but rejects aggressively to close at 95 with high volume
        bars.push(mock_bar(16 * 60, 94.0, 96.0, 88.0, 95.0, 500.0));

        let params = SweepParams {
            piv_len: 3,
            wick_ratio: 0.3,
            vol_sweep_min: 1.5,
            vol_len: 10,
            ..Default::default()
        };
        let res = analyze_sweep(&bars, &params);
        let sweep = res
            .events
            .iter()
            .find(|e| e.kind == SweepEventKind::SweepLowLong);
        assert!(sweep.is_some(), "expected sweep low long detection");
    }
}
