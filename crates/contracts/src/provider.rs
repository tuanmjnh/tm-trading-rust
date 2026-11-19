use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use tm_trading_domain::{ProviderHealthState, ProviderInstance};

/// One adapter compiled into the running service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ProviderDefinitionDto {
    pub id: String,
    pub display_name: String,
    pub default_base_url: String,
    pub timeframes: Vec<String>,
}

/// Last recorded health of a configured provider instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderHealthStateDto {
    Unknown,
    Healthy,
    Unhealthy,
}

impl From<ProviderHealthState> for ProviderHealthStateDto {
    fn from(value: ProviderHealthState) -> Self {
        match value {
            ProviderHealthState::Unknown => Self::Unknown,
            ProviderHealthState::Healthy => Self::Healthy,
            ProviderHealthState::Unhealthy => Self::Unhealthy,
        }
    }
}

/// A configured instance of a provider definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ProviderInstanceDto {
    pub id: String,
    pub provider_definition_id: String,
    pub display_name: String,
    pub enabled: bool,
    pub base_url: String,
    pub timeout_ms: u64,
    pub health_state: ProviderHealthStateDto,
    pub last_health_check_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_latency_ms: Option<u64>,
    pub last_error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<ProviderInstance> for ProviderInstanceDto {
    fn from(value: ProviderInstance) -> Self {
        Self {
            id: value.id().to_owned(),
            provider_definition_id: value.provider_definition_id().to_owned(),
            display_name: value.display_name().to_owned(),
            enabled: value.enabled(),
            base_url: value.base_url().to_owned(),
            timeout_ms: value.timeout_ms(),
            health_state: value.health_state().into(),
            last_health_check_at: value.last_health_check_at(),
            last_success_at: value.last_success_at(),
            last_latency_ms: value.last_latency_ms(),
            last_error_code: value.last_error_code().map(str::to_owned),
            created_at: value.created_at(),
            updated_at: value.updated_at(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CreateProviderInstanceRequest {
    pub provider_definition_id: String,
    pub display_name: String,
    /// Defaults to the definition's public base URL when omitted.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Defaults to the service's probe timeout when omitted.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// Fields left out are unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UpdateProviderInstanceRequest {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub enabled: Option<bool>,
}
