//! Deterministic WonderWhales method (Parabolic SAR + EMA trend alignment).
//!
//! Ported faithfully from `D:\Applications\tm-trading\engine\methods\wonderwhales.mjs`.

use serde::{Deserialize, Serialize};

use crate::Bar;
use crate::ta::{atr, ema, sar};

pub const ID: &str = "wonderwhales";
pub const NAME: &str = "WonderWhales (PSAR + EMA Trend)";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WonderWhalesMode {
    Sar,
    Ema,
    SarEma,
    #[default]
    SarEmaAtr,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WonderWhalesParams {
    pub mode: WonderWhalesMode,
    pub psar_start: f64,
    pub psar_inc: f64,
    pub psar_max: f64,
    pub fast: usize,
    pub slow: usize,
    pub atr_len: usize,
    pub min_atr_pct: f64,
    pub sl_buf: f64,
    pub rr_fb: f64,
}

impl Default for WonderWhalesParams {
    fn default() -> Self {
        Self {
            mode: WonderWhalesMode::default(),
            psar_start: 0.02,
            psar_inc: 0.02,
            psar_max: 0.2,
            fast: 20,
            slow: 50,
            atr_len: 14,
            min_atr_pct: 0.2,
            sl_buf: 0.5,
            rr_fb: 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WonderWhalesEventKind {
    SarFlipLong,
    SarFlipShort,
    EmaCrossLong,
    EmaCrossShort,
}

impl WonderWhalesEventKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::SarFlipLong => "SAR FLIP LONG",
            Self::SarFlipShort => "SAR FLIP SHORT",
            Self::EmaCrossLong => "EMA CROSS LONG",
            Self::EmaCrossShort => "EMA CROSS SHORT",
        }
    }

    pub const fn score(&self) -> f64 {
        match self {
            Self::SarFlipLong => 0.6,
            Self::SarFlipShort => -0.6,
            Self::EmaCrossLong => 0.5,
            Self::EmaCrossShort => -0.5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WonderWhalesEvent {
    pub bar_index: usize,
    pub kind: WonderWhalesEventKind,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WonderWhalesSignal {
    pub bar_index: usize,
    pub direction: i32,
    pub entry: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WonderWhalesResult {
    pub events: Vec<WonderWhalesEvent>,
    pub signals: Vec<WonderWhalesSignal>,
    pub scores: Vec<f64>,
}

pub fn analyze_wonderwhales(bars: &[Bar], params: &WonderWhalesParams) -> WonderWhalesResult {
    let n = bars.len();
    if n < params.slow.max(params.fast) {
        return WonderWhalesResult {
            events: Vec::new(),
            signals: Vec::new(),
            scores: vec![0.0; n],
        };
    }

    let mut scores = vec![0.0; n];
    let mut events = Vec::new();
    let mut signals = Vec::new();

    let closes: Vec<Option<f64>> = bars.iter().map(|b| Some(b.close)).collect();
    let fast_ma = ema(&closes, params.fast);
    let slow_ma = ema(&closes, params.slow);
    let atrs = atr(bars, params.atr_len);
    let sars = sar(bars, params.psar_start, params.psar_inc, params.psar_max);

    for i in 1..n {
        let b = &bars[i];
        let pv = &bars[i - 1];

        let (Some(sar_curr), Some(sar_prev)) = (
            sars.get(i).copied().flatten(),
            sars.get(i - 1).copied().flatten(),
        ) else {
            continue;
        };

        let atr_opt = atrs.get(i).copied().flatten();
        let fast_curr = fast_ma.get(i).copied().flatten();
        let slow_curr = slow_ma.get(i).copied().flatten();
        let fast_prev = fast_ma.get(i - 1).copied().flatten();
        let slow_prev = slow_ma.get(i - 1).copied().flatten();

        let mut record_event = |kind: WonderWhalesEventKind| {
            let sc = kind.score();
            scores[i] = sc;
            events.push(WonderWhalesEvent {
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
                signals.push(WonderWhalesSignal {
                    bar_index: i,
                    direction: dir,
                    entry,
                    stop_loss: sl,
                    take_profit: tp,
                });
            }
        };

        let atr_ok = match (params.mode, atr_opt) {
            (WonderWhalesMode::SarEmaAtr, Some(a)) => (a / b.close) * 100.0 >= params.min_atr_pct,
            (WonderWhalesMode::SarEmaAtr, None) => false,
            _ => true,
        };

        if !atr_ok {
            continue;
        }

        let atr_val = atr_opt.unwrap_or(0.0);

        // Check Parabolic SAR flips
        let sar_flip_long = sar_prev > pv.close && sar_curr < b.close;
        let sar_flip_short = sar_prev < pv.close && sar_curr > b.close;

        // Check EMA crossovers
        let ema_cross_long = match (fast_prev, slow_prev, fast_curr, slow_curr) {
            (Some(fp), Some(sp), Some(fc), Some(sc)) => fp <= sp && fc > sc,
            _ => false,
        };
        let ema_cross_short = match (fast_prev, slow_prev, fast_curr, slow_curr) {
            (Some(fp), Some(sp), Some(fc), Some(sc)) => fp >= sp && fc < sc,
            _ => false,
        };

        let ema_bull = match (fast_curr, slow_curr) {
            (Some(f), Some(s)) => f > s,
            _ => false,
        };
        let ema_bear = match (fast_curr, slow_curr) {
            (Some(f), Some(s)) => f < s,
            _ => false,
        };

        match params.mode {
            WonderWhalesMode::Sar => {
                if sar_flip_long {
                    record_event(WonderWhalesEventKind::SarFlipLong);
                    record_plan(1, sar_curr - params.sl_buf * atr_val);
                } else if sar_flip_short {
                    record_event(WonderWhalesEventKind::SarFlipShort);
                    record_plan(-1, sar_curr + params.sl_buf * atr_val);
                }
            }
            WonderWhalesMode::Ema => {
                if ema_cross_long {
                    record_event(WonderWhalesEventKind::EmaCrossLong);
                    record_plan(1, b.low - params.sl_buf * atr_val);
                } else if ema_cross_short {
                    record_event(WonderWhalesEventKind::EmaCrossShort);
                    record_plan(-1, b.high + params.sl_buf * atr_val);
                }
            }
            WonderWhalesMode::SarEma | WonderWhalesMode::SarEmaAtr => {
                if sar_flip_long && ema_bull {
                    record_event(WonderWhalesEventKind::SarFlipLong);
                    record_plan(1, sar_curr - params.sl_buf * atr_val);
                } else if sar_flip_short && ema_bear {
                    record_event(WonderWhalesEventKind::SarFlipShort);
                    record_plan(-1, sar_curr + params.sl_buf * atr_val);
                }
            }
        }
    }

    WonderWhalesResult {
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
    fn detects_wonderwhales_signals() {
        let mut bars = Vec::new();
        // Generate a downtrend then an aggressive reversal upwards
        for i in 0..60 {
            let p = 150.0 - (i as f64 * 0.5);
            bars.push(mock_bar(i as i64 * 60, p, p + 1.0, p - 1.0, p));
        }
        for i in 60..100 {
            let p = 120.0 + ((i - 60) as f64 * 1.5);
            bars.push(mock_bar(i as i64 * 60, p, p + 2.0, p - 0.5, p + 1.5));
        }

        let params = WonderWhalesParams {
            mode: WonderWhalesMode::Sar,
            fast: 10,
            slow: 20,
            atr_len: 14,
            min_atr_pct: 0.0,
            ..Default::default()
        };
        let res = analyze_wonderwhales(&bars, &params);
        assert!(
            !res.events.is_empty(),
            "expected SAR flip events on strong trend reversal"
        );
    }
}
