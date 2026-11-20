use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Editorial significance of one story.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NewsLevelDto {
    High,
    Med,
    Low,
}

/// Direction the story is read as having for the market.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NewsDirectionDto {
    Pos,
    Neg,
    Flat,
}

/// Whether a story is recent enough for a live desk.
///
/// Freshness is computed when the story is read, because it depends on the
/// current time rather than on the moment the story was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NewsFreshnessDto {
    Fresh,
    Stale,
}

/// Observed health of one news source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NewsHealthStateDto {
    Ok,
    Degraded,
    Down,
    Unknown,
}

/// One rights-safe normalized story.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NewsItemDto {
    pub id: String,
    pub source_id: String,
    /// Citation link exactly as the publisher exposed it.
    pub url: String,
    pub title: String,
    pub excerpt: String,
    pub language: String,
    /// Provider publication time; absent when the feed carries no usable date.
    pub published_at: Option<DateTime<Utc>>,
    pub first_seen_at: DateTime<Utc>,
    pub ingested_at: DateTime<Utc>,
    pub market_group: String,
    pub event_type: Option<String>,
    pub tags: Vec<String>,
    pub level: NewsLevelDto,
    pub direction: NewsDirectionDto,
    pub score: f64,
    pub freshness: NewsFreshnessDto,
    pub age_seconds: i64,
}

/// A bounded list of normalized stories.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NewsListDto {
    pub generated_at: DateTime<Utc>,
    pub count: usize,
    /// Age after which a story is reported as stale.
    pub stale_after_seconds: i64,
    pub items: Vec<NewsItemDto>,
}

/// Optional filters for a news list request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NewsQueryDto {
    pub source_id: Option<String>,
    pub market_group: Option<String>,
    pub level: Option<NewsLevelDto>,
    /// Only items ingested at or after this instant.
    pub since: Option<DateTime<Utc>>,
    /// Maximum number of items to return.
    pub limit: Option<usize>,
}

/// One configured news source and the health it last reported.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NewsSourceDto {
    pub id: String,
    pub name: String,
    pub provider: Option<String>,
    pub transport: String,
    pub trust_tier: String,
    pub enabled: bool,
    /// Public feed endpoint; `null` when the source has no safe feed URL.
    pub feed_url: Option<String>,
    pub market_groups: Vec<String>,
    pub languages: Vec<String>,
    pub poll_interval_sec: u32,
    pub retention_days: u32,
    pub health_state: NewsHealthStateDto,
    pub last_fetch_at: Option<DateTime<Utc>>,
    pub last_ok_at: Option<DateTime<Utc>>,
    pub last_item_at: Option<DateTime<Utc>>,
    /// Bounded diagnostic code; raw provider text is never returned.
    pub last_error: Option<String>,
    pub consecutive_errors: u32,
}

/// The configured news source registry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NewsSourceListDto {
    pub generated_at: DateTime<Utc>,
    pub count: usize,
    pub sources: Vec<NewsSourceDto>,
}

/// One cluster of stories that describe the same event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NewsClusterDto {
    pub id: String,
    pub item_ids: Vec<String>,
    pub item_count: usize,
    pub tokens: Vec<String>,
    pub market_group: String,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

/// A bounded list of news event clusters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NewsClusterListDto {
    pub generated_at: DateTime<Utc>,
    pub count: usize,
    pub clusters: Vec<NewsClusterDto>,
}

/// Optional filters for a news cluster list request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NewsClusterQueryDto {
    pub market_group: Option<String>,
    /// Only clusters whose last observation is at or after this instant.
    pub since: Option<DateTime<Utc>>,
    /// Maximum number of clusters to return.
    pub limit: Option<usize>,
}
