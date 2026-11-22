use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Task that produced an AI briefing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiBriefingTaskDto {
    NewsClassification,
    NewsSummary,
    MarketBriefing,
    ResearchExplanation,
    DailyDigest,
}

/// Kind of source cited by a briefing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiBriefingReferenceKindDto {
    ResearchEvent,
    NewsItem,
    SourceUrl,
}

/// One exact source reference cited by a briefing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AiBriefingReferenceDto {
    pub kind: AiBriefingReferenceKindDto,
    pub value: String,
}

/// One evidence-bound AI briefing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiBriefingDto {
    pub id: String,
    pub owner_scope: String,
    pub task: AiBriefingTaskDto,
    pub provider_id: String,
    pub model_registry_id: String,
    pub model_id: String,
    pub prompt_version: String,
    pub facts: Vec<String>,
    pub interpretations: Vec<String>,
    pub uncertainties: Vec<String>,
    pub input_snapshot_ids: Vec<String>,
    pub references: Vec<AiBriefingReferenceDto>,
    pub missing_inputs: Vec<String>,
    pub stale_inputs: Vec<String>,
    pub generated_at: DateTime<Utc>,
    pub producer_version: String,
    pub schema_version: String,
}

/// A bounded list of owner-scoped briefings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiBriefingListDto {
    pub generated_at: DateTime<Utc>,
    pub count: usize,
    pub briefings: Vec<AiBriefingDto>,
}

/// Optional filters for a briefing list request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiBriefingQueryDto {
    pub task: Option<AiBriefingTaskDto>,
    #[schema(minimum = 1, maximum = 200)]
    pub limit: Option<usize>,
}
