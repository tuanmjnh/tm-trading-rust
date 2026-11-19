use thiserror::Error;

/// Schema stamp written to every row of the `candles` collection.
///
/// Matches the legacy recorder so a row written by either implementation is readable
/// by the other.
pub const CANDLE_SCHEMA_VERSION: &str = "candle.v1";

/// A closed bar offered for recording.
///
/// `event_time` defaults to `close_time` and `ingest_time` to `event_time`, which is
/// what the legacy recorder does when the source omits them.
#[derive(Debug, Clone)]
pub struct NewCandle {
    pub symbol: String,
    pub timeframe: String,
    pub market: Option<String>,
    pub source: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub open_time: i64,
    pub close_time: i64,
    pub event_time: Option<i64>,
    pub ingest_time: Option<i64>,
}

/// A recorded closed candle: an append-only fact, never a forming bar.
#[derive(Debug, Clone, PartialEq)]
pub struct Candle {
    id: String,
    schema_version: String,
    symbol: String,
    timeframe: String,
    market: Option<String>,
    source: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    open_time: i64,
    close_time: i64,
    event_time: i64,
    ingest_time: i64,
}

impl Candle {
    /// Builds a candle, rejecting anything that is not a valid closed bar.
    pub fn new(input: NewCandle) -> Result<Self, CandleError> {
        let symbol = required("symbol", input.symbol)?;
        let timeframe = required("timeframe", input.timeframe)?;
        let source = required("source", input.source)?;
        let market = input.market.filter(|value| !value.trim().is_empty());

        for (name, value) in [
            ("open", input.open),
            ("high", input.high),
            ("low", input.low),
            ("close", input.close),
        ] {
            if !value.is_finite() {
                return Err(CandleError::NonFinite(name));
            }
            if value < 0.0 {
                return Err(CandleError::NegativePrice(name));
            }
        }
        if !input.volume.is_finite() {
            return Err(CandleError::NonFinite("volume"));
        }
        if input.volume < 0.0 {
            return Err(CandleError::NegativeVolume);
        }
        if input.close_time <= input.open_time {
            return Err(CandleError::InvalidTimeRange);
        }
        if input.high < input.open.max(input.close) {
            return Err(CandleError::HighBelowBody);
        }
        if input.low > input.open.min(input.close) {
            return Err(CandleError::LowAboveBody);
        }

        let event_time = input.event_time.unwrap_or(input.close_time);
        Ok(Self {
            id: candle_id(&symbol, &timeframe, input.open_time),
            schema_version: CANDLE_SCHEMA_VERSION.to_owned(),
            symbol,
            timeframe,
            market,
            source,
            open: input.open,
            high: input.high,
            low: input.low,
            close: input.close,
            volume: input.volume,
            open_time: input.open_time,
            close_time: input.close_time,
            event_time,
            ingest_time: input.ingest_time.unwrap_or(event_time),
        })
    }

    /// Natural key: `{symbol}:{timeframe}:{openTime}`.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }

    #[must_use]
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    #[must_use]
    pub fn timeframe(&self) -> &str {
        &self.timeframe
    }

