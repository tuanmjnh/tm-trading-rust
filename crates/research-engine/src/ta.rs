//! Pine-compatible technical-analysis primitives.
//!
//! These port `engine/ta.mjs` from the legacy engine. Each function keeps the
//! legacy warmup rule, so `None` is Pine `na` and appears exactly where the
//! legacy implementation returns `null`. Nothing is smoothed, seeded, or
//! clamped "more sensibly": parity with the approved fixtures is the contract.

use crate::Bar;

/// Simple moving average. `na` until `length` finite values are available.
#[must_use]
pub fn sma(values: &[Option<f64>], length: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if length == 0 {
        return out;
    }
    let mut sum = 0.0;
    let mut missing = 0_usize;
    for (i, value) in values.iter().enumerate() {
        match value {
            Some(value) if value.is_finite() => sum += value,
            _ => missing += 1,
        }
        if i >= length {
            match values[i - length] {
                Some(value) if value.is_finite() => sum -= value,
                _ => missing -= 1,
            }
        }
        if i + 1 >= length && missing == 0 {
            out[i] = Some(sum / length as f64);
        }
    }
    out
}

/// Wilder moving average, seeded with `sma(length)` exactly like Pine's `ta.rma`.
#[must_use]
pub fn rma(values: &[Option<f64>], length: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if length == 0 {
        return out;
    }
    let seed = sma(values, length);
    let alpha = 1.0 / length as f64;
    for i in length - 1..values.len() {
        let Some(value) = values[i].filter(|value| value.is_finite()) else {
            continue;
        };
        let previous = i.checked_sub(1).and_then(|previous| out[previous]);
        out[i] = match previous {
            Some(previous) => Some(alpha * value + (1.0 - alpha) * previous),
            // A `na` in the seed window keeps the value `na`, as the legacy
            // implementation does; the budget restarts once the window is full.
            None => seed[i],
        };
    }
    out
}

/// Exponential moving average seeded from the first finite value, as Pine `ta.ema` does.
#[must_use]
pub fn ema(values: &[Option<f64>], length: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if length == 0 {
        return out;
    }
    let alpha = 2.0 / (length as f64 + 1.0);
    let mut previous: Option<f64> = None;
    for (i, value) in values.iter().enumerate() {
        let Some(value) = value.filter(|value| value.is_finite()) else {
            continue;
        };
        let next = previous.map_or(value, |previous| alpha * value + (1.0 - alpha) * previous);
        out[i] = Some(next);
        previous = Some(next);
    }
    out
}

/// `ta.tr`: the first bar has no previous close, so its range is `high - low`.
#[must_use]
pub fn true_range(bars: &[Bar]) -> Vec<f64> {
    bars.iter()
        .enumerate()
        .map(
            |(i, bar)| match i.checked_sub(1).map(|previous| bars[previous]) {
                Some(previous) => (bar.high - bar.low)
                    .max((bar.high - previous.close).abs())
                    .max((bar.low - previous.close).abs()),
                None => bar.high - bar.low,
            },
        )
        .collect()
}

/// Average true range: `rma(true_range)`.
#[must_use]
pub fn atr(bars: &[Bar], length: usize) -> Vec<Option<f64>> {
    let ranges: Vec<Option<f64>> = true_range(bars).into_iter().map(Some).collect();
    rma(&ranges, length)
}

/// `math.sum`: `na` until `length` values are available.
///
/// A missing value counts as `0`, matching the JavaScript accumulator this ports.
#[must_use]
pub fn rolling_sum(values: &[Option<f64>], length: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if length == 0 {
        return out;
    }
    let numeric: Vec<f64> = values.iter().map(|value| value.unwrap_or(0.0)).collect();
    let mut sum = 0.0;
    for (i, value) in numeric.iter().enumerate() {
        sum += value;
        if i >= length {
            sum -= numeric[i - length];
        }
        if i + 1 >= length {
            out[i] = Some(sum);
        }
    }
    out
}

/// Wilder RSI. A flat window is `na` (0/0), while an all-gain window is `100` (x/0).
#[must_use]
pub fn rsi(values: &[Option<f64>], length: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if length == 0 {
        return out;
    }
    let mut gains = vec![None; values.len()];
    let mut losses = vec![None; values.len()];
    for i in 1..values.len() {
        let (Some(close), Some(previous)) = (
            values[i].filter(|value| value.is_finite()),
            values[i - 1].filter(|value| value.is_finite()),
        ) else {
            continue;
        };
        let change = close - previous;
        gains[i] = Some(if change > 0.0 { change } else { 0.0 });
        losses[i] = Some(if change < 0.0 { -change } else { 0.0 });
    }
    let average_gain = rma(&gains, length);
    let average_loss = rma(&losses, length);
    for i in 0..values.len() {
        let (Some(gain), Some(loss)) = (average_gain[i], average_loss[i]) else {
            continue;
        };
        let ratio = if loss == 0.0 {
            if gain == 0.0 { f64::NAN } else { f64::INFINITY }
        } else {
            gain / loss
        };
        out[i] = if ratio.is_finite() {
            Some(100.0 - 100.0 / (1.0 + ratio))
        } else if ratio == f64::INFINITY {
            Some(100.0)
        } else {
            None
        };
    }
    out
}

