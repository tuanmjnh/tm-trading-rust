use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiProviderProfileDto {
    pub id: String,
    pub kind: String,
    pub display_name: String,
    pub enabled: bool,
    pub base_url: String,
    pub default_model: String,
    pub timeout_ms: u64,
    pub api_key_env: String,
    pub health_state: String,
    pub last_health_check_at: Option<DateTime<Utc>>,
    pub last_error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AiProviderProfileListDto {
    pub generated_at: DateTime<Utc>,
    pub count: usize,
    pub profiles: Vec<AiProviderProfileDto>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct NewAiProviderProfileDto {
    pub id: String,
    pub kind: String,
    pub display_name: String,
    pub base_url: String,
    pub default_model: String,
    pub timeout_ms: u64,
    pub api_key_env: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct UpdateAiProviderProfileDto {
    pub display_name: Option<String>,
    pub base_url: Option<String>,
    pub default_model: Option<String>,
    pub timeout_ms: Option<u64>,
    pub enabled: Option<bool>,
    pub api_key_env: Option<String>,
}
