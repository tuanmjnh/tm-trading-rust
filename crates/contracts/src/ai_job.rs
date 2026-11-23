use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Task a queued AI job will run.
///
/// The queue currently exposes only the research-explanation task. Other AI task
/// kinds remain domain concepts for their dedicated providers and briefings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiJobTaskDto {
    ResearchExplanation,
}

/// Durable state of one queued AI job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AiJobStatusDto {
    Queued,
    Claimed,
    Succeeded,
    Failed,
    Expired,
    OutcomeUnknown,
}

/// One owner-scoped AI job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiJobDto {
    pub id: String,
    pub owner_scope: String,
    pub correlation_id: String,
    pub task: AiJobTaskDto,
    pub status: AiJobStatusDto,
    pub attempt: u32,
    pub research_event_ids: Vec<String>,
    pub news_item_ids: Vec<String>,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub briefing_id: Option<String>,
    pub error_code: Option<String>,
}

/// A bounded list of owner-scoped AI jobs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiJobListDto {
    pub generated_at: DateTime<Utc>,
    pub count: usize,
    pub jobs: Vec<AiJobDto>,
}

/// Request to queue one evidence-bound briefing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewAiJobDto {
    pub task: AiJobTaskDto,
    pub research_event_ids: Vec<String>,
    pub news_item_ids: Vec<String>,
    /// Optional replay key. Without it, an identical request reuses its own job.
    #[schema(max_length = 200)]
    pub idempotency_key: Option<String>,
}

/// Optional filters for an AI job list request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiJobQueryDto {
    pub task: Option<AiJobTaskDto>,
    #[schema(minimum = 1, maximum = 200)]
    pub limit: Option<usize>,
}
