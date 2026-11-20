//! Canonical indicator registry, ported from the legacy `engine/indicators.mjs`.
//!
//! The registry is the single catalog of named indicators: each definition
//! carries a stable id, a definition version, its parameter bounds, the outputs
//! it produces, and a calculation built only on [`crate::ta`]. The Desktop and
//! any future worker resolve parameters the same way, so a chart and a research
//! event cannot disagree about what "MACD 12/26/9" means.

use std::collections::BTreeMap;

use crate::Bar;
use crate::ta;

/// Resolved indicator parameters. Values are clamped to the definition bounds.
pub type IndicatorParams = BTreeMap<&'static str, f64>;

/// One series per output key, aligned to the input bars.
pub type IndicatorSeries = BTreeMap<&'static str, Vec<Option<f64>>>;

/// Where an indicator is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndicatorKind {
    Overlay,
    Pane,
    Volume,
}

/// How an output is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    Line,
    Histogram,
}

/// One numeric parameter with its default and inclusive bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamSpec {
    pub name: &'static str,
    pub default: f64,
    pub min: f64,
    pub max: f64,
}

/// One calculated series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSpec {
    pub key: &'static str,
    pub kind: OutputKind,
    pub color: &'static str,
}

/// A registered indicator.
pub struct IndicatorDefinition {
    pub id: &'static str,
    pub version: &'static str,
    pub kind: IndicatorKind,
    pub params: &'static [ParamSpec],
    pub warmup: usize,
    pub outputs: &'static [OutputSpec],
    pub calculate: fn(&[Bar], &IndicatorParams) -> IndicatorSeries,
}

const fn param(name: &'static str, default: f64, min: f64, max: f64) -> ParamSpec {
    ParamSpec {
        name,
        default,
        min,
        max,
    }
}

const fn line(key: &'static str, color: &'static str) -> OutputSpec {
    OutputSpec {
        key,
        kind: OutputKind::Line,
        color,
    }
}

const fn histogram(key: &'static str, color: &'static str) -> OutputSpec {
    OutputSpec {
        key,
        kind: OutputKind::Histogram,
        color,
    }
}

const LENGTH_20: &[ParamSpec] = &[param("length", 20.0, 2.0, 500.0)];
const LENGTH_14: &[ParamSpec] = &[param("length", 14.0, 2.0, 500.0)];

