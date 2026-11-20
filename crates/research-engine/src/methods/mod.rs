//! Deterministic research methods ported from the legacy `engine/methods`.
//!
//! A method turns closed bars into observations. It never places orders and it
//! never talks to the network: the Research Worker stores what the method
//! returns, and the Desktop only displays it.

pub mod vsa;

pub use vsa::{
    EntryMode, ID as VSA_ID, NAME as VSA_NAME, SignalMode, TpMode, VsaEvent, VsaEventKind,
    VsaFlags, VsaOutput, VsaParams, VsaResult, VsaSeries, VsaSetup, WyckoffEvent,
    analyze as analyze_vsa,
};
