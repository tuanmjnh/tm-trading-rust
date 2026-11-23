//! Deterministic Price Action / Smart Money Concepts (SMC) research method.
//!
//! Ported faithfully from `D:\Applications\tm-trading\engine\methods\priceAction.mjs`.
//! Pure indicators and patterns over closed price bars:
//! - PIN bar (Bullish / Bearish Pin Bar rejection)
//! - Engulfing (Bullish / Bearish Engulfing pattern)
//! - Inside Bar (Contraction / pause)
//! - BOS (Break of Structure past most recent pivot high/low)

use serde::{Deserialize, Serialize};

use crate::Bar;
use crate::ta::{atr, pivot_high, pivot_low};

pub const ID: &str = "price-action";
pub const NAME: &str = "Price Action / SMC";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PriceActionParams {
    /// Minimum wick ratio to total range to classify as pin bar (default 0.6 = 60%).
    pub pin_wick: f64,
    /// Maximum body ratio to total range to classify as pin bar (default 0.4 = 40%).
    pub pin_body_max: f64,
    /// Enable engulfing pattern detection.
    pub eng_on: bool,
    /// Enable inside bar pattern detection.
    pub ins_on: bool,
    /// Enable break of structure (BOS) pattern detection.
    pub bos_on: bool,
    /// Pivot swing lookback left.
    pub piv_left: usize,
    /// Pivot swing lookback right.
    pub piv_right: usize,
    /// Period for Average True Range.
    pub atr_len: usize,
    /// Stop-loss ATR buffer beyond extreme of pattern.
    pub sl_buf: f64,
    /// Risk:Reward ratio for fallback take-profit calculation.
    pub rr_fb: f64,
}

impl Default for PriceActionParams {
    fn default() -> Self {
        Self {
            pin_wick: 0.6,
            pin_body_max: 0.4,
            eng_on: true,
            ins_on: true,
            bos_on: true,
            piv_left: 3,
            piv_right: 3,
            atr_len: 14,
            sl_buf: 0.5,
            rr_fb: 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PriceActionPatternKind {
    PinLong,
    PinShort,
    EngLong,
    EngShort,
    Inside,
    BosLong,
    BosShort,
}

impl PriceActionPatternKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::PinLong => "PIN LONG",
            Self::PinShort => "PIN SHORT",
            Self::EngLong => "ENG LONG",
            Self::EngShort => "ENG SHORT",
            Self::Inside => "INSIDE",
            Self::BosLong => "BOS LONG",
            Self::BosShort => "BOS SHORT",
        }
    }

