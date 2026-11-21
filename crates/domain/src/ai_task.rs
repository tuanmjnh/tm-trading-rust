use chrono::{DateTime, Utc};
use thiserror::Error;

/// AI tasks the platform routes to a configured provider/model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AiTaskKind {
    #[default]
    NewsClassification,
    NewsSummary,
    MarketBriefing,
    ResearchExplanation,
    DailyDigest,
}

impl AiTaskKind {
    pub const ALL: [Self; 5] = [
        Self::NewsClassification,
        Self::NewsSummary,
        Self::MarketBriefing,
        Self::ResearchExplanation,
        Self::DailyDigest,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NewsClassification => "news_classification",
            Self::NewsSummary => "news_summary",
            Self::MarketBriefing => "market_briefing",
            Self::ResearchExplanation => "research_explanation",
            Self::DailyDigest => "daily_digest",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "news_classification" => Some(Self::NewsClassification),
            "news_summary" => Some(Self::NewsSummary),
            "market_briefing" => Some(Self::MarketBriefing),
            "research_explanation" => Some(Self::ResearchExplanation),
            "daily_digest" => Some(Self::DailyDigest),
            _ => None,
        }
    }
}

/// Fallback presets. A route is a closed list, so the number of provider attempts is
/// bounded by the largest preset instead of growing with configured providers.
pub const MIN_AI_TASK_MAX_ATTEMPTS: u8 = 1;
pub const MAX_AI_TASK_MAX_ATTEMPTS: u8 = 3;

/// Concurrency is bounded so a route cannot fan out without limit.
pub const MIN_AI_TASK_CONCURRENCY: u8 = 1;
pub const MAX_AI_TASK_CONCURRENCY: u8 = 4;

/// Output budget is capped independently of the model's declared `max_output_tokens`.
pub const MIN_AI_TASK_OUTPUT_TOKENS: u32 = 1;
pub const MAX_AI_TASK_OUTPUT_TOKENS: u32 = 200_000;

/// A task model entry as written by an operator: `provider_id:model_registry_id`.
///
/// The two halves only have to be non-empty here. Whether the pair exists in the
/// model registry is decided by the API layer, which owns the repositories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiTaskModelRef {
    provider_id: String,
    model_registry_id: String,
}

impl AiTaskModelRef {
    pub fn parse(value: &str) -> Result<Self, AiTaskRouteError> {
        let Some((provider_id, model_registry_id)) = value.trim().split_once(':') else {
            return Err(AiTaskRouteError::InvalidModelRef);
        };
        let provider_id = provider_id.trim();
        let model_registry_id = model_registry_id.trim();
        if provider_id.is_empty() || model_registry_id.is_empty() {
            return Err(AiTaskRouteError::EmptyProviderId);
        }
        Ok(Self {
            provider_id: provider_id.to_owned(),
            model_registry_id: model_registry_id.to_owned(),
        })
    }

    #[must_use]
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    #[must_use]
    pub fn model_registry_id(&self) -> &str {
        &self.model_registry_id
    }

    #[must_use]
    pub fn as_string(&self) -> String {
        format!("{}:{}", self.provider_id, self.model_registry_id)
    }
}