/// Every compiled indicator definition, in catalog order.
pub static INDICATORS: &[IndicatorDefinition] = &[
    IndicatorDefinition {
        id: "sma",
        version: "1",
        kind: IndicatorKind::Overlay,
        params: LENGTH_20,
        warmup: 20,
        outputs: &[line("sma", "#3b82f6")],
        calculate: calc_sma,
    },
    IndicatorDefinition {
        id: "ema",
        version: "1",
        kind: IndicatorKind::Overlay,
        params: LENGTH_20,
        warmup: 1,
        outputs: &[line("ema", "#f59e0b")],
        calculate: calc_ema,
    },
    IndicatorDefinition {
        id: "rma",
        version: "1",
        kind: IndicatorKind::Overlay,
        params: LENGTH_14,
        warmup: 14,
        outputs: &[line("rma", "#ec4899")],
        calculate: calc_rma,
    },
    IndicatorDefinition {
        id: "vwap",
        version: "1",
        kind: IndicatorKind::Overlay,
        params: &[],
        warmup: 1,
        outputs: &[line("vwap", "#06b6d4")],
        calculate: calc_vwap,
    },
    IndicatorDefinition {
        id: "bb",
        version: "1",
        kind: IndicatorKind::Overlay,
        params: &[
            param("length", 20.0, 2.0, 500.0),
            param("mult", 2.0, 0.5, 6.0),
        ],
        warmup: 20,
        outputs: &[
            line("upper", "#a78bfa"),
            line("mid", "#64748b"),
            line("lower", "#a78bfa"),
        ],
        calculate: calc_bb,
    },
    IndicatorDefinition {
        id: "donchian",
        version: "1",
        kind: IndicatorKind::Overlay,
        params: LENGTH_20,
        warmup: 20,
        outputs: &[
            line("upper", "#f43f5e"),
            line("middle", "#64748b"),
            line("lower", "#10b981"),
        ],
        calculate: calc_donchian,
    },
    IndicatorDefinition {
        id: "volume",
        version: "1",
        kind: IndicatorKind::Volume,
        params: &[],
        warmup: 0,
        outputs: &[histogram("volume", "#3b82f6")],
        calculate: calc_volume,
    },
    IndicatorDefinition {
        id: "volma",
        version: "1",
        kind: IndicatorKind::Volume,
        params: LENGTH_20,
        warmup: 20,
        outputs: &[line("volma", "#f59e0b")],
        calculate: calc_volma,
    },
    IndicatorDefinition {
        id: "atr",
        version: "1",
        kind: IndicatorKind::Pane,
        params: LENGTH_14,
        warmup: 14,
        outputs: &[line("atr", "#f97316")],
        calculate: calc_atr,
    },
    IndicatorDefinition {
        id: "rsi",
        version: "1",
        kind: IndicatorKind::Pane,
        params: LENGTH_14,
        warmup: 15,
        outputs: &[line("rsi", "#10b981")],
        calculate: calc_rsi,
    },
    IndicatorDefinition {
        id: "macd",
        version: "1",
        kind: IndicatorKind::Pane,
        params: &[
            param("fast", 12.0, 2.0, 200.0),
            param("slow", 26.0, 3.0, 400.0),
            param("signal", 9.0, 2.0, 200.0),
        ],
        warmup: 26,
        outputs: &[
            line("macd", "#3b82f6"),
            line("signal", "#f59e0b"),
            histogram("hist", "#64748b"),
        ],
        calculate: calc_macd,
    },
    IndicatorDefinition {
        id: "obv",
        version: "1",
        kind: IndicatorKind::Pane,
        params: &[],
        warmup: 1,
        outputs: &[line("obv", "#8b5cf6")],
        calculate: calc_obv,
    },
    IndicatorDefinition {
        id: "cmf",
        version: "1",
        kind: IndicatorKind::Pane,
        params: LENGTH_20,
        warmup: 20,
        outputs: &[line("cmf", "#14b8a6")],
        calculate: calc_cmf,
    },
    IndicatorDefinition {
        id: "psar",
        version: "1",
        kind: IndicatorKind::Overlay,
        params: &[
            param("start", 0.02, 0.001, 0.1),
            param("inc", 0.02, 0.001, 0.1),
            param("max", 0.2, 0.01, 1.0),
        ],
        warmup: 2,
        outputs: &[line("psar", "#d946ef")],
        calculate: calc_psar,
    },
];

/// Look up a definition by its stable id.
#[must_use]
pub fn definition(id: &str) -> Option<&'static IndicatorDefinition> {
    INDICATORS.iter().find(|definition| definition.id == id)
}

/// Clamp a partial parameter object onto the definition defaults.
///
/// Unknown keys are dropped, a missing or non-finite value falls back to the
/// default, and every value is clamped to the inclusive bounds.
#[must_use]
pub fn resolve_params(
    definition: &IndicatorDefinition,
    partial: &IndicatorParams,
) -> IndicatorParams {
    definition
        .params
        .iter()
        .map(|schema| {
            let value = partial
                .get(schema.name)
                .filter(|value| value.is_finite())
                .copied()
                .unwrap_or(schema.default);
            (schema.name, value.clamp(schema.min, schema.max))
        })
        .collect()
}

/// Compute one indicator with schema-resolved parameters.
///
/// Returns [`None`] for an unknown id. Every declared output is present and
/// aligned to `bars.len()`; `None` is Pine `na` and becomes chart whitespace.
#[must_use]
pub fn calculate_indicator(
    id: &str,
    bars: &[Bar],
    partial: &IndicatorParams,
) -> Option<IndicatorSeries> {
    let definition = definition(id)?;
    let params = resolve_params(definition, partial);
    let calculated = (definition.calculate)(bars, &params);
    let aligned = definition
        .outputs
        .iter()
        .map(|output| {
            let series = calculated
                .get(output.key)
                .filter(|series| series.len() == bars.len())
                .cloned()
                .unwrap_or_else(|| vec![None; bars.len()]);
            (output.key, series)
        })
        .collect();
    Some(aligned)
}

fn closes(bars: &[Bar]) -> Vec<Option<f64>> {
    bars.iter().map(|bar| Some(bar.close)).collect()
}

