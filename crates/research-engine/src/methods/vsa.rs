//! Method 0: VSA / Wyckoff, ported from the legacy `engine/methods/vsa.mjs`.
//!
//! The legacy module is the acceptance source for this port. The order of the
//! per-bar steps is preserved exactly (`20_volume` -> `30_levels` -> `40_events`),
//! because the order carries meaning: a setup bar cannot be both `SV` and `ST`.
//!
//! The Rust port differs from the legacy JavaScript in two deliberate ways:
//!
//! - The mode parameters are enums, so an invalid configuration cannot be built.
//! - A missing ATR suppresses the ATR-dependent events and setups instead of
//!   coercing `na` to `0` the way JavaScript arithmetic does. That only happens
//!   when `atr_length` is `0`, which is not a supported configuration.

use crate::Bar;
use crate::ta::{self, VsaBucket, VsaRatios};

/// Stable identifier of the method.
pub const ID: &str = "vsa";

/// Human-readable name of the method.
pub const NAME: &str = "VSA / Wyckoff (method 0)";

/// How a setup is entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryMode {
    /// Enter at the close of the signal bar.
    Market,
    /// Enter at the support/resistance extreme of the setup.
    Limit,
}

/// How a setup takes profit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpMode {
    /// Take profit at a fixed multiple of the risk.
    R,
    /// Take profit at the opposing pivot when it is far enough away.
    Pivot,
}

/// What decides the entry timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalMode {
    /// The VSA setup/retest pattern decides the entry.
    Vsa,
    /// A Wyckoff event decides the entry; VSA only defines the zones.
    Wyckoff,
}

/// The Wyckoff event used in [`SignalMode::Wyckoff`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WyckoffEvent {
    Spring,
    Sos,
    Lps,
}

/// Method parameters. The defaults are the legacy `DEFAULTS` object.
#[derive(Debug, Clone, PartialEq)]
pub struct VsaParams {
    /// Length of the volume moving average.
    pub len: usize,
    /// Volume ratio that marks the `TIM` bucket.
    pub r_purple: f64,
    /// Volume ratio that marks the `VeryHigh` bucket.
    pub r_very_high: f64,
    /// Volume ratio that marks the `High` bucket.
    pub r_high: f64,
    /// Volume ratio that marks the `Normal` bucket.
    pub r_normal: f64,
    /// Volume ratio that marks the `Low` bucket.
    pub r_low: f64,
    /// Window of the rolling purple-bar count.
    pub purple_window: usize,
    /// Minimum absolute volume for an event to count.
    pub volume_min: f64,
    /// Whether the session filter is applied.
    pub session_enabled: bool,
    /// Session definition, `HHMM-HHMM`.
    pub session: String,
    /// IANA timezone of the session.
    pub timezone: String,
    /// Whether setup and zone events are emitted.
    pub events_enabled: bool,
    /// Whether no-supply/no-demand events are emitted.
    pub quiet_enabled: bool,
    /// Bars used by the run-up/run-down comparison.
    pub run: usize,
    /// Distance from a pivot that counts as "near", in ATR units.
    pub level_tolerance: f64,
    /// Bars a pivot or zone stays relevant.
    pub level_fresh: usize,
    /// Volume ratio below which a bar is a quiet test.
    pub test_ratio: f64,
    /// Distance from the zone extreme that counts as a retest, in ATR units.
    pub retest_tolerance: f64,
    /// Length of the ATR.
    pub atr_length: usize,
    /// Length of the pivot lookback.
    pub pivot_length: usize,
    /// Stop buffer below the zone extreme, in ATR units.
    pub stop_buffer: f64,
    /// Reward/risk ratio used by [`TpMode::R`].
    pub reward_risk: f64,
    /// Round-trip fee in percent.
    pub fee_percent: f64,
    /// Highest fee expressed in R that still allows a setup.
    pub fee_max_r: f64,
    /// Entry mode.
    pub entry_mode: EntryMode,
    /// Take-profit mode.
    pub take_profit_mode: TpMode,
    /// Signal mode.
    pub signal_mode: SignalMode,
    /// Wyckoff event used in [`SignalMode::Wyckoff`].
    pub wyckoff_event: WyckoffEvent,
    /// Minimum body range of a SOS/SOW bar, in ATR units.
    pub sos_spread: f64,
    /// Minimum volume of a SOS/SOW bar, in moving-average units.
    pub sos_volume: f64,
    /// Pullback tolerance of an LPS/LPSY bar, in ATR units.
    pub lps_tolerance: f64,
}

