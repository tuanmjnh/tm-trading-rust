//! Deterministic research methods ported from the legacy `engine/methods`.
//!
//! A method turns closed bars into observations. It never places orders and it
//! never talks to the network: the Research Worker stores what the method
//! returns, and the Desktop only displays it.

pub mod price_action;
pub mod sweep;
pub mod trend;
pub mod vsa;
pub mod wonderwhales;

pub use price_action::{
    ID as PRICE_ACTION_ID, NAME as PRICE_ACTION_NAME, PriceActionEvent, PriceActionParams,
    PriceActionPatternKind, PriceActionResult, PriceActionSignal, analyze_price_action,
};
pub use sweep::{
    ID as SWEEP_ID, NAME as SWEEP_NAME, SweepEvent, SweepEventKind, SweepParams, SweepResult,
    SweepSignal, analyze_sweep,
};
pub use trend::{
    ID as TREND_ID, NAME as TREND_NAME, TrendEvent, TrendEventKind, TrendParams, TrendResult,
    TrendSignal, analyze_trend,
};
pub use vsa::{
    EntryMode, ID as VSA_ID, NAME as VSA_NAME, SignalMode, TpMode, VsaEvent, VsaEventKind,
    VsaFlags, VsaOutput, VsaParams, VsaResult, VsaSeries, VsaSetup, WyckoffEvent,
    analyze as analyze_vsa,
};
pub use wonderwhales::{
    ID as WONDERWHALES_ID, NAME as WONDERWHALES_NAME, WonderWhalesEvent, WonderWhalesEventKind,
    WonderWhalesMode, WonderWhalesParams, WonderWhalesResult, WonderWhalesSignal,
    analyze_wonderwhales,
};
