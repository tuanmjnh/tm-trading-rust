use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use thiserror::Error;

/// Schema stamp written to every research event document.
pub const RESEARCH_EVENT_SCHEMA_VERSION: &str = "research-event.v1";

/// Prefix of a research event identifier.
pub const RESEARCH_EVENT_ID_PREFIX: &str = "rev1_";

/// Prefix of an input snapshot identifier.
pub const RESEARCH_SNAPSHOT_ID_PREFIX: &str = "rsnap1_";

/// Producer that wrote the event. It changes whenever the calculation changes.
pub const RESEARCH_PRODUCER_VERSION: &str = "tm-trading-research-engine/0.1.0";

/// How complete the input of a research event was.
///
/// The state describes the evidence, not the market: `Complete` means the
/// method had every input it needed, `Partial` means some optional input was
/// missing, and `Unknown` means the event could not be evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResearchQuality {
    Complete,
    Partial,
    Unknown,
}

impl ResearchQuality {
    /// The stored label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Unknown => "unknown",
        }
    }

    /// Reads a stored label.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "complete" => Some(Self::Complete),
            "partial" => Some(Self::Partial),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

/// The fields a producer supplies for one research event.
#[derive(Debug, Clone, PartialEq)]
pub struct NewResearchEvent {
    pub owner_scope: String,
    pub symbol: String,
    pub timeframe: String,
    pub method: String,
    pub method_version: String,
    pub event_type: String,
    /// `-1` for a bearish observation, `1` for a bullish one, `0` for neutral.
    pub direction: i8,
    pub score: f64,
    /// When the underlying market observation happened.
    pub observed_at: DateTime<Utc>,
    /// When the method detected the event.
    pub detected_at: DateTime<Utc>,
    /// When the event was written.
    pub ingested_at: DateTime<Utc>,
    /// The market source the input came from.
    pub source_id: String,
    /// The provider method that produced the input.
    pub provider_method: String,
    /// Identifier of the exact input window the calculation used.
    pub input_snapshot_id: String,
    pub quality_state: ResearchQuality,
    pub quality_reasons: Vec<String>,
    /// Numeric evidence behind the observation.
    pub evidence: BTreeMap<String, f64>,
    /// Textual context behind the observation.
    pub context: BTreeMap<String, String>,
}

/// One research observation with its full provenance.
///
/// A research event is information, never an instruction. It records what a
/// deterministic method observed, on which input, and how good that input was.
#[derive(Debug, Clone, PartialEq)]
pub struct ResearchEvent {
    id: String,
    owner_scope: String,
    symbol: String,
    timeframe: String,
    method: String,
    method_version: String,
    event_type: String,
    direction: i8,
    score: f64,
    observed_at: DateTime<Utc>,
    detected_at: DateTime<Utc>,
    ingested_at: DateTime<Utc>,
    source_id: String,
    provider_method: String,
    input_snapshot_id: String,
    producer_version: String,
    schema_version: String,
    quality_state: ResearchQuality,
    quality_reasons: Vec<String>,
    evidence: BTreeMap<String, f64>,
    context: BTreeMap<String, String>,
}

