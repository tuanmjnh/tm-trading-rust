//! Goldens for the VSA method.
//!
//! `fixtures/vsa_vectors.json` is generated from the legacy
//! `engine/methods/vsa.mjs` by `fixtures/generate_vectors.mjs`. Every scenario
//! compares the events, the setups, the flags, the scores and the per-bar
//! series of the Rust port against the legacy output.

use serde_json::Value;
use tm_trading_research_engine::Bar;
use tm_trading_research_engine::methods::vsa::{
    self, EntryMode, SignalMode, TpMode, VsaEventKind, VsaParams, WyckoffEvent,
};

const VECTORS: &str = include_str!("fixtures/vsa_vectors.json");

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

fn close(actual: f64, expected: f64, context: &str) {
    let tolerance = 1e-8 * (1.0 + actual.abs().max(expected.abs()));
    assert!(
        (actual - expected).abs() <= tolerance,
        "{context}: {actual} != {expected}"
    );
}

fn assert_bools(actual: &[bool], expected: &Value, context: &str) {
    let expected = expected.as_array().expect("array");
    assert_eq!(actual.len(), expected.len(), "{context}: length");
    for (i, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(*actual, expected.as_bool().expect("bool"), "{context}[{i}]");
    }
}

fn assert_numbers(actual: &[Option<f64>], expected: &Value, context: &str) {
    let expected = expected.as_array().expect("array");
    assert_eq!(actual.len(), expected.len(), "{context}: length");
    for (i, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        match (actual, expected.as_f64()) {
            (None, None) => {}
            (Some(actual), Some(expected)) => close(*actual, expected, &format!("{context}[{i}]")),
            _ => panic!("{context}[{i}]: {actual:?} != {expected:?}"),
        }
    }
}

/// Apply the scenario options of the fixture to the default parameters.
fn scenario_params(options: &Value) -> VsaParams {
    let mut params = VsaParams::default();
    let number = |name: &str| options[name].as_f64().expect("number") as usize;
    if options.get("pivLen").is_some() {
        params.pivot_length = number("pivLen");
    }
    if options.get("lvFresh").is_some() {
        params.level_fresh = number("lvFresh");
    }
    if options.get("sessOn").is_some() {
        params.session_enabled = options["sessOn"].as_bool().expect("sessOn");
    }
    if options.get("sess").is_some() {
        params.session = options["sess"].as_str().expect("sess").to_owned();
    }
    if options.get("tz").is_some() {
        params.timezone = options["tz"].as_str().expect("tz").to_owned();
    }
    if options.get("entryMode").is_some() {
        params.entry_mode = match options["entryMode"].as_str().expect("entryMode") {
            "limit" => EntryMode::Limit,
            _ => EntryMode::Market,
        };
    }
    if options.get("tpMode").is_some() {
        params.take_profit_mode = match options["tpMode"].as_str().expect("tpMode") {
            "pivot" => TpMode::Pivot,
            _ => TpMode::R,
        };
    }
    if options.get("signalMode").is_some() {
        params.signal_mode = match options["signalMode"].as_str().expect("signalMode") {
            "wyckoff" => SignalMode::Wyckoff,
            _ => SignalMode::Vsa,
        };
    }
    if options.get("wyckoffEvent").is_some() {
        params.wyckoff_event = match options["wyckoffEvent"].as_str().expect("wyckoffEvent") {
            "sos" => WyckoffEvent::Sos,
            "lps" => WyckoffEvent::Lps,
            _ => WyckoffEvent::Spring,
        };
    }
    params
}