impl Default for VsaParams {
    fn default() -> Self {
        Self {
            len: 20,
            r_purple: 2.2,
            r_very_high: 1.8,
            r_high: 1.2,
            r_normal: 0.8,
            r_low: 0.4,
            purple_window: 20,
            volume_min: 0.0,
            session_enabled: false,
            session: "0800-1600".to_owned(),
            timezone: "Asia/Ho_Chi_Minh".to_owned(),
            events_enabled: true,
            quiet_enabled: true,
            run: 5,
            level_tolerance: 0.5,
            level_fresh: 60,
            test_ratio: 1.2,
            retest_tolerance: 1.0,
            atr_length: 14,
            pivot_length: 20,
            stop_buffer: 0.5,
            reward_risk: 2.0,
            fee_percent: 0.05,
            fee_max_r: 0.15,
            entry_mode: EntryMode::Market,
            take_profit_mode: TpMode::R,
            signal_mode: SignalMode::Vsa,
            wyckoff_event: WyckoffEvent::Spring,
            sos_spread: 1.0,
            sos_volume: 1.2,
            lps_tolerance: 0.5,
        }
    }
}

/// The kind of a research event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VsaEventKind {
    /// Selling exhaustion: a support zone candidate.
    Sv,
    /// Buying climax: a resistance zone candidate.
    Bc,
    /// Long setup.
    StLong,
    /// Short setup.
    StShort,
    /// No supply: a quiet confirmation of a long.
    Ns,
    /// No demand: a quiet confirmation of a short.
    Nd,
}

impl VsaEventKind {
    /// The label used by the legacy engine and stored documents.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Sv => "SV",
            Self::Bc => "BC",
            Self::StLong => "ST LONG",
            Self::StShort => "ST SHORT",
            Self::Ns => "NS",
            Self::Nd => "ND",
        }
    }

    /// The directional score of the event, in `[-1, 1]`.
    #[must_use]
    pub fn score(self) -> f64 {
        match self {
            Self::StLong => 1.0,
            Self::StShort => -1.0,
            Self::Sv => 0.5,
            Self::Bc => -0.5,
            Self::Ns => 0.3,
            Self::Nd => -0.3,
        }
    }
}

/// One research event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VsaEvent {
    /// Index of the bar that produced the event.
    pub bar: usize,
    /// The kind of the event.
    pub kind: VsaEventKind,
}

/// The outcome of a setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VsaResult {
    StopLoss,
    TakeProfit,
}

impl VsaResult {
    /// The label used by the legacy engine and stored documents.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::StopLoss => "SL",
            Self::TakeProfit => "TP",
        }
    }
}

/// One setup. It is an observation, never an order.
#[derive(Debug, Clone, PartialEq)]
pub struct VsaSetup {
    /// Index of the bar that produced the setup.
    pub bar: usize,
    /// `1` for a long setup, `-1` for a short setup.
    pub direction: i8,
    /// Entry price.
    pub entry: f64,
    /// Stop price.
    pub stop_loss: f64,
    /// Target price.
    pub take_profit: f64,
    /// Distance between entry and stop.
    pub risk: f64,
    /// Whether the entry price was reached.
    pub filled: bool,
    /// Bar on which the entry price was reached.
    pub fill_bar: usize,
    /// Bar on which the setup closed.
    pub done_bar: Option<usize>,
    /// How the setup closed.
    pub result: Option<VsaResult>,
}

/// Per-bar flags of the legacy method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VsaFlags {
    pub sv: Vec<bool>,
    pub bc: Vec<bool>,
    pub st_long: Vec<bool>,
    pub st_short: Vec<bool>,
    pub no_supply: Vec<bool>,
    pub no_demand: Vec<bool>,
}