/// Population standard deviation over a strict window; any `na` inside it yields `na`.
#[must_use]
pub fn stdev(values: &[Option<f64>], length: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if length == 0 {
        return out;
    }
    for i in length - 1..values.len() {
        let window = &values[i + 1 - length..=i];
        let mut sum = 0.0;
        let mut complete = true;
        for value in window {
            let Some(value) = value.filter(|value| value.is_finite()) else {
                complete = false;
                break;
            };
            sum += value;
        }
        if !complete {
            continue;
        }
        let mean = sum / length as f64;
        let variance = window
            .iter()
            .map(|value| {
                let difference = value.unwrap_or(mean) - mean;
                difference * difference
            })
            .sum::<f64>()
            / length as f64;
        out[i] = Some(variance.sqrt());
    }
    out
}

/// The three MACD series, all aligned to the input.
#[derive(Debug, Clone, PartialEq)]
pub struct Macd {
    pub macd: Vec<Option<f64>>,
    pub signal: Vec<Option<f64>>,
    pub hist: Vec<Option<f64>>,
}

/// `ema(fast) - ema(slow)`, its `ema(signal)`, and the difference.
#[must_use]
pub fn macd(values: &[Option<f64>], fast: usize, slow: usize, signal: usize) -> Macd {
    let fast_ema = ema(values, fast);
    let slow_ema = ema(values, slow);
    let line: Vec<Option<f64>> = fast_ema
        .iter()
        .zip(&slow_ema)
        .map(|(fast, slow)| match (fast, slow) {
            (Some(fast), Some(slow)) => Some(fast - slow),
            _ => None,
        })
        .collect();
    let signal_line = ema(&line, signal);
    let hist = line
        .iter()
        .zip(&signal_line)
        .map(|(line, signal)| match (line, signal) {
            (Some(line), Some(signal)) => Some(line - signal),
            _ => None,
        })
        .collect();
    Macd {
        macd: line,
        signal: signal_line,
        hist,
    }
}

/// Anchored typical-price VWAP. A bar with an invalid field outputs `na` and
/// leaves the accumulator untouched, matching the legacy implementation.
#[must_use]
pub fn vwap(bars: &[Bar]) -> Vec<Option<f64>> {
    let mut out = vec![None; bars.len()];
    let mut cumulative_price_volume = 0.0;
    let mut cumulative_volume = 0.0;
    for (i, bar) in bars.iter().enumerate() {
        if !bar.volume.is_finite()
            || bar.volume < 0.0
            || !bar.high.is_finite()
            || !bar.low.is_finite()
            || !bar.close.is_finite()
        {
            continue;
        }
        let typical = (bar.high + bar.low + bar.close) / 3.0;
        cumulative_price_volume += typical * bar.volume;
        cumulative_volume += bar.volume;
        if cumulative_volume > 0.0 {
            out[i] = Some(cumulative_price_volume / cumulative_volume);
        }
    }
    out
}

/// On-balance volume, starting at `0`, moved by volume in the close's direction.
#[must_use]
pub fn obv(bars: &[Bar]) -> Vec<Option<f64>> {
    let mut out = vec![None; bars.len()];
    let mut accumulator = 0.0;
    for (i, bar) in bars.iter().enumerate() {
        if !bar.close.is_finite() || !bar.volume.is_finite() {
            continue;
        }
        if let Some(previous) = i.checked_sub(1).map(|previous| bars[previous].close)
            && previous.is_finite()
        {
            if bar.close > previous {
                accumulator += bar.volume;
            } else if bar.close < previous {
                accumulator -= bar.volume;
            }
        }
        out[i] = Some(accumulator);
    }
    out
}