fn volumes(bars: &[Bar]) -> Vec<Option<f64>> {
    bars.iter().map(|bar| Some(bar.volume)).collect()
}

fn number(params: &IndicatorParams, name: &str) -> f64 {
    params.get(name).copied().unwrap_or(0.0)
}

fn length(params: &IndicatorParams) -> usize {
    number(params, "length").max(0.0) as usize
}

fn calc_sma(bars: &[Bar], params: &IndicatorParams) -> IndicatorSeries {
    let sma = ta::sma(&closes(bars), length(params));
    series([("sma", sma)])
}

fn calc_ema(bars: &[Bar], params: &IndicatorParams) -> IndicatorSeries {
    let ema = ta::ema(&closes(bars), length(params));
    series([("ema", ema)])
}

fn calc_rma(bars: &[Bar], params: &IndicatorParams) -> IndicatorSeries {
    let rma = ta::rma(&closes(bars), length(params));
    series([("rma", rma)])
}

fn calc_vwap(bars: &[Bar], _params: &IndicatorParams) -> IndicatorSeries {
    series([("vwap", ta::vwap(bars))])
}

fn calc_bb(bars: &[Bar], params: &IndicatorParams) -> IndicatorSeries {
    let length = length(params);
    let mult = number(params, "mult");
    let close = closes(bars);
    let mid = ta::sma(&close, length);
    let deviation = ta::stdev(&close, length);
    let mut upper = Vec::with_capacity(bars.len());
    let mut lower = Vec::with_capacity(bars.len());
    for (value, deviation) in mid.iter().zip(&deviation) {
        match (value, deviation) {
            (Some(value), Some(deviation)) => {
                upper.push(Some(value + mult * deviation));
                lower.push(Some(value - mult * deviation));
            }
            _ => {
                upper.push(None);
                lower.push(None);
            }
        }
    }
    series([("upper", upper), ("mid", mid), ("lower", lower)])
}

fn calc_donchian(bars: &[Bar], params: &IndicatorParams) -> IndicatorSeries {
    let donchian = ta::donchian(bars, length(params));
    series([
        ("upper", donchian.upper),
        ("middle", donchian.middle),
        ("lower", donchian.lower),
    ])
}

fn calc_volume(bars: &[Bar], _params: &IndicatorParams) -> IndicatorSeries {
    series([("volume", volumes(bars))])
}

fn calc_volma(bars: &[Bar], params: &IndicatorParams) -> IndicatorSeries {
    let volma = ta::sma(&volumes(bars), length(params));
    series([("volma", volma)])
}

fn calc_atr(bars: &[Bar], params: &IndicatorParams) -> IndicatorSeries {
    let atr = ta::atr(bars, length(params));
    series([("atr", atr)])
}

fn calc_rsi(bars: &[Bar], params: &IndicatorParams) -> IndicatorSeries {
    let rsi = ta::rsi(&closes(bars), length(params));
    series([("rsi", rsi)])
}

fn calc_macd(bars: &[Bar], params: &IndicatorParams) -> IndicatorSeries {
    let macd = ta::macd(
        &closes(bars),
        length_of(params, "fast"),
        length_of(params, "slow"),
        length_of(params, "signal"),
    );
    series([
        ("macd", macd.macd),
        ("signal", macd.signal),
        ("hist", macd.hist),
    ])
}

fn calc_obv(bars: &[Bar], _params: &IndicatorParams) -> IndicatorSeries {
    series([("obv", ta::obv(bars))])
}

fn calc_cmf(bars: &[Bar], params: &IndicatorParams) -> IndicatorSeries {
    let cmf = ta::cmf(bars, length(params));
    series([("cmf", cmf)])
}

fn calc_psar(bars: &[Bar], params: &IndicatorParams) -> IndicatorSeries {
    let psar = ta::sar(
        bars,
        number(params, "start"),
        number(params, "inc"),
        number(params, "max"),
    );
    series([("psar", psar)])
}

fn length_of(params: &IndicatorParams, name: &str) -> usize {
    number(params, name).max(0.0) as usize
}

fn series<const N: usize>(entries: [(&'static str, Vec<Option<f64>>); N]) -> IndicatorSeries {
    entries.into_iter().collect()
}
