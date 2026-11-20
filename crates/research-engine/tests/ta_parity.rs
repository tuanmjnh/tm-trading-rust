//! Goldens for the ported technical-analysis primitives.
//!
//! The hand-computed cases are copied from the legacy `engine/test.mjs`, where
//! each expected value is derived from the Pine Script definition. The
//! differential fixture is generated from `engine/ta.mjs` itself by
//! `fixtures/generate_ta_vectors.mjs`, so a porting mistake that still looks
//! plausible fails here.

use serde_json::Value;
use tm_trading_research_engine::Bar;
use tm_trading_research_engine::ta;

fn series(values: &[f64]) -> Vec<Option<f64>> {
    values.iter().copied().map(Some).collect()
}

fn closed(value: Option<f64>, expected: f64) {
    let value = value.expect("expected a value");
    assert!((value - expected).abs() < 1e-9, "{value} != {expected}");
}

fn bars(rows: &[(f64, f64, f64, f64, f64)]) -> Vec<Bar> {
    rows.iter()
        .map(|&(open, high, low, close, volume)| Bar {
            open,
            high,
            low,
            close,
            volume,
        })
        .collect()
}

#[test]
fn sma_is_na_until_the_window_is_full() {
    let sma = ta::sma(&series(&[1.0, 2.0, 3.0, 4.0, 5.0]), 3);
    assert_eq!(&sma[..2], &[None, None]);
    closed(sma[2], 2.0);
    closed(sma[3], 3.0);
    closed(sma[4], 4.0);
}

#[test]
fn rma_seeds_from_the_first_full_sma_window() {
    // Pine: sum := na(sum[1]) ? ta.sma(src, length) : alpha * src + (1 - alpha) * nz(sum[1])
    let rma = ta::rma(&series(&[10.0, 12.0, 11.0, 13.0, 14.0, 12.0, 15.0]), 3);
    assert_eq!(&rma[..2], &[None, None]);
    closed(rma[2], 11.0);
    closed(rma[3], 35.0 / 3.0);
    closed(rma[4], 112.0 / 9.0);
}

#[test]
fn rma_keeps_na_when_the_seed_window_is_incomplete() {
    // Legacy behavior: a missing seed value stays `na`, it is not replaced by
    // the current value, and the next full window seeds the recursion again.
    let rma = ta::rma(&[None, Some(5.0), Some(6.0), Some(7.0)], 3);
    assert_eq!(rma, vec![None, None, None, Some(6.0)]);
}

#[test]
fn true_range_and_atr_use_the_first_bar_range() {
    let bars = bars(&[
        (10.0, 12.0, 9.0, 11.0, 1.0),
        (11.0, 13.0, 10.0, 12.0, 1.0),
        (12.0, 14.0, 11.0, 13.0, 1.0),
    ]);
    let range = ta::true_range(&bars);
    assert_eq!(range.len(), 3);
    for value in range {
        closed(Some(value), 3.0);
    }
    closed(ta::atr(&bars, 3)[2], 3.0);
}

#[test]
fn rolling_sum_is_na_before_the_window_is_full() {
    let sum = ta::rolling_sum(&series(&[1.0, 2.0, 3.0, 4.0]), 2);
    assert_eq!(sum[0], None);
    closed(sum[1], 3.0);
    closed(sum[3], 7.0);
}

#[test]
fn pivots_are_confirmed_only_after_the_right_bars() {
    let lows = series(&[5.0, 3.0, 7.0, 9.0, 8.0]);
    let low = ta::pivot_low(&lows, 1, 1);
    assert_eq!(&low[..2], &[None, None]);
    let pivot = low[2].expect("pivot confirmed at i = 2");
    assert_eq!(pivot.value, 3.0);
    assert_eq!(pivot.index, 1);
    assert_eq!(&low[3..], &[None, None]);

    let highs = series(&[5.0, 9.0, 4.0, 7.0, 6.0]);
    let high = ta::pivot_high(&highs, 1, 1);
    let pivot = high[2].expect("pivot confirmed at i = 2");
    assert_eq!(pivot.value, 9.0);
    assert_eq!(pivot.index, 1);

    // Ties are not pivots: Pine requires strictly greater neighbors.
    let flat = series(&[4.0, 4.0, 4.0, 4.0, 4.0]);
    assert!(ta::pivot_low(&flat, 1, 1).iter().all(Option::is_none));

    // A window wider than the array yields no pivot instead of panicking.
    let short = series(&[1.0, 2.0, 3.0]);
    assert!(ta::pivot_low(&short, 10, 10).iter().all(Option::is_none));
}