/// A configured route for one AI task: a primary model, an optional fallback chain,
/// and the budget that bounds a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiTaskRoute {
    task: AiTaskKind,
    primary: String,
    fallbacks: Vec<String>,
    enabled: bool,
    timeout_ms: u64,
    max_attempts: u8,
    concurrency: u8,
    max_output_tokens: Option<u32>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewAiTaskRoute {
    pub task: AiTaskKind,
    pub primary: String,
    pub fallbacks: Vec<String>,
    pub timeout_ms: u64,
    pub max_attempts: u8,
    pub concurrency: u8,
    pub max_output_tokens: Option<u32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct StoredAiTaskRoute {
    pub task: AiTaskKind,
    pub primary: String,
    pub fallbacks: Vec<String>,
    pub enabled: bool,
    pub timeout_ms: u64,
    pub max_attempts: u8,
    pub concurrency: u8,
    pub max_output_tokens: Option<u32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AiTaskRoute {
    pub fn new(input: NewAiTaskRoute) -> Result<Self, AiTaskRouteError> {
        let (primary, fallbacks) = normalize_chain(input.primary, input.fallbacks)?;
        Ok(Self {
            task: input.task,
            primary,
            fallbacks,
            enabled: true,
            timeout_ms: check_timeout(input.timeout_ms)?,
            max_attempts: check_attempts(input.max_attempts)?,
            concurrency: check_concurrency(input.concurrency)?,
            max_output_tokens: check_output_tokens(input.max_output_tokens)?,
            created_at: input.created_at,
            updated_at: input.created_at,
        })
    }

    pub fn from_stored(input: StoredAiTaskRoute) -> Result<Self, AiTaskRouteError> {
        let (primary, fallbacks) = normalize_chain(input.primary, input.fallbacks)?;
        Ok(Self {
            task: input.task,
            primary,
            fallbacks,
            enabled: input.enabled,
            timeout_ms: check_timeout(input.timeout_ms)?,
            max_attempts: check_attempts(input.max_attempts)?,
            concurrency: check_concurrency(input.concurrency)?,
            max_output_tokens: check_output_tokens(input.max_output_tokens)?,
            created_at: input.created_at,
            updated_at: input.updated_at,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_settings(
        self,
        primary: String,
        fallbacks: Vec<String>,
        enabled: bool,
        timeout_ms: u64,
        max_attempts: u8,
        concurrency: u8,
        max_output_tokens: Option<u32>,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, AiTaskRouteError> {
        let (primary, fallbacks) = normalize_chain(primary, fallbacks)?;
        Ok(Self {
            primary,
            fallbacks,
            enabled,
            timeout_ms: check_timeout(timeout_ms)?,
            max_attempts: check_attempts(max_attempts)?,
            concurrency: check_concurrency(concurrency)?,
            max_output_tokens: check_output_tokens(max_output_tokens)?,
            updated_at,
            ..self
        })
    }

    /// The whole chain in the order a request may be tried: primary, then fallbacks.
    #[must_use]
    pub fn chain(&self) -> Vec<&str> {
        std::iter::once(self.primary.as_str())
            .chain(self.fallbacks.iter().map(String::as_str))
            .collect()
    }

    /// How many providers a request may actually try; the attempt budget bounds this.
    #[must_use]
    pub fn providers_in_reach(&self) -> usize {
        if self.enabled {
            self.chain().len().min(usize::from(self.max_attempts))
        } else {
            0
        }
    }

    #[must_use]
    pub fn task(&self) -> AiTaskKind {
        self.task
    }
    #[must_use]
    pub fn primary(&self) -> &str {
        &self.primary
    }
    #[must_use]
    pub fn fallbacks(&self) -> &[String] {
        &self.fallbacks
    }
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    #[must_use]
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }
    #[must_use]
    pub fn max_attempts(&self) -> u8 {
        self.max_attempts
    }
    #[must_use]
    pub fn concurrency(&self) -> u8 {
        self.concurrency
    }
    #[must_use]
    pub fn max_output_tokens(&self) -> Option<u32> {
        self.max_output_tokens
    }
    #[must_use]
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    #[must_use]
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

/// Rejects a chain that repeats a provider, so one failing provider is never tried twice.
fn normalize_chain(
    primary: String,
    fallbacks: Vec<String>,
) -> Result<(String, Vec<String>), AiTaskRouteError> {
    let primary = normalize_entry(primary)?;
    if fallbacks.len() + 1 > usize::from(MAX_AI_TASK_MAX_ATTEMPTS) {
        return Err(AiTaskRouteError::TooManyProviders(fallbacks.len() + 1));
    }
    let mut seen: Vec<String> = vec![primary.clone()];
    let mut normalized = Vec::with_capacity(fallbacks.len());
    for fallback in fallbacks {
        let fallback = normalize_entry(fallback)?;
        if seen.iter().any(|existing| existing == &fallback) {
            return Err(AiTaskRouteError::DuplicateProvider(fallback));
        }
        seen.push(fallback.clone());
        normalized.push(fallback);
    }
    Ok((primary, normalized))
}

fn normalize_entry(value: String) -> Result<String, AiTaskRouteError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AiTaskRouteError::EmptyProviderId);
    }
    Ok(value.to_owned())
}

fn check_timeout(value: u64) -> Result<u64, AiTaskRouteError> {
    if !(crate::MIN_AI_PROVIDER_TIMEOUT_MS..=crate::MAX_AI_PROVIDER_TIMEOUT_MS).contains(&value) {
        return Err(AiTaskRouteError::TimeoutOutOfRange);
    }
    Ok(value)
}

fn check_attempts(value: u8) -> Result<u8, AiTaskRouteError> {
    if !(MIN_AI_TASK_MAX_ATTEMPTS..=MAX_AI_TASK_MAX_ATTEMPTS).contains(&value) {
        return Err(AiTaskRouteError::AttemptsOutOfRange);
    }
    Ok(value)
}

fn check_concurrency(value: u8) -> Result<u8, AiTaskRouteError> {
    if !(MIN_AI_TASK_CONCURRENCY..=MAX_AI_TASK_CONCURRENCY).contains(&value) {
        return Err(AiTaskRouteError::ConcurrencyOutOfRange);
    }
    Ok(value)
}

fn check_output_tokens(value: Option<u32>) -> Result<Option<u32>, AiTaskRouteError> {
    match value {
        Some(0) => Err(AiTaskRouteError::OutputTokensOutOfRange),
        Some(value) if value > MAX_AI_TASK_OUTPUT_TOKENS => {
            Err(AiTaskRouteError::OutputTokensOutOfRange)
        }
        _ => Ok(value),
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AiTaskRouteError {
    #[error("ai task route model must be written as provider_id:model_id")]
    InvalidModelRef,
    #[error("ai task route provider entry must not be empty")]
    EmptyProviderId,
    #[error("ai task route names provider '{0}' more than once")]
    DuplicateProvider(String),
    #[error("ai task route names {0} providers but at most {MAX_AI_TASK_MAX_ATTEMPTS} are allowed")]
    TooManyProviders(usize),
    #[error("ai task route attempts are out of range")]
    AttemptsOutOfRange,
    #[error("ai task route concurrency is out of range")]
    ConcurrencyOutOfRange,
    #[error("ai task route output token limit is out of range")]
    OutputTokensOutOfRange,
    #[error("ai task route timeout is out of range")]
    TimeoutOutOfRange,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 21, 8, 0, 0).unwrap()
    }

    fn new_route(primary: &str, fallbacks: &[&str], max_attempts: u8) -> NewAiTaskRoute {
        NewAiTaskRoute {
            task: AiTaskKind::MarketBriefing,
            primary: primary.to_owned(),
            fallbacks: fallbacks.iter().map(|value| (*value).to_owned()).collect(),
            timeout_ms: 5_000,
            max_attempts,
            concurrency: 2,
            max_output_tokens: Some(1_024),
            created_at: at(),
        }
    }

    #[test]
    fn task_kinds_round_trip() {
        for kind in AiTaskKind::ALL {
            assert_eq!(AiTaskKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(AiTaskKind::parse("unknown"), None);
    }

    #[test]
    fn model_refs_parse_the_provider_and_model_halves() {
        let reference = AiTaskModelRef::parse(" primary : model-a ").unwrap();
        assert_eq!(reference.provider_id(), "primary");
        assert_eq!(reference.model_registry_id(), "model-a");
        assert_eq!(reference.as_string(), "primary:model-a");
    }

    #[test]
    fn model_refs_reject_missing_halves() {
        assert_eq!(
            AiTaskModelRef::parse("primary").unwrap_err(),
            AiTaskRouteError::InvalidModelRef
        );
        assert_eq!(
            AiTaskModelRef::parse("primary:").unwrap_err(),
            AiTaskRouteError::EmptyProviderId
        );
    }

    #[test]
    fn a_route_only_reaches_its_attempt_budget() {
        let route = AiTaskRoute::new(new_route("primary", &["backup", "tertiary"], 2)).unwrap();
        assert_eq!(route.chain(), vec!["primary", "backup", "tertiary"]);
        assert_eq!(route.providers_in_reach(), 2);
        assert!(route.enabled());
        assert_eq!(route.task(), AiTaskKind::MarketBriefing);
    }

    #[test]
    fn a_route_rejects_a_repeated_provider() {
        assert_eq!(
            AiTaskRoute::new(new_route("primary", &["backup", "primary"], 2)).unwrap_err(),
            AiTaskRouteError::DuplicateProvider("primary".to_owned())
        );
        assert_eq!(
            AiTaskRoute::new(new_route("primary", &["primary"], 2)).unwrap_err(),
            AiTaskRouteError::DuplicateProvider("primary".to_owned())
        );
    }

    #[test]
    fn a_route_rejects_a_chain_longer_than_the_attempt_budget() {
        assert_eq!(
            AiTaskRoute::new(new_route("a", &["b", "c", "d"], 3)).unwrap_err(),
            AiTaskRouteError::TooManyProviders(4)
        );
        assert_eq!(
            AiTaskRoute::new(new_route("a", &[], 0)).unwrap_err(),
            AiTaskRouteError::AttemptsOutOfRange
        );
    }

    #[test]
    fn a_route_rejects_blank_and_out_of_range_settings() {
        assert_eq!(
            AiTaskRoute::new(new_route("  ", &[], 1)).unwrap_err(),
            AiTaskRouteError::EmptyProviderId
        );
        assert_eq!(
            AiTaskRoute::new(NewAiTaskRoute {
                timeout_ms: 100,
                ..new_route("primary", &[], 1)
            })
            .unwrap_err(),
            AiTaskRouteError::TimeoutOutOfRange
        );
        assert_eq!(
            AiTaskRoute::new(NewAiTaskRoute {
                concurrency: 0,
                ..new_route("primary", &[], 1)
            })
            .unwrap_err(),
            AiTaskRouteError::ConcurrencyOutOfRange
        );
        assert_eq!(
            AiTaskRoute::new(NewAiTaskRoute {
                max_output_tokens: Some(0),
                ..new_route("primary", &[], 1)
            })
            .unwrap_err(),
            AiTaskRouteError::OutputTokensOutOfRange
        );
    }

    #[test]
    fn disabling_a_route_removes_every_reachable_provider() {
        let route = AiTaskRoute::new(new_route("primary", &["backup"], 2))
            .unwrap()
            .with_settings(
                "primary".to_owned(),
                vec!["backup".to_owned()],
                false,
                5_000,
                2,
                2,
                None,
                at(),
            )
            .unwrap();
        assert!(!route.enabled());
        assert_eq!(route.providers_in_reach(), 0);
    }
}