fn assert_scenario(bars: &[Bar], name: &str, expected: &Value) {
    let params = scenario_params(&expected["options"]);
    let actual = vsa::analyze(bars, &params);

    let events = expected["events"].as_array().expect("events");
    assert_eq!(actual.events.len(), events.len(), "{name}: event count");
    for (event, expected) in actual.events.iter().zip(events) {
        assert_eq!(event.bar, expected["bar"].as_u64().expect("bar") as usize);
        assert_eq!(event.kind.label(), expected["type"].as_str().expect("type"));
    }

    let setups = expected["setups"].as_array().expect("setups");
    assert_eq!(actual.setups.len(), setups.len(), "{name}: setup count");
    for (setup, expected) in actual.setups.iter().zip(setups) {
        let context = format!("{name}: setup at bar {}", setup.bar);
        assert_eq!(setup.bar, expected["bar"].as_u64().expect("bar") as usize);
        assert_eq!(
            setup.direction,
            expected["dir"].as_i64().expect("dir") as i8
        );
        close(
            setup.entry,
            expected["entry"].as_f64().expect("entry"),
            &context,
        );
        close(
            setup.stop_loss,
            expected["sl"].as_f64().expect("sl"),
            &context,
        );
        close(
            setup.take_profit,
            expected["tp"].as_f64().expect("tp"),
            &context,
        );
        close(
            setup.risk,
            expected["risk"].as_f64().expect("risk"),
            &context,
        );
        assert_eq!(setup.filled, expected["filled"].as_bool().expect("filled"));
        assert_eq!(
            setup.fill_bar,
            expected["fillBar"].as_u64().expect("fillBar") as usize
        );
        match (setup.done_bar, expected.get("doneBar")) {
            (None, None) => {}
            (None, Some(Value::Null)) => {}
            (Some(actual), Some(expected)) => assert_eq!(
                actual,
                expected.as_u64().expect("doneBar") as usize,
                "{context}: done bar"
            ),
            _ => panic!("{context}: done bar mismatch"),
        }
        let expected_result = expected.get("result").and_then(Value::as_str);
        assert_eq!(
            setup.result.map(|result| result.label()),
            expected_result,
            "{context}: result"
        );
    }

    let scores = expected["scores"].as_array().expect("scores");
    assert_eq!(actual.scores.len(), scores.len(), "{name}: score count");
    for (i, (actual, expected)) in actual.scores.iter().zip(scores).enumerate() {
        close(
            *actual,
            expected.as_f64().expect("score"),
            &format!("{name}: score[{i}]"),
        );
    }

    let flags = &expected["flags"];
    assert_bools(&actual.flags.sv, &flags["SV"], &format!("{name}: SV"));
    assert_bools(&actual.flags.bc, &flags["BC"], &format!("{name}: BC"));
    assert_bools(
        &actual.flags.st_long,
        &flags["STl"],
        &format!("{name}: STl"),
    );
    assert_bools(
        &actual.flags.st_short,
        &flags["STs"],
        &format!("{name}: STs"),
    );
    assert_bools(
        &actual.flags.no_supply,
        &flags["NS"],
        &format!("{name}: NS"),
    );
    assert_bools(
        &actual.flags.no_demand,
        &flags["ND"],
        &format!("{name}: ND"),
    );

    let series = &expected["series"];
    assert_bools(
        &actual.series.is_purple,
        &series["isPurple"],
        &format!("{name}: isPurple"),
    );
    assert_bools(
        &actual.series.quiet_setup,
        &series["quietSetup"],
        &format!("{name}: quietSetup"),
    );
    assert_bools(
        &actual.series.quiet_low,
        &series["quietLow"],
        &format!("{name}: quietLow"),
    );
    assert_bools(
        &actual.series.event_ok,
        &series["eventOk"],
        &format!("{name}: eventOk"),
    );
    assert_bools(
        &actual.series.session_ok,
        &series["sessionOk"],
        &format!("{name}: sessionOk"),
    );
    assert_bools(
        &actual.series.near_support,
        &series["nearSupport"],
        &format!("{name}: nearSupport"),
    );
    assert_bools(
        &actual.series.near_resistance,
        &series["nearResistance"],
        &format!("{name}: nearResistance"),
    );
    assert_bools(
        &actual.series.sv_fresh,
        &series["svFresh"],
        &format!("{name}: svFresh"),
    );
    assert_bools(
        &actual.series.bc_fresh,
        &series["bcFresh"],
        &format!("{name}: bcFresh"),
    );
    assert_bools(
        &actual.series.spring,
        &series["spring"],
        &format!("{name}: spring"),
    );
    assert_bools(
        &actual.series.utad,
        &series["utad"],
        &format!("{name}: utad"),
    );
    assert_bools(&actual.series.sos, &series["sos"], &format!("{name}: sos"));
    assert_bools(&actual.series.sow, &series["sow"], &format!("{name}: sow"));
    assert_bools(&actual.series.lps, &series["lps"], &format!("{name}: lps"));
    assert_bools(
        &actual.series.lpsy,
        &series["lpsy"],
        &format!("{name}: lpsy"),
    );

    assert_numbers(
        &actual.series.volume_average,
        &series["volumeAverage"],
        &format!("{name}: volumeAverage"),
    );
    assert_numbers(
        &actual.series.fee_r,
        &series["feeR"],
        &format!("{name}: feeR"),
    );
    assert_numbers(
        &actual.series.purple_count,
        &series["purpleCount"],
        &format!("{name}: purpleCount"),
    );

    let buckets = series["bucket"].as_array().expect("bucket");
    assert_eq!(
        actual.series.bucket.len(),
        buckets.len(),
        "{name}: bucket count"
    );
    for (i, (actual, expected)) in actual.series.bucket.iter().zip(buckets).enumerate() {
        assert_eq!(
            actual.label(),
            expected.as_str().expect("bucket"),
            "{name}: bucket[{i}]"
        );
    }

    let events = series["lastEvent"].as_array().expect("lastEvent");
    for (i, (actual, expected)) in actual.series.last_event.iter().zip(events).enumerate() {
        let expected = expected.as_str().expect("lastEvent");
        // The legacy series starts with the placeholder "-".
        let expected = if expected == "-" {
            None
        } else {
            Some(expected)
        };
        assert_eq!(*actual, expected, "{name}: lastEvent[{i}]");
    }
}

