//! Deterministic Trend (MA Pullback, Donchian Breakout, Range Mean Reversion) method.
//!
//! Ported faithfully from `D:\Applications\tm-trading\engine\methods\trend.mjs`.
//! Analyzes moving average alignment, Donchian channel breakouts, and consolidation ranges.

use serde::{Deserialize, Serialize};

use crate::Bar;
use crate::ta::{atr, sma};

pub const ID: &str = "trend";
pub const NAME: &str = "Trend (Pullback / Breakout / Range)";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TrendParams {
    /// Fast SMA period (default 20).
    pub fast: usize,
    /// Slow SMA period (default 50).
    pub slow: usize,
    /// Enable pullback detection.
    pub pull_on: bool,
    /// Maximum distance (as fraction of ATR) to consider price touching the fast SMA.
    pub pull_dist: f64,
    /// Enable breakout detection.
    pub brk_on: bool,
    /// Donchian breakout lookback window.
    pub brk_len: usize,
    /// Enable range mean-reversion detection.
    pub rng_on: bool,
    /// Window for range determination.
    pub rng_len: usize,
    /// Bandwidth limit as multiple of ATR to classify as consolidation box.
    pub rng_band: f64,
    /// Threshold difference between fast and slow SMA as multiple of ATR to consider market flat.
    pub rng_atr_flat: f64,
    /// ATR period.
    pub atr_len: usize,
    /// Stop-loss buffer multiplier of ATR.
    pub sl_buf: f64,
    /// Fallback Risk:Reward ratio for take-profit calculation.
    pub rr_fb: f64,
}

impl Default for TrendParams {
    fn default() -> Self {
        Self {
            fast: 20,
            slow: 50,
            pull_on: true,
            pull_dist: 0.35,
            brk_on: true,
            brk_len: 20,
            rng_on: true,
            rng_len: 50,
            rng_band: 2.0,
            rng_atr_flat: 1.0,
            atr_len: 14,
            sl_buf: 0.5,
            rr_fb: 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrendEventKind {
    PullLong,
    PullShort,
    BrkLong,
    BrkShort,
    RangeLong,
    RangeShort,
}

impl TrendEventKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::PullLong => "PULL LONG",
            Self::PullShort => "PULL SHORT",
            Self::BrkLong => "BRK LONG",
            Self::BrkShort => "BRK SHORT",
            Self::RangeLong => "RANGE LONG",
            Self::RangeShort => "RANGE SHORT",
        }
    }

