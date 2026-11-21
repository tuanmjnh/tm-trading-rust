use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiTaskRouteDto {
    pub task: String,
    pub primary: String,
    pub fallbacks: Vec<String>,
    pub enabled: bool,
    pub timeout_ms: u64,
    pub max_attempts: u8,
    pub concurrency: u8,
    pub max_output_tokens: Option<u32>,
    pub providers_in_reach: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiTaskRouteListDto {
    pub generated_at: DateTime<Utc>,
    pub count: usize,
    pub routes: Vec<AiTaskRouteDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NewAiTaskRouteDto {
    pub task: String,
    pub primary: String,
    #[serde(default)]
    pub fallbacks: Vec<String>,
    pub timeout_ms: u64,
    pub max_attempts: u8,
    pub concurrency: u8,
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct UpdateAiTaskRouteDto {
    pub primary: Option<String>,
    pub fallbacks: Option<Vec<String>>,
    pub enabled: Option<bool>,
    pub timeout_ms: Option<u64>,
    pub max_attempts: Option<u8>,
    pub concurrency: Option<u8>,
    pub max_output_tokens: Option<u32>,
}