/// Chaikin money flow: `sum(money_flow_volume) / sum(volume)` over `length` bars.
#[must_use]
pub fn cmf(bars: &[Bar], length: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; bars.len()];
    if length == 0 {
        return out;
    }
    let money_flow: Vec<Option<f64>> = bars
        .iter()
        .map(|bar| {
            let range = bar.high - bar.low;
            if !range.is_finite()
                || range <= 0.0
                || !bar.close.is_finite()
                || !bar.volume.is_finite()
            {
                return None;
            }
            let multiplier = ((bar.close - bar.low) - (bar.high - bar.close)) / range;
            Some(multiplier * bar.volume)
        })
        .collect();
    for (i, slot) in out.iter_mut().enumerate().skip(length - 1) {
        let mut flow_sum = 0.0;
        let mut volume_sum = 0.0;
        let mut complete = true;
        for k in i + 1 - length..=i {
            let (Some(flow), volume) = (money_flow[k], bars[k].volume) else {
                complete = false;
                break;
            };
            if !volume.is_finite() {
                complete = false;
                break;
            }
            flow_sum += flow;
            volume_sum += volume;
        }
        if complete && volume_sum != 0.0 {
            *slot = Some(flow_sum / volume_sum);
        }
    }
    out
}

/// Donchian channel from `highest(high)` and `lowest(low)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Donchian {
    pub upper: Vec<Option<f64>>,
    pub lower: Vec<Option<f64>>,
    pub middle: Vec<Option<f64>>,
}

/// Rolling high/low channel over `length` bars.
#[must_use]
pub fn donchian(bars: &[Bar], length: usize) -> Donchian {
    let highs: Vec<Option<f64>> = bars.iter().map(|bar| Some(bar.high)).collect();
    let lows: Vec<Option<f64>> = bars.iter().map(|bar| Some(bar.low)).collect();
    let upper = highest(&highs, length);
    let lower = lowest(&lows, length);
    let middle = upper
        .iter()
        .zip(&lower)
        .map(|(upper, lower)| match (upper, lower) {
            (Some(upper), Some(lower)) => Some((upper + lower) / 2.0),
            _ => None,
        })
        .collect();
    Donchian {
        upper,
        lower,
        middle,
    }
}

/// A confirmed pivot: the value and the bar it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pivot {
    pub value: f64,
    pub index: usize,
}

/// Pivot low confirmed at `pivot_index + right`; nothing is known earlier.
#[must_use]
pub fn pivot_low(values: &[Option<f64>], left: usize, right: usize) -> Vec<Option<Pivot>> {
    pivot(values, left, right, true)
}

/// Pivot high confirmed at `pivot_index + right`; nothing is known earlier.
#[must_use]
pub fn pivot_high(values: &[Option<f64>], left: usize, right: usize) -> Vec<Option<Pivot>> {
    pivot(values, left, right, false)
}

fn pivot(values: &[Option<f64>], left: usize, right: usize, low: bool) -> Vec<Option<Pivot>> {
    let mut out = vec![None; values.len()];
    for (i, slot) in out.iter_mut().enumerate().skip(right) {
        let Some(center) = i.checked_sub(right) else {
            continue;
        };
        if center < left {
            continue;
        }
        let Some(value) = values[center].filter(|value| value.is_finite()) else {
            continue;
        };
        let window = &values[center - left..=center + right];
        let mut confirmed = true;
        for (offset, other) in window.iter().enumerate() {
            if center - left + offset == center {
                continue;
            }
            let Some(other) = other.filter(|other| other.is_finite()) else {
                confirmed = false;
                break;
            };
            let is_pivot = if low { other > value } else { other < value };
            if !is_pivot {
                confirmed = false;
                break;
            }
        }
        if confirmed {
            *slot = Some(Pivot {
                value,
                index: center,
            });
        }
    }
    out
}

/// Rolling minimum over `length` bars.
#[must_use]
pub fn lowest(values: &[Option<f64>], length: usize) -> Vec<Option<f64>> {
    extreme(values, length, f64::min)
}

/// Rolling maximum over `length` bars.
#[must_use]
pub fn highest(values: &[Option<f64>], length: usize) -> Vec<Option<f64>> {
    extreme(values, length, f64::max)
}

fn extreme(values: &[Option<f64>], length: usize, pick: fn(f64, f64) -> f64) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if length == 0 {
        return out;
    }
    for (i, slot) in out.iter_mut().enumerate().skip(length - 1) {
        let mut current: Option<f64> = None;
        for value in &values[i + 1 - length..=i] {
            // shortcut: an `na` inside the window yields `na`, while the legacy
            // `Math.min`/`Math.max` coerces it to 0; not reachable from stored
            // candles, upgrade if fixtures ever feed missing highs/lows.
            let Some(value) = value.filter(|value| value.is_finite()) else {
                current = None;
                break;
            };
            current = Some(current.map_or(value, |current| pick(current, value)));
        }
        *slot = current;
    }
    out
}
