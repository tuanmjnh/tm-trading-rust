use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::AiProviderHealthState;

pub const MAX_AI_MODEL_DISPLAY_NAME_CHARS: usize = 80;
pub const MAX_AI_MODEL_CONTEXT_WINDOW: u32 = 2_000_000;
pub const MAX_AI_MODEL_OUTPUT_TOKENS: u32 = 200_000;

#[derive(Debug, Clone)]
pub struct NewAiModel {
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
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct StoredAiModel {
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
    pub health_state: AiProviderHealthState,
    pub last_health_check_at: Option<DateTime<Utc>>,
    pub last_error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiModel {
    id: String,
    provider_id: String,
    model_id: String,
    display_name: String,
    context_window: Option<u32>,
    max_output_tokens: Option<u32>,
    supports_streaming: bool,
    supports_json_mode: bool,
    supports_tools: bool,
    supports_vision: bool,
    enabled: bool,
    health_state: AiProviderHealthState,
    last_health_check_at: Option<DateTime<Utc>>,
    last_error_code: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl AiModel {
    pub fn new(input: NewAiModel) -> Result<Self, AiModelError> {
        Ok(Self {
            id: required("id", input.id)?,
            provider_id: required("provider id", input.provider_id)?,
            model_id: required("model id", input.model_id)?,
            display_name: normalize_display_name(&input.display_name)?,
            context_window: check_context_window(input.context_window)?,
            max_output_tokens: check_max_output_tokens(input.max_output_tokens)?,
            supports_streaming: input.supports_streaming,
            supports_json_mode: input.supports_json_mode,
            supports_tools: input.supports_tools,
            supports_vision: input.supports_vision,
            enabled: true,
            health_state: AiProviderHealthState::Unknown,
            last_health_check_at: None,
            last_error_code: None,
            created_at: input.created_at,
            updated_at: input.created_at,
        })
    }

    pub fn from_stored(input: StoredAiModel) -> Result<Self, AiModelError> {
        Ok(Self {
            id: required("id", input.id)?,
            provider_id: required("provider id", input.provider_id)?,
            model_id: required("model id", input.model_id)?,
            display_name: normalize_display_name(&input.display_name)?,
            context_window: check_context_window(input.context_window)?,
            max_output_tokens: check_max_output_tokens(input.max_output_tokens)?,
            supports_streaming: input.supports_streaming,
            supports_json_mode: input.supports_json_mode,
            supports_tools: input.supports_tools,
            supports_vision: input.supports_vision,
            enabled: input.enabled,
            health_state: input.health_state,
            last_health_check_at: input.last_health_check_at,
            last_error_code: input.last_error_code,
            created_at: input.created_at,
            updated_at: input.updated_at,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_settings(
        self,
        display_name: &str,
        context_window: Option<u32>,
        max_output_tokens: Option<u32>,
        supports_streaming: bool,
        supports_json_mode: bool,
        supports_tools: bool,
        supports_vision: bool,
        enabled: bool,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, AiModelError> {
        Ok(Self {
            display_name: normalize_display_name(display_name)?,
            context_window: check_context_window(context_window)?,
            max_output_tokens: check_max_output_tokens(max_output_tokens)?,
            supports_streaming,
            supports_json_mode,
            supports_tools,
            supports_vision,
            enabled,
            updated_at,
            ..self
        })
    }

    pub fn with_health(
        self,
        observed_at: DateTime<Utc>,
        healthy: bool,
        error_code: Option<&str>,
    ) -> Self {
        Self {
            health_state: if healthy {
                AiProviderHealthState::Healthy
            } else {
                AiProviderHealthState::Unhealthy
            },
            last_health_check_at: Some(observed_at),
            last_error_code: if healthy {
                None
            } else {
                error_code.map(str::to_owned)
            },
            ..self
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }
    pub fn model_id(&self) -> &str {
        &self.model_id
    }
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    pub fn context_window(&self) -> Option<u32> {
        self.context_window
    }
    pub fn max_output_tokens(&self) -> Option<u32> {
        self.max_output_tokens
    }
    pub fn supports_streaming(&self) -> bool {
        self.supports_streaming
    }
    pub fn supports_json_mode(&self) -> bool {
        self.supports_json_mode
    }
    pub fn supports_tools(&self) -> bool {
        self.supports_tools
    }
    pub fn supports_vision(&self) -> bool {
        self.supports_vision
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn health_state(&self) -> AiProviderHealthState {
        self.health_state
    }
    pub fn last_health_check_at(&self) -> Option<DateTime<Utc>> {
        self.last_health_check_at
    }
    pub fn last_error_code(&self) -> Option<&str> {
        self.last_error_code.as_deref()
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

fn required(field: &'static str, value: String) -> Result<String, AiModelError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AiModelError::EmptyField(field));
    }
    Ok(value.to_owned())
}

fn normalize_display_name(name: &str) -> Result<String, AiModelError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AiModelError::EmptyDisplayName);
    }
    if name.chars().count() > MAX_AI_MODEL_DISPLAY_NAME_CHARS {
        return Err(AiModelError::DisplayNameTooLong);
    }
    Ok(name.to_owned())
}

fn check_context_window(value: Option<u32>) -> Result<Option<u32>, AiModelError> {
    match value {
        Some(0) => Err(AiModelError::ContextWindowOutOfRange),
        Some(value) if value > MAX_AI_MODEL_CONTEXT_WINDOW => {
            Err(AiModelError::ContextWindowOutOfRange)
        }
        _ => Ok(value),
    }
}

fn check_max_output_tokens(value: Option<u32>) -> Result<Option<u32>, AiModelError> {
    match value {
        Some(0) => Err(AiModelError::MaxOutputTokensOutOfRange),
        Some(value) if value > MAX_AI_MODEL_OUTPUT_TOKENS => {
            Err(AiModelError::MaxOutputTokensOutOfRange)
        }
        _ => Ok(value),
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AiModelError {
    #[error("ai model field '{0}' must not be empty")]
    EmptyField(&'static str),
    #[error("ai model display name must not be empty")]
    EmptyDisplayName,
    #[error("ai model display name is too long")]
    DisplayNameTooLong,
    #[error("ai model context window is out of range")]
    ContextWindowOutOfRange,
    #[error("ai model maximum output tokens is out of range")]
    MaxOutputTokensOutOfRange,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 21, 8, 0, 0).unwrap()
    }

    fn new_model() -> NewAiModel {
        NewAiModel {
            id: "primary-large".into(),
            provider_id: "primary".into(),
            model_id: "model-a".into(),
            display_name: "Primary Large".into(),
            context_window: Some(128_000),
            max_output_tokens: Some(8_192),
            supports_streaming: true,
            supports_json_mode: true,
            supports_tools: false,
            supports_vision: false,
            created_at: at(),
        }
    }

    #[test]
    fn creates_a_model_with_unknown_health() {
        let model = AiModel::new(new_model()).unwrap();
        assert_eq!(model.provider_id(), "primary");
        assert_eq!(model.health_state(), AiProviderHealthState::Unknown);
        assert!(model.enabled());
    }

    #[test]
    fn rejects_out_of_range_token_limits() {
        let mut input = new_model();
        input.context_window = Some(0);
        assert_eq!(
            AiModel::new(input).unwrap_err(),
            AiModelError::ContextWindowOutOfRange
        );
        let mut input = new_model();
        input.max_output_tokens = Some(MAX_AI_MODEL_OUTPUT_TOKENS + 1);
        assert_eq!(
            AiModel::new(input).unwrap_err(),
            AiModelError::MaxOutputTokensOutOfRange
        );
    }

    #[test]
    fn settings_update_keeps_the_observed_health() {
        let model = AiModel::new(new_model())
            .unwrap()
            .with_health(at(), false, Some("timeout"));
        let updated = model
            .with_settings(
                "Renamed",
                None,
                None,
                false,
                false,
                false,
                false,
                false,
                at(),
            )
            .unwrap();
        assert_eq!(updated.display_name(), "Renamed");
        assert!(!updated.enabled());
        assert!(updated.context_window().is_none());
        assert_eq!(updated.health_state(), AiProviderHealthState::Unhealthy);
        assert_eq!(updated.last_error_code(), Some("timeout"));
    }
}