    pub const fn score(&self) -> f64 {
        match self {
            Self::PullLong => 0.6,
            Self::PullShort => -0.6,
            Self::BrkLong => 0.7,
            Self::BrkShort => -0.7,
            Self::RangeLong => 0.4,
            Self::RangeShort => -0.4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrendEvent {
    pub bar_index: usize,
    pub kind: TrendEventKind,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrendSignal {
    pub bar_index: usize,
    pub direction: i32,
    pub entry: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrendResult {
    pub events: Vec<TrendEvent>,
    pub signals: Vec<TrendSignal>,
    pub scores: Vec<f64>,
}

pub fn analyze_trend(bars: &[Bar], params: &TrendParams) -> TrendResult {
    let n = bars.len();
    if n < params.slow.max(params.brk_len) {
        return TrendResult {
            events: Vec::new(),
            signals: Vec::new(),
            scores: vec![0.0; n],
        };
    }

    let mut scores = vec![0.0; n];
    let mut events = Vec::new();
    let mut signals = Vec::new();

    let closes: Vec<Option<f64>> = bars.iter().map(|b| Some(b.close)).collect();
    let fast_ma = sma(&closes, params.fast);
    let slow_ma = sma(&closes, params.slow);
    let atrs = atr(bars, params.atr_len);

    for i in 1..n {
        let b = &bars[i];
        let pv = &bars[i - 1];
        let (Some(f_val), Some(s_val), Some(atr_val)) = (
            fast_ma.get(i).copied().flatten(),
            slow_ma.get(i).copied().flatten(),
            atrs.get(i).copied().flatten(),
        ) else {
            continue;
        };

        let mut record_event = |kind: TrendEventKind| {
            let sc = kind.score();
            scores[i] = sc;
            events.push(TrendEvent {
                bar_index: i,
                kind,
                score: sc,
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
                signals.push(TrendSignal {
                    bar_index: i,
                    direction: dir,
                    entry,
                    stop_loss: sl,
                    take_profit: tp,
                });
            }
        };

        // --- PULLBACK ---
        if params.pull_on {
            let dist = params.pull_dist * atr_val;
            // Bullish pullback: Fast MA > Slow MA, low touched Fast MA zone, closed above Fast MA
            if f_val > s_val && b.low <= f_val + dist && b.close >= f_val && pv.close >= f_val {
                record_event(TrendEventKind::PullLong);
                record_plan(1, b.low.min(f_val) - params.sl_buf * atr_val);
            } else if f_val < s_val
                && b.high >= f_val - dist
                && b.close <= f_val
                && pv.close <= f_val
            {
                record_event(TrendEventKind::PullShort);
                record_plan(-1, b.high.max(f_val) + params.sl_buf * atr_val);
            }
        }

        // --- BREAKOUT (Donchian channel of past brk_len bars) ---
        if params.brk_on && i >= params.brk_len {
            let start = i - params.brk_len;
            let highest_prev = bars[start..i]
                .iter()
                .map(|bar| bar.high)
                .fold(f64::MIN, f64::max);
            let lowest_prev = bars[start..i]
                .iter()
                .map(|bar| bar.low)
                .fold(f64::MAX, f64::min);

            if b.close > highest_prev && pv.close <= highest_prev {
                record_event(TrendEventKind::BrkLong);
                record_plan(1, highest_prev - params.sl_buf * atr_val);
            } else if b.close < lowest_prev && pv.close >= lowest_prev {
                record_event(TrendEventKind::BrkShort);
                record_plan(-1, lowest_prev + params.sl_buf * atr_val);
            }
        }

        // --- CONSOLIDATION RANGE ---
        if params.rng_on && i >= params.rng_len {
            let diff = (f_val - s_val).abs();
            if diff <= params.rng_atr_flat * atr_val {
                let start = i - params.rng_len;
                let box_high = bars[start..i]
                    .iter()
                    .map(|bar| bar.high)
                    .fold(f64::MIN, f64::max);
                let box_low = bars[start..i]
                    .iter()
                    .map(|bar| bar.low)
                    .fold(f64::MAX, f64::min);
                let bandwidth = box_high - box_low;

                if bandwidth <= params.rng_band * atr_val {
                    // Mean-reversion inside box
                    if b.low <= box_low + 0.1 * bandwidth && b.close > b.open {
                        record_event(TrendEventKind::RangeLong);
                        record_plan(1, box_low - params.sl_buf * atr_val);
                    } else if b.high >= box_high - 0.1 * bandwidth && b.close < b.open {
                        record_event(TrendEventKind::RangeShort);
                        record_plan(-1, box_high + params.sl_buf * atr_val);
                    }
                }
            }
        }
    }

    TrendResult {
        events,
        signals,
        scores,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_bar(time: i64, o: f64, h: f64, l: f64, c: f64) -> Bar {
        Bar {
            time,
            open: o,
            high: h,
            low: l,
            close: c,
            volume: 100.0,
        }
    }

    #[test]
    fn detects_donchian_breakout() {
        let mut bars = Vec::new();
        // Generate 60 bars in tight range 100..102
        for i in 0..60 {
            bars.push(mock_bar(i as i64 * 60, 100.0, 102.0, 99.0, 101.0));
        }
        // Bar 60 breaks out sharply to 110
        bars.push(mock_bar(60 * 60, 101.0, 110.0, 100.5, 109.5));

        let res = analyze_trend(&bars, &TrendParams::default());
        let brk = res
            .events
            .iter()
            .find(|e| e.kind == TrendEventKind::BrkLong);
        assert!(brk.is_some(), "expected breakout long detection");
    }
}