/// Per-bar series of the legacy method.
#[derive(Debug, Clone, PartialEq)]
pub struct VsaSeries {
    pub atr: Vec<Option<f64>>,
    pub volume_average: Vec<Option<f64>>,
    pub volume_ratio: Vec<f64>,
    pub bucket: Vec<VsaBucket>,
    pub purple_count: Vec<Option<f64>>,
    pub effort_run: Vec<usize>,
    pub last_purple_bar: Vec<Option<usize>>,
    pub is_purple_raw: Vec<bool>,
    pub is_purple: Vec<bool>,
    pub quiet_setup: Vec<bool>,
    pub quiet_low: Vec<bool>,
    pub event_ok: Vec<bool>,
    pub session_ok: Vec<bool>,
    pub near_support: Vec<bool>,
    pub near_resistance: Vec<bool>,
    pub run_up: Vec<bool>,
    pub run_down: Vec<bool>,
    pub low_fresh: Vec<bool>,
    pub high_fresh: Vec<bool>,
    pub sv_fresh: Vec<bool>,
    pub bc_fresh: Vec<bool>,
    pub fee_r: Vec<Option<f64>>,
    pub fee_ok: Vec<bool>,
    pub spring: Vec<bool>,
    pub utad: Vec<bool>,
    pub sos: Vec<bool>,
    pub sow: Vec<bool>,
    pub lps: Vec<bool>,
    pub lpsy: Vec<bool>,
    pub last_event: Vec<Option<&'static str>>,
    pub last_event_bar: Vec<usize>,
}

/// Everything the method produced for one bar set.
#[derive(Debug, Clone, PartialEq)]
pub struct VsaOutput {
    /// The parameters that produced the output.
    pub params: VsaParams,
    /// Per-bar series.
    pub series: VsaSeries,
    /// Per-bar flags.
    pub flags: VsaFlags,
    /// Events in the order they occurred.
    pub events: Vec<VsaEvent>,
    /// Setups in the order they occurred.
    pub setups: Vec<VsaSetup>,
    /// Directional score per bar, in `[-1, 1]`.
    pub scores: Vec<f64>,
}

