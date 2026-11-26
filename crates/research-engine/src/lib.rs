//! Deterministic research primitives.
//!
//! The crate is pure: no I/O, no clock, no provider or storage dependency, so a
//! calculation can be replayed from stored bars and compared against the legacy
//! engine's golden fixtures.

pub mod indicators;
pub mod intelligence;
pub mod methods;
pub mod ta;

/// One closed bar. Indicators read only these fields, so a caller can map any
/// stored candle into this shape without pulling storage types in here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    /// Bar open time in Unix milliseconds, as stored on the candle.
    pub time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}