    #[must_use]
    pub fn market(&self) -> Option<&str> {
        self.market.as_deref()
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub const fn open(&self) -> f64 {
        self.open
    }

    #[must_use]
    pub const fn high(&self) -> f64 {
        self.high
    }

    #[must_use]
    pub const fn low(&self) -> f64 {
        self.low
    }

    #[must_use]
    pub const fn close(&self) -> f64 {
        self.close
    }

    #[must_use]
    pub const fn volume(&self) -> f64 {
        self.volume
    }

    #[must_use]
    pub const fn open_time(&self) -> i64 {
        self.open_time
    }

    #[must_use]
    pub const fn close_time(&self) -> i64 {
        self.close_time
    }

    #[must_use]
    pub const fn event_time(&self) -> i64 {
        self.event_time
    }

    #[must_use]
    pub const fn ingest_time(&self) -> i64 {
        self.ingest_time
    }
}

/// Natural key for a closed candle row.
#[must_use]
pub fn candle_id(symbol: &str, timeframe: &str, open_time: i64) -> String {
    format!("{symbol}:{timeframe}:{open_time}")
}

fn required(field: &'static str, value: String) -> Result<String, CandleError> {
    if value.trim().is_empty() {
        return Err(CandleError::EmptyField(field));
    }
    Ok(value)
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum CandleError {
    #[error("candle field '{0}' must not be empty")]
    EmptyField(&'static str),
    #[error("candle field '{0}' must be a finite number")]
    NonFinite(&'static str),
    #[error("candle price '{0}' must not be negative")]
    NegativePrice(&'static str),
    #[error("candle volume must not be negative")]
    NegativeVolume,
    #[error("candle close time must be later than its open time")]
    InvalidTimeRange,
    #[error("candle high is below its open or close")]
    HighBelowBody,
    #[error("candle low is above its open or close")]
    LowAboveBody,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> NewCandle {
        NewCandle {
            symbol: "BTCUSDT".to_owned(),
            timeframe: "1m".to_owned(),
            market: Some("spot".to_owned()),
            source: "binance".to_owned(),
            open: 100.0,
            high: 110.0,
            low: 90.0,
            close: 105.0,
            volume: 12.5,
            open_time: 1_700_000_000_000,
            close_time: 1_700_000_059_999,
            event_time: None,
            ingest_time: None,
        }
    }

    #[test]
    fn builds_the_legacy_natural_key() {
        let candle = Candle::new(input()).unwrap();

        assert_eq!(candle.id(), "BTCUSDT:1m:1700000000000");
        assert_eq!(candle.schema_version(), "candle.v1");
    }

    #[test]
    fn event_and_ingest_time_default_forward_from_close_time() {
        let candle = Candle::new(input()).unwrap();

        assert_eq!(candle.event_time(), 1_700_000_059_999);
        assert_eq!(candle.ingest_time(), 1_700_000_059_999);
    }

    #[test]
    fn explicit_ingest_time_is_kept() {
        let mut with_times = input();
        with_times.event_time = Some(1_700_000_060_500);
        with_times.ingest_time = Some(1_700_000_061_000);

        let candle = Candle::new(with_times).unwrap();

        assert_eq!(candle.event_time(), 1_700_000_060_500);
        assert_eq!(candle.ingest_time(), 1_700_000_061_000);
    }

    #[test]
    fn rejects_a_high_below_the_open_or_close() {
        let mut broken = input();
        broken.high = 104.0;

        assert_eq!(Candle::new(broken), Err(CandleError::HighBelowBody));
    }

    #[test]
    fn rejects_a_low_above_the_open_or_close() {
        let mut broken = input();
        broken.low = 101.0;

        assert_eq!(Candle::new(broken), Err(CandleError::LowAboveBody));
    }

    #[test]
    fn rejects_a_non_finite_price() {
        let mut broken = input();
        broken.close = f64::NAN;

        assert_eq!(Candle::new(broken), Err(CandleError::NonFinite("close")));
    }

    #[test]
    fn rejects_a_negative_volume() {
        let mut broken = input();
        broken.volume = -1.0;

        assert_eq!(Candle::new(broken), Err(CandleError::NegativeVolume));
    }

    #[test]
    fn rejects_an_inverted_time_range() {
        let mut broken = input();
        broken.close_time = broken.open_time;

        assert_eq!(Candle::new(broken), Err(CandleError::InvalidTimeRange));
    }

    #[test]
    fn rejects_an_empty_identity_field() {
        let mut broken = input();
        broken.source = "   ".to_owned();

        assert_eq!(Candle::new(broken), Err(CandleError::EmptyField("source")));
    }

    #[test]
    fn an_empty_market_becomes_none() {
        let mut without_market = input();
        without_market.market = Some(String::new());

        assert_eq!(Candle::new(without_market).unwrap().market(), None);
    }
}
