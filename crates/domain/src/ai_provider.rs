use chrono::{DateTime, Utc};
use thiserror::Error;

pub const MAX_AI_PROVIDER_DISPLAY_NAME_CHARS: usize = 80;
pub const MIN_AI_PROVIDER_TIMEOUT_MS: u64 = 500;
pub const MAX_AI_PROVIDER_TIMEOUT_MS: u64 = 60_000;
/// Only environment variables in this namespace may hold AI provider credentials.
pub const AI_PROVIDER_API_KEY_ENV_PREFIX: &str = "TM_TRADING_AI_";
/// Environment name used by profiles stored without an explicit secret reference.
pub const DEFAULT_AI_PROVIDER_API_KEY_ENV: &str = "TM_TRADING_AI_OPENAI_API_KEY";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiProviderKind {
    OpenAiCompatible,
    OpenAi,
    Anthropic,
    Gemini,
    OpenRouter,
}

impl AiProviderKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai-compatible",
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::OpenRouter => "openrouter",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "openai-compatible" => Some(Self::OpenAiCompatible),
            "openai" => Some(Self::OpenAi),
            "anthropic" => Some(Self::Anthropic),
            "gemini" => Some(Self::Gemini),
            "openrouter" => Some(Self::OpenRouter),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiProviderHealthState {
    Unknown,
    Healthy,
    Unhealthy,
}