#[test]
fn rsi_matches_the_wilder_zigzag_fixture() {
    let rsi = ta::rsi(&series(&[10.0, 11.0, 10.0, 11.0, 10.0]), 2);
    assert_eq!(&rsi[..2], &[None, None]);
    closed(rsi[2], 50.0);
    closed(rsi[3], 75.0);
    closed(rsi[4], 37.5);

    let flat = ta::rsi(&series(&[10.0; 5]), 2);
    assert_eq!(flat[4], None, "0/0 is na");

    let up = ta::rsi(&series(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), 2);
    closed(up[5], 100.0);
}

#[test]
fn stdev_is_population_and_rejects_na_in_the_window() {
    let stdev = ta::stdev(&series(&[1.0, 2.0, 3.0, 4.0]), 4);
    assert_eq!(&stdev[..3], &[None, None, None]);
    closed(stdev[3], 1.25_f64.sqrt());

    closed(ta::stdev(&series(&[3.0; 4]), 4)[3], 0.0);

    let with_na = ta::stdev(&[Some(1.0), None, Some(3.0), Some(4.0)], 3);
    assert_eq!(with_na[3], None);
}

#[test]
fn macd_builds_all_three_aligned_series() {
    let macd = ta::macd(&series(&[1.0, 2.0, 3.0, 4.0, 5.0]), 2, 3, 2);
    assert_eq!(macd.macd.len(), 5);
    assert_eq!(macd.signal.len(), 5);
    assert_eq!(macd.hist.len(), 5);

    closed(macd.macd[0], 0.0);
    closed(macd.macd[1], 1.0 / 6.0);
    closed(macd.signal[1], 1.0 / 9.0);
    for i in 0..5 {
        match (macd.macd[i], macd.signal[i], macd.hist[i]) {
            (Some(line), Some(signal), Some(hist)) => closed(Some(hist), line - signal),
            _ => assert!(macd.hist[i].is_none(), "hist[{i}] must be na"),
        }
    }
}

#[test]
fn vwap_is_anchored_at_the_first_bar() {
    let bars = bars(&[(9.0, 10.0, 8.0, 9.0, 100.0), (10.0, 12.0, 9.0, 10.0, 300.0)]);
    let vwap = ta::vwap(&bars);
    closed(vwap[0], 9.0);
    closed(vwap[1], 10.0);
}

#[test]
fn obv_starts_at_zero_and_follows_close_direction() {
    let bars = bars(&[
        (10.0, 11.0, 9.0, 10.0, 5.0),
        (10.0, 12.0, 9.0, 11.0, 3.0),
        (11.0, 11.0, 9.0, 10.0, 7.0),
        (10.0, 11.0, 9.0, 10.0, 2.0),
    ]);
    let obv = ta::obv(&bars);
    closed(obv[0], 0.0);
    closed(obv[1], 3.0);
    closed(obv[2], -4.0);
    closed(obv[3], -4.0);
}

#[test]
fn cmf_is_money_flow_over_volume() {
    let ticks = bars(&[(9.0, 10.0, 8.0, 10.0, 10.0), (9.0, 10.0, 8.0, 8.0, 20.0)]);
    let cmf = ta::cmf(&ticks, 2);
    assert_eq!(cmf[0], None);
    closed(cmf[1], -1.0 / 3.0);

    let degenerate = bars(&[(9.0, 9.0, 9.0, 9.0, 5.0), (9.0, 9.0, 9.0, 9.0, 5.0)]);
    assert_eq!(ta::cmf(&degenerate, 2)[1], None);
}

#[test]
fn donchian_uses_rolling_highs_and_lows() {
    let bars = bars(&[
        (9.0, 10.0, 8.0, 9.0, 1.0),
        (10.0, 12.0, 9.0, 11.0, 1.0),
        (11.0, 11.0, 7.0, 8.0, 1.0),
        (8.0, 14.0, 10.0, 13.0, 1.0),
    ]);
    let donchian = ta::donchian(&bars, 2);
    assert_eq!(donchian.upper[0], None);
    assert_eq!(donchian.lower[0], None);
    closed(donchian.upper[1], 12.0);
    closed(donchian.lower[1], 8.0);
    closed(donchian.middle[1], 10.0);
    closed(donchian.upper[3], 14.0);
}