/// Run the method over closed bars.
#[must_use]
pub fn analyze(bars: &[Bar], params: &VsaParams) -> VsaOutput {
    let n = bars.len();
    let close: Vec<f64> = bars.iter().map(|bar| bar.close).collect();
    let high: Vec<f64> = bars.iter().map(|bar| bar.high).collect();
    let low: Vec<f64> = bars.iter().map(|bar| bar.low).collect();
    let volume: Vec<f64> = bars.iter().map(|bar| finite(bar.volume)).collect();

    let atr = if params.atr_length > 0 {
        ta::atr(bars, params.atr_length)
    } else {
        vec![None; n]
    };
    let volume_average = ta::rma(
        &volume.iter().copied().map(Some).collect::<Vec<_>>(),
        params.len,
    );
    let pivot_lows = ta::pivot_low(
        &low.iter().copied().map(Some).collect::<Vec<_>>(),
        params.pivot_length,
        params.pivot_length,
    );
    let pivot_highs = ta::pivot_high(
        &high.iter().copied().map(Some).collect::<Vec<_>>(),
        params.pivot_length,
        params.pivot_length,
    );
    let volume_ratio: Vec<f64> = volume_average
        .iter()
        .enumerate()
        .map(|(i, average)| match average {
            Some(average) if *average > 0.0 => volume[i] / average,
            _ => 0.0,
        })
        .collect();
    let effort_move: Vec<bool> = (0..n)
        .map(|i| {
            let Some(atr) = atr[i] else {
                return false;
            };
            if i == 0 {
                return false;
            }
            (close[i] - close[i - 1]).abs() >= 0.5 * atr
        })
        .collect();

    let ratios = VsaRatios {
        purple: params.r_purple,
        very_high: params.r_very_high,
        high: params.r_high,
        normal: params.r_normal,
        low: params.r_low,
    };

    let mut series = VsaSeries {
        atr: atr.clone(),
        volume_average: volume_average.clone(),
        volume_ratio: volume_ratio.clone(),
        bucket: vec![VsaBucket::VeryLow; n],
        purple_count: vec![None; n],
        effort_run: vec![0; n],
        last_purple_bar: vec![None; n],
        is_purple_raw: vec![false; n],
        is_purple: vec![false; n],
        quiet_setup: vec![false; n],
        quiet_low: vec![false; n],
        event_ok: vec![false; n],
        session_ok: vec![false; n],
        near_support: vec![false; n],
        near_resistance: vec![false; n],
        run_up: vec![false; n],
        run_down: vec![false; n],
        low_fresh: vec![false; n],
        high_fresh: vec![false; n],
        sv_fresh: vec![false; n],
        bc_fresh: vec![false; n],
        fee_r: vec![None; n],
        fee_ok: vec![false; n],
        spring: vec![false; n],
        utad: vec![false; n],
        sos: vec![false; n],
        sow: vec![false; n],
        lps: vec![false; n],
        lpsy: vec![false; n],
        last_event: vec![None; n],
        last_event_bar: vec![0; n],
    };
    let mut flags = VsaFlags {
        sv: vec![false; n],
        bc: vec![false; n],
        st_long: vec![false; n],
        st_short: vec![false; n],
        no_supply: vec![false; n],
        no_demand: vec![false; n],
    };
    let mut events: Vec<VsaEvent> = Vec::new();
    let mut setups: Vec<VsaSetup> = Vec::new();

    let mut state = State::default();

    for i in 0..n {
        let bar = bars[i];
        let atr = atr[i];
        let average = volume_average[i];

        // ---------- 20_volume -------------------------------------------------
        series.session_ok[i] =
            !params.session_enabled || ta::session_ok(bar.time, &params.session, &params.timezone);
        let event_ok =
            (params.volume_min <= 0.0 || volume[i] >= params.volume_min) && series.session_ok[i];
        series.event_ok[i] = event_ok;

        series.bucket[i] = ta::vsa_bucket(volume[i], average, &ratios);

        let is_purple_raw = match average {
            Some(average) if average > 0.0 => volume[i] >= average * params.r_purple,
            _ => false,
        };
        series.is_purple_raw[i] = is_purple_raw;
        let is_purple = is_purple_raw && event_ok;
        series.is_purple[i] = is_purple;

        let quiet_setup = match average {
            Some(average) if average > 0.0 => {
                volume[i] < average * params.test_ratio && !is_purple_raw && event_ok
            }
            _ => false,
        };
        series.quiet_setup[i] = quiet_setup;
        let quiet_low = match average {
            Some(average) if average > 0.0 => volume[i] < average * params.r_normal && event_ok,
            _ => false,
        };
        series.quiet_low[i] = quiet_low;

        if let Some(atr) = atr.filter(|atr| *atr > 0.0).filter(|_| close[i] > 0.0) {
            let atr_percent = atr / close[i];
            if params.stop_buffer > 0.0 {
                let round_trip = (params.fee_percent / 100.0) * 2.0;
                let fee_r = round_trip / (params.stop_buffer * atr_percent);
                series.fee_r[i] = Some(fee_r);
                series.fee_ok[i] = fee_r <= params.fee_max_r;
            }
        }

        state.effort_run = if quiet_low && effort_move[i] {
            state.effort_run + 1
        } else {
            0
        };
        series.effort_run[i] = state.effort_run;

        if is_purple {
            state.last_purple_bar = Some(i);
        }
        series.last_purple_bar[i] = state.last_purple_bar;

        // ---------- 30_levels -------------------------------------------------
        if let Some(pivot) = pivot_lows[i] {
            state.last_pivot_low = Some(pivot.value);
            state.pivot_low_bar = i;
            state.has_low = true;
        }
        if let Some(pivot) = pivot_highs[i] {
            state.last_pivot_high = Some(pivot.value);
            state.pivot_high_bar = i;
            state.has_high = true;
        }
        let low_fresh = state.has_low && i - state.pivot_low_bar <= params.level_fresh;
        let high_fresh = state.has_high && i - state.pivot_high_bar <= params.level_fresh;
        series.low_fresh[i] = low_fresh;
        series.high_fresh[i] = high_fresh;

        let near_support = low_fresh
            && atr.is_some()
            && state
                .last_pivot_low
                .is_some_and(|level| low[i] <= level + params.level_tolerance * atr.unwrap());
        let near_resistance = high_fresh
            && atr.is_some()
            && state
                .last_pivot_high
                .is_some_and(|level| high[i] >= level - params.level_tolerance * atr.unwrap());
        series.near_support[i] = near_support;
        series.near_resistance[i] = near_resistance;

        let run_down = i >= params.run && close[i] < close[i - params.run];
        let run_up = i >= params.run && close[i] > close[i - params.run];
        series.run_down[i] = run_down;
        series.run_up[i] = run_up;

        // ---------- 40_events -------------------------------------------------
        let signal_sv = params.events_enabled
            && is_purple
            && run_down
            && near_support
            && bar.high > bar.low
            && bar.close > bar.open;
        let signal_bc = params.events_enabled
            && is_purple
            && run_up
            && near_resistance
            && bar.high > bar.low
            && (bar.high - bar.close) >= 0.5 * (bar.high - bar.low);
        flags.sv[i] = signal_sv;
        flags.bc[i] = signal_bc;

        if signal_sv {
            state.sv_bar = i;
            state.sv_price = Some(low[i]);
            state.has_sv = true;
        }
        if signal_bc {
            state.bc_bar = i;
            state.bc_price = Some(high[i]);
            state.has_bc = true;
        }
        let sv_fresh = state.has_sv && i - state.sv_bar <= params.level_fresh;
        let bc_fresh = state.has_bc && i - state.bc_bar <= params.level_fresh;
        series.sv_fresh[i] = sv_fresh;
        series.bc_fresh[i] = bc_fresh;

        let pivot_low = state.last_pivot_low;
        let pivot_high = state.last_pivot_high;
        let spread = bar.high - bar.low;
        let strong_volume = match average {
            Some(average) if average > 0.0 => volume[i] >= average * params.sos_volume,
            _ => false,
        };

        let spring = atr.is_some()
            && low_fresh
            && pivot_low
                .is_some_and(|level| bar.low < level && bar.close > level && bar.close > bar.open);
        let utad = atr.is_some()
            && high_fresh
            && pivot_high
                .is_some_and(|level| bar.high > level && bar.close < level && bar.close < bar.open);
        let sos = atr.is_some()
            && high_fresh
            && pivot_high.is_some_and(|level| bar.close > level)
            && spread >= params.sos_spread * atr.unwrap()
            && strong_volume
            && bar.close > bar.open;
        let sow = atr.is_some()
            && low_fresh
            && pivot_low.is_some_and(|level| bar.close < level)
            && spread >= params.sos_spread * atr.unwrap()
            && strong_volume
            && bar.close < bar.open;
        series.spring[i] = spring;
        series.utad[i] = utad;
        series.sos[i] = sos;
        series.sow[i] = sow;

        let lps = atr.is_some()
            && state.sos_done
            && state.sos_low.is_some_and(|sos_low| {
                i > state.sos_bar
                    && bar.low > sos_low
                    && bar.low <= sos_low + params.lps_tolerance * atr.unwrap()
                    && bar.close > bar.open
            });
        let lpsy = atr.is_some()
            && state.sow_done
            && state.sow_high.is_some_and(|sow_high| {
                i > state.sow_bar
                    && bar.high < sow_high
                    && bar.high >= sow_high - params.lps_tolerance * atr.unwrap()
                    && bar.close < bar.open
            });
        series.lps[i] = lps;
        series.lpsy[i] = lpsy;

        if sos {
            state.sos_done = true;
            state.sos_bar = i;
            state.sos_low = Some(bar.low);
        }
        if sow {
            state.sow_done = true;
            state.sow_bar = i;
            state.sow_high = Some(bar.high);
        }

        let wyckoff_mode = params.signal_mode == SignalMode::Wyckoff;
        let (signal_st_long, signal_st_short) = match params.signal_mode {
            SignalMode::Wyckoff => {
                let (long_event, short_event) = match params.wyckoff_event {
                    WyckoffEvent::Spring => (spring, utad),
                    WyckoffEvent::Sos => (sos, sow),
                    WyckoffEvent::Lps => (lps, lpsy),
                };
                (
                    params.events_enabled && long_event && sv_fresh,
                    params.events_enabled && short_event && bc_fresh,
                )
            }
            SignalMode::Vsa => (
                params.events_enabled
                    && sv_fresh
                    && quiet_setup
                    && atr.is_some_and(|atr| {
                        state.sv_price.is_some_and(|price| {
                            (low[i] - price).abs() <= params.retest_tolerance * atr
                        })
                    })
                    && bar.close > bar.open,
                params.events_enabled
                    && bc_fresh
                    && quiet_setup
                    && atr.is_some_and(|atr| {
                        state.bc_price.is_some_and(|price| {
                            (high[i] - price).abs() <= params.retest_tolerance * atr
                        })
                    })
                    && bar.close < bar.open,
            ),
        };
        flags.st_long[i] = signal_st_long;
        flags.st_short[i] = signal_st_short;

        let entry_market = params.entry_mode == EntryMode::Market || wyckoff_mode;
        if signal_st_long
            && state.has_sv
            && let (Some(atr), Some(sv_price)) = (atr, state.sv_price)
        {
            let entry = if entry_market { bar.close } else { sv_price };
            let stop_base = if wyckoff_mode { low[i] } else { sv_price };
            let stop_loss = stop_base - params.stop_buffer * atr;
            let risk = entry - stop_loss;
            if risk > 0.0 {
                let take_profit = match params.take_profit_mode {
                    TpMode::Pivot => state
                        .last_pivot_high
                        .filter(|level| *level >= entry + risk)
                        .unwrap_or(entry + params.reward_risk * risk),
                    TpMode::R => entry + params.reward_risk * risk,
                };
                setups.push(VsaSetup {
                    bar: i,
                    direction: 1,
                    entry,
                    stop_loss,
                    take_profit,
                    risk,
                    filled: entry_market,
                    fill_bar: if entry_market { i } else { 0 },
                    done_bar: None,
                    result: None,
                });
                state.begin_setup(Level {
                    entry,
                    stop_loss,
                    take_profit,
                    bar: i,
                    direction: 1,
                    filled: entry_market,
                    fill_bar: if entry_market { i } else { 0 },
                    done: false,
                    result: None,
                });
            }
        }
        if signal_st_short
            && state.has_bc
            && let (Some(atr), Some(bc_price)) = (atr, state.bc_price)
        {
            let entry = if entry_market { bar.close } else { bc_price };
            let stop_base = if wyckoff_mode { high[i] } else { bc_price };
            let stop_loss = stop_base + params.stop_buffer * atr;
            let risk = stop_loss - entry;
            if risk > 0.0 {
                let take_profit = match params.take_profit_mode {
                    TpMode::Pivot => state
                        .last_pivot_low
                        .filter(|level| *level <= entry - risk)
                        .unwrap_or(entry - params.reward_risk * risk),
                    TpMode::R => entry - params.reward_risk * risk,
                };
                setups.push(VsaSetup {
                    bar: i,
                    direction: -1,
                    entry,
                    stop_loss,
                    take_profit,
                    risk,
                    filled: entry_market,
                    fill_bar: if entry_market { i } else { 0 },
                    done_bar: None,
                    result: None,
                });
                state.begin_setup(Level {
                    entry,
                    stop_loss,
                    take_profit,
                    bar: i,
                    direction: -1,
                    filled: entry_market,
                    fill_bar: if entry_market { i } else { 0 },
                    done: false,
                    result: None,
                });
            }
        }

        // Auto-close the active setup. The order matters: the entry is filled
        // first, and the stop wins over the target inside the same bar.
        if let Some(level) = state.level.as_mut()
            && !level.done
            && i > level.bar
        {
            if !level.filled {
                let filled = match level.direction {
                    1 => bar.low <= level.entry,
                    _ => bar.high >= level.entry,
                };
                if filled {
                    level.filled = true;
                    level.fill_bar = i;
                }
            }
            if level.filled {
                if let Some(setup) = setups.last_mut()
                    && setup.bar == level.bar
                    && !setup.filled
                {
                    setup.filled = true;
                    setup.fill_bar = level.fill_bar;
                }
                let outcome = match level.direction {
                    1 => {
                        if bar.low <= level.stop_loss {
                            Some(VsaResult::StopLoss)
                        } else if bar.high >= level.take_profit {
                            Some(VsaResult::TakeProfit)
                        } else {
                            None
                        }
                    }
                    _ => {
                        if bar.high >= level.stop_loss {
                            Some(VsaResult::StopLoss)
                        } else if bar.low <= level.take_profit {
                            Some(VsaResult::TakeProfit)
                        } else {
                            None
                        }
                    }
                };
                if let Some(outcome) = outcome {
                    level.done = true;
                    level.result = Some(outcome);
                    if let Some(setup) = setups.last_mut()
                        && setup.bar == level.bar
                    {
                        setup.done_bar = Some(i);
                        setup.result = Some(outcome);
                        setup.filled = true;
                        setup.fill_bar = level.fill_bar;
                    }
                }
            }
        }

        let no_supply = params.quiet_enabled && quiet_low && near_support && bar.close > bar.open;
        let no_demand =
            params.quiet_enabled && quiet_low && near_resistance && bar.close < bar.open;
        flags.no_supply[i] = no_supply;
        flags.no_demand[i] = no_demand;

        let any_event =
            signal_sv || signal_bc || signal_st_long || signal_st_short || no_supply || no_demand;
        if any_event {
            state.last_event = Some(if signal_sv {
                "SV"
            } else if signal_bc {
                "BC"
            } else if signal_st_long || signal_st_short {
                "ST"
            } else if no_supply {
                "NS"
            } else {
                "ND"
            });
            state.last_event_bar = i;
            events.push(VsaEvent {
                bar: i,
                kind: if signal_sv {
                    VsaEventKind::Sv
                } else if signal_bc {
                    VsaEventKind::Bc
                } else if signal_st_long {
                    VsaEventKind::StLong
                } else if signal_st_short {
                    VsaEventKind::StShort
                } else if no_supply {
                    VsaEventKind::Ns
                } else {
                    VsaEventKind::Nd
                },
            });
        }
        series.last_event[i] = state.last_event;
        series.last_event_bar[i] = state.last_event_bar;
    }

    series.purple_count = ta::rolling_sum(
        &series
            .is_purple
            .iter()
            .map(|purple| Some(if *purple { 1.0 } else { 0.0 }))
            .collect::<Vec<_>>(),
        params.purple_window,
    );

    let mut scores = vec![0.0; n];
    for event in &events {
        scores[event.bar] = event.kind.score();
    }

    VsaOutput {
        params: params.clone(),
        series,
        flags,
        events,
        setups,
        scores,
    }
}

fn finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

/// The active setup of the legacy engine.
#[derive(Debug, Clone, Copy)]
struct Level {
    entry: f64,
    stop_loss: f64,
    take_profit: f64,
    bar: usize,
    direction: i8,
    filled: bool,
    fill_bar: usize,
    done: bool,
    result: Option<VsaResult>,
}

/// The `var` state of the legacy Pine script.
#[derive(Debug, Default)]
struct State {
    effort_run: usize,
    last_purple_bar: Option<usize>,
    last_pivot_low: Option<f64>,
    last_pivot_high: Option<f64>,
    pivot_low_bar: usize,
    pivot_high_bar: usize,
    has_low: bool,
    has_high: bool,
    sv_bar: usize,
    sv_price: Option<f64>,
    has_sv: bool,
    bc_bar: usize,
    bc_price: Option<f64>,
    has_bc: bool,
    sos_done: bool,
    sos_bar: usize,
    sos_low: Option<f64>,
    sow_done: bool,
    sow_bar: usize,
    sow_high: Option<f64>,
    last_event: Option<&'static str>,
    last_event_bar: usize,
    level: Option<Level>,
}

impl State {
    fn begin_setup(&mut self, level: Level) {
        self.level = Some(level);
    }
}