#[test]
fn every_scenario_matches_the_legacy_method() {
    let fixture: Value = serde_json::from_str(VECTORS).expect("fixture is valid JSON");
    let bars = fixture_bars(&fixture);
    let scenarios = fixture["scenarios"].as_object().expect("scenarios");
    assert!(!scenarios.is_empty(), "the fixture has scenarios");
    for (name, expected) in scenarios {
        assert_scenario(&bars, name, expected);
    }
}

#[test]
fn the_scenario_exercises_every_event_kind() {
    let fixture: Value = serde_json::from_str(VECTORS).expect("fixture is valid JSON");
    let bars = fixture_bars(&fixture);
    let params = scenario_params(&fixture["scenarios"]["scenario"]["options"]);
    let output = vsa::analyze(&bars, &params);

    let kinds: Vec<VsaEventKind> = output.events.iter().map(|event| event.kind).collect();
    for kind in [
        VsaEventKind::Sv,
        VsaEventKind::Bc,
        VsaEventKind::StLong,
        VsaEventKind::StShort,
        VsaEventKind::Ns,
        VsaEventKind::Nd,
    ] {
        assert!(
            kinds.contains(&kind),
            "the scenario must produce {}",
            kind.label()
        );
    }

    assert!(
        output.series.spring.iter().any(|spring| *spring),
        "the scenario must produce a spring"
    );
    assert!(
        output.series.utad.iter().any(|utad| *utad),
        "the scenario must produce an UTAD"
    );
    assert!(
        output.series.sos.iter().any(|sos| *sos),
        "the scenario must produce a SOS"
    );
    assert!(
        output.series.sow.iter().any(|sow| *sow),
        "the scenario must produce a SOW"
    );
    assert!(
        output.series.lps.iter().any(|lps| *lps),
        "the scenario must produce an LPS"
    );
    assert!(
        output.series.lpsy.iter().any(|lpsy| *lpsy),
        "the scenario must produce an LPSY"
    );
    assert!(
        output.setups.iter().any(|setup| setup.result.is_some()),
        "the scenario must close a setup"
    );
}

#[test]
fn a_session_filter_suppresses_events_outside_the_session() {
    let fixture: Value = serde_json::from_str(VECTORS).expect("fixture is valid JSON");
    let bars = fixture_bars(&fixture);
    let without_filter = vsa::analyze(
        &bars,
        &scenario_params(&fixture["scenarios"]["scenario"]["options"]),
    );
    let with_filter = vsa::analyze(
        &bars,
        &scenario_params(&fixture["scenarios"]["session"]["options"]),
    );
    assert!(
        with_filter.events.len() < without_filter.events.len(),
        "the session filter must remove events"
    );
    assert!(
        with_filter.series.session_ok.iter().any(|inside| !inside),
        "the scenario must contain bars outside the session"
    );
}