const VECTORS: &str = include_str!("fixtures/ta_vectors.json");

fn fixture_series(fixture: &Value, path: &[&str]) -> Vec<Option<f64>> {
    let mut node = fixture;
    for key in path {
        node = node.get(*key).expect("fixture key");
    }
    node.as_array()
        .expect("fixture series")
        .iter()
        .map(Value::as_f64)
        .collect()
}

fn assert_series(name: &str, actual: &[Option<f64>], expected: &[Option<f64>]) {
    assert_eq!(actual.len(), expected.len(), "{name}: length");
    for (i, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        match (actual, expected) {
            (None, None) => {}
            (Some(actual), Some(expected)) => {
                let tolerance = 1e-9 * (1.0 + actual.abs().max(expected.abs()));
                assert!(
                    (actual - expected).abs() <= tolerance,
                    "{name}[{i}]: {actual} != {expected}"
                );
            }
            _ => panic!("{name}[{i}]: {actual:?} != {expected:?}"),
        }
    }
}

#[test]
fn ported_primitives_match_the_legacy_engine() {
    let fixture: Value = serde_json::from_str(VECTORS).expect("fixture is valid JSON");

    let closes = fixture_series(&fixture, &["closes"]);
    let bars: Vec<Bar> = fixture["bars"]
        .as_array()
        .expect("bars")
        .iter()
        .map(|bar| Bar {
            open: bar["open"].as_f64().expect("open"),
            high: bar["high"].as_f64().expect("high"),
            low: bar["low"].as_f64().expect("low"),
            close: bar["close"].as_f64().expect("close"),
            volume: bar["volume"].as_f64().expect("volume"),
        })
        .collect();

    assert_series(
        "sma20",
        &ta::sma(&closes, 20),
        &fixture_series(&fixture, &["expected", "sma20"]),
    );
    assert_series(
        "ema12",
        &ta::ema(&closes, 12),
        &fixture_series(&fixture, &["expected", "ema12"]),
    );
    assert_series(
        "rma14",
        &ta::rma(&closes, 14),
        &fixture_series(&fixture, &["expected", "rma14"]),
    );
    assert_series(
        "rollingSum10",
        &ta::rolling_sum(&closes, 10),
        &fixture_series(&fixture, &["expected", "rollingSum10"]),
    );
    assert_series(
        "rsi14",
        &ta::rsi(&closes, 14),
        &fixture_series(&fixture, &["expected", "rsi14"]),
    );
    assert_series(
        "stdev20",
        &ta::stdev(&closes, 20),
        &fixture_series(&fixture, &["expected", "stdev20"]),
    );

    let ranges: Vec<Option<f64>> = ta::true_range(&bars).into_iter().map(Some).collect();
    assert_series(
        "trueRange",
        &ranges,
        &fixture_series(&fixture, &["expected", "trueRange"]),
    );
    assert_series(
        "atr14",
        &ta::atr(&bars, 14),
        &fixture_series(&fixture, &["expected", "atr14"]),
    );
    assert_series(
        "vwap",
        &ta::vwap(&bars),
        &fixture_series(&fixture, &["expected", "vwap"]),
    );
    assert_series(
        "obv",
        &ta::obv(&bars),
        &fixture_series(&fixture, &["expected", "obv"]),
    );
    assert_series(
        "cmf20",
        &ta::cmf(&bars, 20),
        &fixture_series(&fixture, &["expected", "cmf20"]),
    );

    let expected = ta::macd(&closes, 12, 26, 9);
    let legacy = &fixture["expected"]["macd12_26_9"];
    for field in ["macd", "signal", "hist"] {
        assert_series(
            &format!("macd12_26_9.{field}"),
            match field {
                "macd" => &expected.macd,
                "signal" => &expected.signal,
                _ => &expected.hist,
            },
            &fixture_series(legacy, &[field]),
        );
    }

    let expected = ta::donchian(&bars, 20);
    let legacy = &fixture["expected"]["donchian20"];
    assert_series(
        "donchian20.upper",
        &expected.upper,
        &fixture_series(legacy, &["upper"]),
    );
    assert_series(
        "donchian20.lower",
        &expected.lower,
        &fixture_series(legacy, &["lower"]),
    );
    assert_series(
        "donchian20.middle",
        &expected.middle,
        &fixture_series(legacy, &["middle"]),
    );
}
