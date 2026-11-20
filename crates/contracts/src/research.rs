use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// How complete the input of a research event was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResearchQualityDto {
    Complete,
    Partial,
    Unknown,
}

/// Whether an observation is still current.
///
/// Freshness is computed when the event is read, because it depends on the
/// current time, not on the moment the event was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResearchFreshnessDto {
    Fresh,
    Stale,
}

/// One research observation with its provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ResearchEventDto {
    pub id: String,
    pub owner_scope: String,
    pub symbol: String,
    pub timeframe: String,
    pub event_type: String,
    pub method: String,
    pub method_version: String,
    /// `-1` for a bearish observation, `1` for a bullish one, `0` for neutral.
    pub direction: i8,
    pub score: f64,
    pub observed_at: DateTime<Utc>,
    pub detected_at: DateTime<Utc>,
    pub ingested_at: DateTime<Utc>,
    pub source_id: String,
    pub provider_method: String,
    pub input_snapshot_id: String,
    pub producer_version: String,
    pub schema_version: String,
    pub quality_state: ResearchQualityDto,
    pub quality_reasons: Vec<String>,
    pub freshness: ResearchFreshnessDto,
    pub age_seconds: i64,
    pub evidence: BTreeMap<String, f64>,
    pub context: BTreeMap<String, String>,
}

/// A bounded list of research observations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ResearchEventListDto {
    pub generated_at: DateTime<Utc>,
    pub count: usize,
    pub events: Vec<ResearchEventDto>,
}

/// Optional filters for a research event list request.
///
/// The owner scope is never accepted from the client: the API always derives it
/// from the authenticated subject.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ResearchEventQueryDto {
    pub symbol: Option<String>,
    pub timeframe: Option<String>,
    pub event_type: Option<String>,
    /// Only events detected at or after this instant.
    pub since: Option<DateTime<Utc>>,
    /// Maximum number of events to return.
    pub limit: Option<usize>,
}
