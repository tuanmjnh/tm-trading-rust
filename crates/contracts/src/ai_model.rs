use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiModelDto {
    pub id: String,
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub context_window: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub supports_streaming: bool,
    pub supports_json_mode: bool,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub enabled: bool,
    pub health_state: String,
    pub last_health_check_at: Option<DateTime<Utc>>,
    pub last_error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiModelListDto {
    pub generated_at: DateTime<Utc>,
    pub count: usize,
    pub models: Vec<AiModelDto>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NewAiModelDto {
    pub id: String,
    pub model_id: String,
    pub display_name: String,
    pub context_window: Option<u32>,
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub supports_streaming: bool,
    #[serde(default)]
    pub supports_json_mode: bool,
    #[serde(default)]
    pub supports_tools: bool,
    #[serde(default)]
    pub supports_vision: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct UpdateAiModelDto {
    pub display_name: Option<String>,
    pub context_window: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub supports_streaming: Option<bool>,
    pub supports_json_mode: Option<bool>,
    pub supports_tools: Option<bool>,
    pub supports_vision: Option<bool>,
    pub enabled: Option<bool>,
}