    pub const fn score(&self) -> f64 {
        match self {
            Self::PinLong => 0.7,
            Self::PinShort => -0.7,
            Self::EngLong => 0.6,
            Self::EngShort => -0.6,
            Self::BosLong => 0.5,
            Self::BosShort => -0.5,
            Self::Inside => 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceActionEvent {
    pub bar_index: usize,
    pub kind: PriceActionPatternKind,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceActionSignal {
    pub bar_index: usize,
    pub direction: i32, // 1 for long, -1 for short
    pub entry: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceActionResult {
    pub events: Vec<PriceActionEvent>,
    pub signals: Vec<PriceActionSignal>,
    pub scores: Vec<f64>,
}

/// Pure deterministic analysis of price action on a sequence of closed bars.
pub fn analyze_price_action(bars: &[Bar], params: &PriceActionParams) -> PriceActionResult {
    let n = bars.len();
    if n < 2 {
        return PriceActionResult {
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
    let atrs = atr(bars, params.atr_len);

    let ph_arr = pivot_high(&highs, params.piv_left, params.piv_right);
    let pl_arr = pivot_low(&lows, params.piv_left, params.piv_right);

    let mut last_ph: Option<f64> = None;
    let mut last_pl: Option<f64> = None;

    for i in 1..n {
        let b = &bars[i];
        let pv = &bars[i - 1];
        let a = atrs.get(i).copied().flatten();
        let range = b.high - b.low;
        let body = (b.close - b.open).abs();
        let up_w = b.high - b.open.max(b.close);
        let dn_w = b.open.min(b.close) - b.low;

        if let Some(ph) = ph_arr.get(i).copied().flatten() {
            last_ph = Some(ph.value);
        }
        if let Some(pl) = pl_arr.get(i).copied().flatten() {
            last_pl = Some(pl.value);
        }

        let mut record_event = |kind: PriceActionPatternKind| {
            let sc = kind.score();
            scores[i] = sc;
            events.push(PriceActionEvent {
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
                signals.push(PriceActionSignal {
                    bar_index: i,
                    direction: dir,
                    entry,
                    stop_loss: sl,
                    take_profit: tp,
                });
            }
        };

        // --- PIN bar ---
        if let Some(atr_val) = a
            && range > 0.0
        {
            let body_ratio = body / range;
            if body_ratio <= params.pin_body_max {
                if dn_w / range >= params.pin_wick && b.close > b.low + range * 0.5 {
                    record_event(PriceActionPatternKind::PinLong);
                    record_plan(1, b.low - params.sl_buf * atr_val);
                } else if up_w / range >= params.pin_wick && b.close < b.high - range * 0.5 {
                    record_event(PriceActionPatternKind::PinShort);
                    record_plan(-1, b.high + params.sl_buf * atr_val);
                }
            }
        }

        // --- Engulfing ---
        if params.eng_on
            && let Some(atr_val) = a
        {
            let pv_bear = pv.close < pv.open;
            let pv_bull = pv.close > pv.open;
            let bull = b.close > b.open;
            let bear = b.close < b.open;
            let body_prev = (pv.close - pv.open).abs();

            if pv_bear && bull && b.open <= pv.close && b.close >= pv.open && body >= body_prev {
                record_event(PriceActionPatternKind::EngLong);
                record_plan(1, b.open.min(b.low) - params.sl_buf * atr_val);
            } else if pv_bull
                && bear
                && b.open >= pv.close
                && b.close <= pv.open
                && body >= body_prev
            {
                record_event(PriceActionPatternKind::EngShort);
                record_plan(-1, b.open.max(b.high) + params.sl_buf * atr_val);
            }
        }

        // --- Inside Bar ---
        if params.ins_on && b.high <= pv.high && b.low >= pv.low {
            record_event(PriceActionPatternKind::Inside);
        }

        // --- BOS: Break of Structure past previous confirmed swing ---
        if params.bos_on
            && let Some(atr_val) = a
        {
            if let Some(ph) = last_ph
                && b.close > ph
                && pv.close <= ph
            {
                record_event(PriceActionPatternKind::BosLong);
                record_plan(1, ph - params.sl_buf * atr_val);
                last_ph = None; // consumed until next confirmed pivot
            }
            if let Some(pl) = last_pl
                && b.close < pl
                && pv.close >= pl
            {
                record_event(PriceActionPatternKind::BosShort);
                record_plan(-1, pl + params.sl_buf * atr_val);
                last_pl = None;
            }
        }
    }

    PriceActionResult {
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
    fn detects_bullish_pin_bar() {
        let mut bars = Vec::new();
        for i in 0..20 {
            bars.push(mock_bar(i as i64 * 60, 100.0, 102.0, 98.0, 100.5));
        }
        // Insert a bullish hammer/pin bar at bar 20: low drops to 90, closes at 99.5 near high 100
        bars.push(mock_bar(20 * 60, 99.0, 100.0, 90.0, 99.5));

        let res = analyze_price_action(&bars, &PriceActionParams::default());
        let pin = res
            .events
            .iter()
            .find(|e| e.kind == PriceActionPatternKind::PinLong);
        assert!(pin.is_some(), "expected bullish pin bar detection");
    }

    #[test]
    fn detects_inside_bar() {
        let mut bars = Vec::new();
        for i in 0..10 {
            bars.push(mock_bar(i as i64 * 60, 100.0, 105.0, 95.0, 100.0));
        }
        // Next bar is inside previous range
        bars.push(mock_bar(10 * 60, 100.0, 102.0, 98.0, 101.0));

        let res = analyze_price_action(&bars, &PriceActionParams::default());
        let inside = res
            .events
            .iter()
            .find(|e| e.kind == PriceActionPatternKind::Inside);
        assert!(inside.is_some(), "expected inside bar event");
    }
}