impl ResearchEvent {
    /// Builds an event and derives its stable identifier.
    ///
    /// The identifier is derived from the owner, symbol, timeframe, method,
    /// event type and detection time, so re-running the same method over the
    /// same input never produces a second copy of the same observation.
    pub fn new(input: NewResearchEvent) -> Result<Self, ResearchEventError> {
        let owner_scope = required("ownerScope", input.owner_scope)?;
        let symbol = required("symbol", input.symbol)?;
        let timeframe = required("timeframe", input.timeframe)?;
        let method = required("method", input.method)?;
        let method_version = required("methodVersion", input.method_version)?;
        let event_type = required("eventType", input.event_type)?;
        let source_id = required("sourceId", input.source_id)?;
        let provider_method = required("providerMethod", input.provider_method)?;
        let input_snapshot_id = required("inputSnapshotId", input.input_snapshot_id)?;
        if !(-1..=1).contains(&input.direction) {
            return Err(ResearchEventError::InvalidDirection(input.direction));
        }
        if !input.score.is_finite() {
            return Err(ResearchEventError::InvalidScore);
        }
        if input.evidence.values().any(|value| !value.is_finite()) {
            return Err(ResearchEventError::InvalidEvidence);
        }
        if input.detected_at < input.observed_at {
            return Err(ResearchEventError::DetectedBeforeObserved);
        }
        if input.ingested_at < input.detected_at {
            return Err(ResearchEventError::IngestedBeforeDetected);
        }
        let id = event_id(
            &owner_scope,
            &symbol,
            &timeframe,
            &method,
            &method_version,
            &event_type,
            input.detected_at,
        );
        Ok(Self {
            id,
            owner_scope,
            symbol,
            timeframe,
            method,
            method_version,
            event_type,
            direction: input.direction,
            score: input.score,
            observed_at: input.observed_at,
            detected_at: input.detected_at,
            ingested_at: input.ingested_at,
            source_id,
            provider_method,
            input_snapshot_id,
            producer_version: RESEARCH_PRODUCER_VERSION.to_owned(),
            schema_version: RESEARCH_EVENT_SCHEMA_VERSION.to_owned(),
            quality_state: input.quality_state,
            quality_reasons: input.quality_reasons,
            evidence: input.evidence,
            context: input.context,
        })
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn owner_scope(&self) -> &str {
        &self.owner_scope
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
    pub fn method(&self) -> &str {
        &self.method
    }

    #[must_use]
    pub fn method_version(&self) -> &str {
        &self.method_version
    }

    #[must_use]
    pub fn event_type(&self) -> &str {
        &self.event_type
    }

    #[must_use]
    pub const fn direction(&self) -> i8 {
        self.direction
    }

    #[must_use]
    pub const fn score(&self) -> f64 {
        self.score
    }

    #[must_use]
    pub const fn observed_at(&self) -> DateTime<Utc> {
        self.observed_at
    }

    #[must_use]
    pub const fn detected_at(&self) -> DateTime<Utc> {
        self.detected_at
    }

    #[must_use]
    pub const fn ingested_at(&self) -> DateTime<Utc> {
        self.ingested_at
    }

    #[must_use]
    pub fn source_id(&self) -> &str {
        &self.source_id
    }

    #[must_use]
    pub fn provider_method(&self) -> &str {
        &self.provider_method
    }

    #[must_use]
    pub fn input_snapshot_id(&self) -> &str {
        &self.input_snapshot_id
    }

    #[must_use]
    pub fn producer_version(&self) -> &str {
        &self.producer_version
    }

    #[must_use]
    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }

    #[must_use]
    pub const fn quality_state(&self) -> ResearchQuality {
        self.quality_state
    }

    #[must_use]
    pub fn quality_reasons(&self) -> &[String] {
        &self.quality_reasons
    }

    #[must_use]
    pub fn evidence(&self) -> &BTreeMap<String, f64> {
        &self.evidence
    }

    #[must_use]
    pub fn context(&self) -> &BTreeMap<String, String> {
        &self.context
    }
}

/// Stable identifier of one research event.
#[must_use]
pub fn event_id(
    owner_scope: &str,
    symbol: &str,
    timeframe: &str,
    method: &str,
    method_version: &str,
    event_type: &str,
    detected_at: DateTime<Utc>,
) -> String {
    let canonical = format!(
        "{owner_scope}|{symbol}|{timeframe}|{method}|{method_version}|{event_type}|{}",
        detected_at.timestamp_millis()
    );
    format!(
        "{RESEARCH_EVENT_ID_PREFIX}{:016x}",
        stable_hash(canonical.as_bytes())
    )
}

/// Stable identifier of the input window a calculation used.
///
/// The identifier covers the input range and the method that consumed it, so
/// the same window replayed by a different method is a different snapshot.
#[must_use]
pub fn input_snapshot_id(
    symbol: &str,
    timeframe: &str,
    method: &str,
    method_version: &str,
    first_open_time: i64,
    last_open_time: i64,
    bar_count: usize,
) -> String {
    let canonical = format!(
        "{symbol}|{timeframe}|{method}|{method_version}|{first_open_time}|{last_open_time}|{bar_count}"
    );
    format!(
        "{RESEARCH_SNAPSHOT_ID_PREFIX}{:016x}",
        stable_hash(canonical.as_bytes())
    )
}

/// FNV-1a over the canonical form.
///
/// The digest only has to be stable and collision-resistant enough to key one
/// observation, so a dependency-free hash is enough.
fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn required(field: &'static str, value: String) -> Result<String, ResearchEventError> {
    if value.trim().is_empty() {
        return Err(ResearchEventError::EmptyField(field));
    }
    Ok(value.trim().to_owned())
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ResearchEventError {
    #[error("research event field '{0}' must not be empty")]
    EmptyField(&'static str),
    #[error("research event direction {0} is out of range")]
    InvalidDirection(i8),
    #[error("research event score must be finite")]
    InvalidScore,
    #[error("research event evidence must contain finite values only")]
    InvalidEvidence,
    #[error("a research event cannot be detected before it was observed")]
    DetectedBeforeObserved,
    #[error("a research event cannot be ingested before it was detected")]
    IngestedBeforeDetected,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, 8, 0, 0).unwrap()
    }

    fn input() -> NewResearchEvent {
        NewResearchEvent {
            owner_scope: "local-operator".to_owned(),
            symbol: "BTCUSDT".to_owned(),
            timeframe: "15m".to_owned(),
            method: "vsa".to_owned(),
            method_version: "1".to_owned(),
            event_type: "ST LONG".to_owned(),
            direction: 1,
            score: 1.0,
            observed_at: at(),
            detected_at: at(),
            ingested_at: at(),
            source_id: "binance".to_owned(),
            provider_method: "binance.klines".to_owned(),
            input_snapshot_id: input_snapshot_id("BTCUSDT", "15m", "vsa", "1", 1, 2, 2),
            quality_state: ResearchQuality::Complete,
            quality_reasons: Vec::new(),
            evidence: BTreeMap::from([("close".to_owned(), 100.0)]),
            context: BTreeMap::from([("bucket".to_owned(), "TIM".to_owned())]),
        }
    }

    #[test]
    fn the_identifier_is_stable_for_the_same_observation() {
        let first = ResearchEvent::new(input()).unwrap();
        let second = ResearchEvent::new(input()).unwrap();
        assert_eq!(first.id(), second.id());
        assert!(first.id().starts_with(RESEARCH_EVENT_ID_PREFIX));
    }

    #[test]
    fn a_different_detection_time_is_a_different_event() {
        let first = ResearchEvent::new(input()).unwrap();
        let mut later = input();
        later.detected_at = at() + chrono::Duration::minutes(15);
        later.ingested_at = later.detected_at;
        let second = ResearchEvent::new(later).unwrap();
        assert_ne!(first.id(), second.id());
    }

    #[test]
    fn the_input_snapshot_covers_the_method_and_the_window() {
        let one = input_snapshot_id("BTCUSDT", "15m", "vsa", "1", 1, 2, 2);
        let other_method = input_snapshot_id("BTCUSDT", "15m", "trend", "1", 1, 2, 2);
        let other_window = input_snapshot_id("BTCUSDT", "15m", "vsa", "1", 1, 3, 3);
        assert_ne!(one, other_method);
        assert_ne!(one, other_window);
        assert_eq!(
            one,
            input_snapshot_id("BTCUSDT", "15m", "vsa", "1", 1, 2, 2)
        );
    }

    #[test]
    fn rejects_an_out_of_range_direction_and_a_non_finite_score() {
        let mut bad_direction = input();
        bad_direction.direction = 2;
        assert_eq!(
            ResearchEvent::new(bad_direction),
            Err(ResearchEventError::InvalidDirection(2))
        );

        let mut bad_score = input();
        bad_score.score = f64::NAN;
        assert_eq!(
            ResearchEvent::new(bad_score),
            Err(ResearchEventError::InvalidScore)
        );
    }

    #[test]
    fn rejects_an_empty_identity_field() {
        let mut blank = input();
        blank.symbol = "  ".to_owned();
        assert_eq!(
            ResearchEvent::new(blank),
            Err(ResearchEventError::EmptyField("symbol"))
        );
    }

    #[test]
    fn rejects_a_detection_before_the_observation() {
        let mut inverted = input();
        inverted.detected_at = at() - chrono::Duration::minutes(1);
        assert_eq!(
            ResearchEvent::new(inverted),
            Err(ResearchEventError::DetectedBeforeObserved)
        );
    }
}
