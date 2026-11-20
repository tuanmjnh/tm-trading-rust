//! Goldens for the indicator registry.
//!
//! `fixtures/indicator_vectors.json` is generated from the legacy
//! `engine/indicators.mjs` by `fixtures/generate_vectors.mjs`. The catalog is
//! compared field by field, and every indicator is recomputed on the same bars
//! and compared against the legacy output.

use std::collections::BTreeMap;

use serde_json::Value;
use tm_trading_research_engine::Bar;
use tm_trading_research_engine::indicators::{self, IndicatorKind, IndicatorParams, OutputKind};

const VECTORS: &str = include_str!("fixtures/indicator_vectors.json");

fn fixture_bars(fixture: &Value) -> Vec<Bar> {
    fixture["bars"]
        .as_array()
        .expect("bars")
        .iter()
        .map(|bar| Bar {
            time: bar["time"].as_i64().expect("time"),
            open: bar["open"].as_f64().expect("open"),
            high: bar["high"].as_f64().expect("high"),
            low: bar["low"].as_f64().expect("low"),
            close: bar["close"].as_f64().expect("close"),
            volume: bar["volume"].as_f64().expect("volume"),
        })
        .collect()
}

fn fixture_series(values: &Value) -> Vec<Option<f64>> {
    values
        .as_array()
        .expect("series")
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

fn kind_name(kind: IndicatorKind) -> &'static str {
    match kind {
        IndicatorKind::Overlay => "overlay",
        IndicatorKind::Pane => "pane",
        IndicatorKind::Volume => "volume",
    }
}

fn output_kind_name(kind: OutputKind) -> &'static str {
    match kind {
        OutputKind::Line => "line",
        OutputKind::Histogram => "histogram",
    }
}

#[test]
fn the_catalog_matches_the_legacy_registry() {
    let fixture: Value = serde_json::from_str(VECTORS).expect("fixture is valid JSON");
    let catalog = fixture["catalog"].as_array().expect("catalog");
    assert_eq!(
        indicators::INDICATORS.len(),
        catalog.len(),
        "catalog length"
    );

    for (definition, legacy) in indicators::INDICATORS.iter().zip(catalog) {
        assert_eq!(definition.id, legacy["id"].as_str().expect("id"));
        assert_eq!(
            definition.version,
            legacy["version"].as_str().expect("version")
        );
        assert_eq!(
            kind_name(definition.kind),
            legacy["kind"].as_str().expect("kind")
        );
        assert_eq!(
            definition.warmup,
            legacy["warmup"].as_u64().expect("warmup") as usize
        );

        let legacy_params = legacy["params"].as_array().expect("params");
        assert_eq!(
            definition.params.len(),
            legacy_params.len(),
            "{}: params",
            definition.id
        );
        for (param, legacy) in definition.params.iter().zip(legacy_params) {
            assert_eq!(param.name, legacy["name"].as_str().expect("param name"));
            for (value, key) in [
                (param.default, "default"),
                (param.min, "min"),
                (param.max, "max"),
            ] {
                let expected = legacy[key].as_f64().expect("param bound");
                assert!(
                    (value - expected).abs() < 1e-12,
                    "{}: {} {} != {expected}",
                    definition.id,
                    param.name,
                    key
                );
            }
        }

        let legacy_outputs = legacy["outputs"].as_array().expect("outputs");
        assert_eq!(
            definition.outputs.len(),
            legacy_outputs.len(),
            "{}: outputs",
            definition.id
        );
        for (output, legacy) in definition.outputs.iter().zip(legacy_outputs) {
            assert_eq!(output.key, legacy["key"].as_str().expect("output key"));
            assert_eq!(
                output_kind_name(output.kind),
                legacy["type"].as_str().expect("output type")
            );
            assert_eq!(
                output.color,
                legacy["color"].as_str().expect("output color")
            );
        }
    }
}

#[test]
fn every_indicator_matches_the_legacy_output() {
    let fixture: Value = serde_json::from_str(VECTORS).expect("fixture is valid JSON");
    let bars = fixture_bars(&fixture);
    let legacy_outputs = &fixture["outputs"];

    for definition in indicators::INDICATORS {
        let defaults: IndicatorParams = BTreeMap::new();
        let calculated = indicators::calculate_indicator(definition.id, &bars, &defaults)
            .unwrap_or_else(|| panic!("{}: missing definition", definition.id));
        for output in definition.outputs {
            let expected = fixture_series(
                legacy_outputs
                    .get(definition.id)
                    .and_then(|outputs| outputs.get(output.key))
                    .unwrap_or_else(|| panic!("{}: missing {}", definition.id, output.key)),
            );
            assert_series(
                &format!("{}.{}", definition.id, output.key),
                &calculated[output.key],
                &expected,
            );
        }
    }

    assert!(
        indicators::calculate_indicator("unknown", &bars, &BTreeMap::new()).is_none(),
        "an unknown id has no definition"
    );
}

#[test]
fn parameters_are_clamped_and_unknown_keys_dropped() {
    let sma = indicators::definition("sma").expect("sma is registered");
    let mut partial: IndicatorParams = BTreeMap::new();
    partial.insert("length", 9000.0);
    partial.insert("unused", 1.0);
    let resolved = indicators::resolve_params(sma, &partial);
    assert_eq!(resolved.len(), 1, "unknown keys are dropped");
    assert_eq!(resolved["length"], 500.0);

    partial.insert("length", -5.0);
    assert_eq!(indicators::resolve_params(sma, &partial)["length"], 2.0);

    partial.insert("length", f64::NAN);
    assert_eq!(
        indicators::resolve_params(sma, &partial)["length"],
        sma.params[0].default,
        "a non-finite value falls back to the default"
    );

    let macd = indicators::definition("macd").expect("macd is registered");
    let mut partial: IndicatorParams = BTreeMap::new();
    partial.insert("fast", 30.0);
    let resolved = indicators::resolve_params(macd, &partial);
    assert_eq!(resolved["fast"], 30.0);
    assert_eq!(resolved["slow"], 26.0);
    assert_eq!(resolved["signal"], 9.0);
}