impl AiProviderHealthState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::Healthy => "HEALTHY",
            Self::Unhealthy => "UNHEALTHY",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "UNKNOWN" => Some(Self::Unknown),
            "HEALTHY" => Some(Self::Healthy),
            "UNHEALTHY" => Some(Self::Unhealthy),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewAiProviderProfile {
    pub id: String,
    pub kind: AiProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub default_model: String,
    pub timeout_ms: u64,
    pub api_key_env: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct StoredAiProviderProfile {
    pub id: String,
    pub kind: AiProviderKind,
    pub display_name: String,
    pub enabled: bool,
    pub base_url: String,
    pub default_model: String,
    pub timeout_ms: u64,
    pub api_key_env: String,
    pub health_state: AiProviderHealthState,
    pub last_health_check_at: Option<DateTime<Utc>>,
    pub last_error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiProviderProfile {
    id: String,
    kind: AiProviderKind,
    display_name: String,
    enabled: bool,
    base_url: String,
    default_model: String,
    timeout_ms: u64,
    api_key_env: String,
    health_state: AiProviderHealthState,
    last_health_check_at: Option<DateTime<Utc>>,
    last_error_code: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl AiProviderProfile {
    pub fn new(input: NewAiProviderProfile) -> Result<Self, AiProviderProfileError> {
        Ok(Self {
            id: required("id", input.id)?,
            kind: input.kind,
            display_name: normalize_display_name(&input.display_name)?,
            enabled: true,
            base_url: normalize_base_url(&input.base_url)?,
            default_model: required("default model", input.default_model)?,
            timeout_ms: check_timeout(input.timeout_ms)?,
            api_key_env: normalize_env_name(&input.api_key_env)?,
            health_state: AiProviderHealthState::Unknown,
            last_health_check_at: None,
            last_error_code: None,
            created_at: input.created_at,
            updated_at: input.created_at,
        })
    }
    pub fn from_stored(input: StoredAiProviderProfile) -> Result<Self, AiProviderProfileError> {
        Ok(Self {
            id: required("id", input.id)?,
            kind: input.kind,
            display_name: normalize_display_name(&input.display_name)?,
            enabled: input.enabled,
            base_url: normalize_base_url(&input.base_url)?,
            default_model: required("default model", input.default_model)?,
            timeout_ms: check_timeout(input.timeout_ms)?,
            api_key_env: normalize_env_name(&input.api_key_env)?,
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
        base_url: &str,
        default_model: &str,
        timeout_ms: u64,
        enabled: bool,
        api_key_env: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, AiProviderProfileError> {
        Ok(Self {
            display_name: normalize_display_name(display_name)?,
            base_url: normalize_base_url(base_url)?,
            default_model: required("default model", default_model.to_owned())?,
            timeout_ms: check_timeout(timeout_ms)?,
            enabled,
            api_key_env: normalize_env_name(api_key_env)?,
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
    pub fn kind(&self) -> AiProviderKind {
        self.kind
    }
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
    pub fn default_model(&self) -> &str {
        &self.default_model
    }
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }
    pub fn api_key_env(&self) -> &str {
        &self.api_key_env
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

fn required(field: &'static str, value: String) -> Result<String, AiProviderProfileError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AiProviderProfileError::EmptyField(field));
    }
    Ok(value.to_owned())
}

fn normalize_display_name(name: &str) -> Result<String, AiProviderProfileError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AiProviderProfileError::EmptyDisplayName);
    }
    if name.chars().count() > MAX_AI_PROVIDER_DISPLAY_NAME_CHARS {
        return Err(AiProviderProfileError::DisplayNameTooLong);
    }
    Ok(name.to_owned())
}

fn normalize_base_url(value: &str) -> Result<String, AiProviderProfileError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AiProviderProfileError::EmptyBaseUrl);
    }
    let url = url::Url::parse(value).map_err(|_| AiProviderProfileError::InvalidBaseUrl)?;
    if url.scheme() != "https" && url.scheme() != "http" {
        return Err(AiProviderProfileError::InvalidBaseUrl);
    }
    Ok(value.trim_end_matches('/').to_owned())
}

fn check_timeout(value: u64) -> Result<u64, AiProviderProfileError> {
    if !(MIN_AI_PROVIDER_TIMEOUT_MS..=MAX_AI_PROVIDER_TIMEOUT_MS).contains(&value) {
        return Err(AiProviderProfileError::TimeoutOutOfRange);
    }
    Ok(value)
}

fn normalize_env_name(value: &str) -> Result<String, AiProviderProfileError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AiProviderProfileError::EmptyApiKeyEnv);
    }
    let Some(suffix) = value.strip_prefix(AI_PROVIDER_API_KEY_ENV_PREFIX) else {
        return Err(AiProviderProfileError::InvalidApiKeyEnv);
    };
    if suffix.is_empty()
        || !suffix
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(AiProviderProfileError::InvalidApiKeyEnv);
    }
    Ok(value.to_owned())
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AiProviderProfileError {
    #[error("ai provider field '{0}' must not be empty")]
    EmptyField(&'static str),
    #[error("ai provider display name must not be empty")]
    EmptyDisplayName,
    #[error("ai provider display name is too long")]
    DisplayNameTooLong,
    #[error("ai provider base URL must not be empty")]
    EmptyBaseUrl,
    #[error("ai provider base URL is invalid")]
    InvalidBaseUrl,
    #[error("ai provider timeout is out of range")]
    TimeoutOutOfRange,
    #[error("ai provider API key environment variable name must not be empty")]
    EmptyApiKeyEnv,
    #[error(
        "ai provider API key environment variable must use the TM_TRADING_AI_ uppercase namespace"
    )]
    InvalidApiKeyEnv,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, 8, 0, 0).unwrap()
    }
    fn new_profile(api_key_env: &str) -> Result<AiProviderProfile, AiProviderProfileError> {
        AiProviderProfile::new(NewAiProviderProfile {
            id: "p1".into(),
            kind: AiProviderKind::OpenAiCompatible,
            display_name: "Local".into(),
            base_url: "http://localhost:11434/v1".into(),
            default_model: "llama3".into(),
            timeout_ms: 5000,
            api_key_env: api_key_env.into(),
            created_at: at(),
        })
    }

    #[test]
    fn creates_profile() {
        let p = new_profile("TM_TRADING_AI_OPENAI_API_KEY").unwrap();
        assert_eq!(p.id(), "p1");
    }

    #[test]
    fn rejects_api_key_environment_names_outside_the_ai_namespace() {
        for name in [
            "PATH",
            "OPENAI_API_KEY",
            "TM_TEST_AI_KEY",
            "TM_TRADING_AI_",
            "TM_TRADING_AI_lowercase",
            "TM_TRADING_AI_BAD-NAME",
        ] {
            assert!(
                matches!(
                    new_profile(name),
                    Err(AiProviderProfileError::InvalidApiKeyEnv)
                ),
                "{name} must be rejected"
            );
        }
    }

    #[test]
    fn stored_profiles_cannot_bypass_the_api_key_namespace() {
        let result = AiProviderProfile::from_stored(StoredAiProviderProfile {
            id: "p1".into(),
            kind: AiProviderKind::OpenAiCompatible,
            display_name: "Local".into(),
            enabled: true,
            base_url: "http://localhost:11434/v1".into(),
            default_model: "llama3".into(),
            timeout_ms: 5000,
            api_key_env: "PATH".into(),
            health_state: AiProviderHealthState::Unknown,
            last_health_check_at: None,
            last_error_code: None,
            created_at: at(),
            updated_at: at(),
        });
        assert!(matches!(
            result,
            Err(AiProviderProfileError::InvalidApiKeyEnv)
        ));
    }
}
