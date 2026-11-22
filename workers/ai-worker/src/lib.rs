//! AI worker core: bounded route execution and evidence-bound briefing publication.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use thiserror::Error;
use tokio::sync::Notify;

use tm_trading_ai_providers::{
    AiGenerationRequest, AiGenerationResponse, AiProvider, AiProviderError,
};
use tm_trading_domain::{AiModel, AiProviderKind, AiProviderProfile, AiTaskKind, AiTaskModelRef};
use tm_trading_storage::{
    AiModelRepository, AiProviderProfileRepository, AiTaskRouteRepository, RepositoryError,
};

mod evidence;
mod job;

pub use evidence::{
    BriefingEvidence, BriefingEvidenceAssembler, BriefingEvidenceRequest, DEFAULT_EVIDENCE_MAX_AGE,
    EvidenceError, ModelBriefing, RESEARCH_EXPLANATION_PROMPT_VERSION, build_briefing,
    parse_model_output, research_explanation_request,
};
pub use job::{AiJobOutcome, AiJobProcessor, BRIEFING_MAX_OUTPUT_TOKENS};

/// Resolves one configured provider/model pair to a callable provider.
///
/// The resolver owns secret lookup so the route executor never receives or logs a secret value.
pub trait AiProviderResolver: Send + Sync {
    fn resolve(
        &self,
        profile: &AiProviderProfile,
        model: &AiModel,
        timeout: Duration,
    ) -> Result<Arc<dyn AiProvider>, AiProviderError>;
}

/// Resolves an OpenAI-compatible provider from an environment-backed secret reference.
#[derive(Debug, Default, Clone, Copy)]
pub struct EnvironmentAiProviderResolver;

impl AiProviderResolver for EnvironmentAiProviderResolver {
    fn resolve(
        &self,
        profile: &AiProviderProfile,
        model: &AiModel,
        timeout: Duration,
    ) -> Result<Arc<dyn AiProvider>, AiProviderError> {
        let api_key =
            std::env::var(profile.api_key_env()).map_err(|_| AiProviderError::MissingApiKey)?;
        if api_key.is_empty() {
            return Err(AiProviderError::MissingApiKey);
        }
        match profile.kind() {
            AiProviderKind::OpenAiCompatible
            | AiProviderKind::OpenAi
            | AiProviderKind::OpenRouter => Ok(Arc::new(
                tm_trading_ai_providers::OpenAiCompatibleProvider::new(
                    profile.base_url(),
                    model.model_id(),
                    &api_key,
                    timeout,
                )?,
            )),
            AiProviderKind::Anthropic | AiProviderKind::Gemini => {
                Err(AiProviderError::UnsupportedKind)
            }
        }
    }
}

/// One provider attempt made while executing a route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiRouteAttempt {
    pub provider_id: String,
    pub model_registry_id: String,
    /// Sanitized provider error code when the attempt failed.
    pub error_code: Option<String>,
}

/// Successful result of one routed AI generation request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiRouteExecution {
    pub provider_id: String,
    pub model_registry_id: String,
    pub model_id: String,
    pub response: AiGenerationResponse,
    pub attempts: Vec<AiRouteAttempt>,
}

/// Failure while resolving or executing one AI route.
#[derive(Debug, Error)]
pub enum AiRouteError {
    #[error("ai task route was not found")]
    RouteNotFound,
    #[error("ai task route is disabled")]
    RouteDisabled,
    #[error("ai task route contains an invalid model reference")]
    InvalidRouteEntry,
    #[error("ai provider profile was not found: {0}")]
    ProviderNotFound(String),
    #[error("ai provider profile is disabled: {0}")]
    ProviderDisabled(String),
    #[error("ai model was not found: {0}")]
    ModelNotFound(String),
    #[error("ai model is disabled: {0}")]
    ModelDisabled(String),
    #[error("ai model does not belong to the named provider: {0}")]
    ModelProviderMismatch(String),
    #[error("ai model does not support JSON mode: {0}")]
    JsonModeUnsupported(String),
    #[error("ai generation request must use JSON mode")]
    JsonModeRequired,
    #[error("ai generation request has no output budget")]
    InvalidOutputBudget,
    #[error("ai task route contains no provider attempt")]
    NoProviderAttempt,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error(transparent)]
    Provider(#[from] AiProviderError),
}

/// Executes one configured task route with bounded timeout, output, attempts, concurrency, and fallback.
#[derive(Clone)]
pub struct AiRouteExecutor {
    routes: Arc<dyn AiTaskRouteRepository>,
    providers: Arc<dyn AiProviderProfileRepository>,
    models: Arc<dyn AiModelRepository>,
    resolver: Arc<dyn AiProviderResolver>,
    limiter: Arc<RouteConcurrencyLimiter>,
}

impl AiRouteExecutor {
    #[must_use]
    pub fn new(
        routes: Arc<dyn AiTaskRouteRepository>,
        providers: Arc<dyn AiProviderProfileRepository>,
        models: Arc<dyn AiModelRepository>,
        resolver: Arc<dyn AiProviderResolver>,
    ) -> Self {
        Self {
            routes,
            providers,
            models,
            resolver,
            limiter: Arc::new(RouteConcurrencyLimiter::default()),
        }
    }

    /// Executes a task through its configured route.
    ///
    /// Fallback is allowed only for a transient provider failure. Invalid configuration,
    /// authentication, permission, input, and response failures stop at the current provider.
    pub async fn execute(
        &self,
        task: AiTaskKind,
        request: &AiGenerationRequest,
    ) -> Result<AiRouteExecution, AiRouteError> {
        if !request.json_mode {
            return Err(AiRouteError::JsonModeRequired);
        }
        if request.max_output_tokens == 0 {
            return Err(AiRouteError::InvalidOutputBudget);
        }

        let route = self
            .routes
            .get(task.as_str())
            .await?
            .ok_or(AiRouteError::RouteNotFound)?;
        if !route.enabled() {
            return Err(AiRouteError::RouteDisabled);
        }
        let _permit = self
            .limiter
            .acquire(task, usize::from(route.concurrency()))
            .await;

        let entries = route
            .chain()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let attempt_limit = route.providers_in_reach().min(entries.len());
        if attempt_limit == 0 {
            return Err(AiRouteError::NoProviderAttempt);
        }

        let mut attempts = Vec::with_capacity(attempt_limit);
        let mut last_error = None;
        for entry in entries.into_iter().take(attempt_limit) {
            let reference =
                AiTaskModelRef::parse(&entry).map_err(|_| AiRouteError::InvalidRouteEntry)?;
            let provider_id = reference.provider_id();
            let model_registry_id = reference.model_registry_id();
            let profile = self
                .providers
                .get(provider_id)
                .await?
                .ok_or_else(|| AiRouteError::ProviderNotFound(provider_id.to_owned()))?;
            if !profile.enabled() {
                return Err(AiRouteError::ProviderDisabled(provider_id.to_owned()));
            }
            let model = self
                .models
                .get(model_registry_id)
                .await?
                .ok_or_else(|| AiRouteError::ModelNotFound(model_registry_id.to_owned()))?;
            if !model.enabled() {
                return Err(AiRouteError::ModelDisabled(model_registry_id.to_owned()));
            }
            if model.provider_id() != profile.id() {
                return Err(AiRouteError::ModelProviderMismatch(
                    model_registry_id.to_owned(),
                ));
            }
            if !model.supports_json_mode() {
                return Err(AiRouteError::JsonModeUnsupported(
                    model_registry_id.to_owned(),
                ));
            }

            let attempt_request = AiGenerationRequest {
                max_output_tokens: effective_output_budget(
                    route.max_output_tokens(),
                    model.max_output_tokens(),
                    request.max_output_tokens,
                ),
                json_mode: true,
                ..request.clone()
            };
            let timeout = Duration::from_millis(route.timeout_ms().min(profile.timeout_ms()));
            let provider = match self.resolver.resolve(&profile, &model, timeout) {
                Ok(provider) => provider,
                Err(error) => {
                    attempts.push(AiRouteAttempt {
                        provider_id: provider_id.to_owned(),
                        model_registry_id: model_registry_id.to_owned(),
                        error_code: Some(error.code().to_owned()),
                    });
                    if !error.is_transient() {
                        return Err(error.into());
                    }
                    last_error = Some(error);
                    continue;
                }
            };

            match tokio::time::timeout(timeout, provider.generate(&attempt_request)).await {
                Ok(Ok(response)) => {
                    attempts.push(AiRouteAttempt {
                        provider_id: provider_id.to_owned(),
                        model_registry_id: model_registry_id.to_owned(),
                        error_code: None,
                    });
                    return Ok(AiRouteExecution {
                        provider_id: provider_id.to_owned(),
                        model_registry_id: model_registry_id.to_owned(),
                        model_id: model.model_id().to_owned(),
                        response,
                        attempts,
                    });
                }
                Ok(Err(error)) => {
                    attempts.push(AiRouteAttempt {
                        provider_id: provider_id.to_owned(),
                        model_registry_id: model_registry_id.to_owned(),
                        error_code: Some(error.code().to_owned()),
                    });
                    if !error.is_transient() {
                        return Err(error.into());
                    }
                    last_error = Some(error);
                }
                Err(_) => {
                    let error = AiProviderError::Timeout;
                    attempts.push(AiRouteAttempt {
                        provider_id: provider_id.to_owned(),
                        model_registry_id: model_registry_id.to_owned(),
                        error_code: Some(error.code().to_owned()),
                    });
                    last_error = Some(error);
                }
            }
        }

        Err(last_error
            .unwrap_or(AiProviderError::Request(
                "no provider attempt was made".to_owned(),
            ))
            .into())
    }
}

fn effective_output_budget(
    route_limit: Option<u32>,
    model_limit: Option<u32>,
    requested_limit: u32,
) -> u32 {
    route_limit
        .map_or(requested_limit, |limit| limit.min(requested_limit))
        .min(model_limit.unwrap_or(u32::MAX))
}

#[derive(Default)]
struct RouteConcurrencyLimiter {
    states: tokio::sync::Mutex<HashMap<String, Arc<ConcurrencyState>>>,
}

struct ConcurrencyState {
    active: AtomicUsize,
    notify: Arc<Notify>,
}

struct ConcurrencyPermit {
    state: Arc<ConcurrencyState>,
}

impl Drop for ConcurrencyPermit {
    fn drop(&mut self) {
        self.state.active.fetch_sub(1, Ordering::SeqCst);
        self.state.notify.notify_waiters();
    }
}

impl RouteConcurrencyLimiter {
    async fn acquire(&self, task: AiTaskKind, limit: usize) -> ConcurrencyPermit {
        let state = {
            let mut states = self.states.lock().await;
            states
                .entry(task.as_str().to_owned())
                .or_insert_with(|| {
                    Arc::new(ConcurrencyState {
                        active: AtomicUsize::new(0),
                        notify: Arc::new(Notify::new()),
                    })
                })
                .clone()
        };
        loop {
            let notify = Arc::clone(&state.notify);
            let notified = notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let active = state.active.load(Ordering::SeqCst);
            if active < limit
                && state
                    .active
                    .compare_exchange(active, active + 1, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                return ConcurrencyPermit { state };
            }
            notified.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::time::Instant;

    use async_trait::async_trait;
    use chrono::{DateTime, TimeZone, Utc};

    use super::*;
    use tm_trading_ai_providers::AiProviderHealth;
    use tm_trading_domain::{AiTaskRoute, NewAiModel, NewAiProviderProfile, NewAiTaskRoute};
    use tm_trading_storage::{
        InMemoryAiModelRepository, InMemoryAiProviderProfileRepository,
        InMemoryAiTaskRouteRepository,
    };

    enum MockOutcome {
        Response(AiGenerationResponse),
        Error(AiProviderError),
        Pending,
    }

    struct ActiveGuard(Arc<AtomicUsize>);

    impl Drop for ActiveGuard {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    struct MockProvider {
        outcomes: Mutex<VecDeque<MockOutcome>>,
        requests: Mutex<Vec<AiGenerationRequest>>,
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
        delay: Duration,
    }

    impl MockProvider {
        fn new(outcomes: Vec<MockOutcome>, delay: Duration) -> Arc<Self> {
            Arc::new(Self {
                outcomes: Mutex::new(outcomes.into()),
                requests: Mutex::new(Vec::new()),
                active: Arc::new(AtomicUsize::new(0)),
                max_active: Arc::new(AtomicUsize::new(0)),
                delay,
            })
        }

        fn requests(&self) -> Vec<AiGenerationRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl AiProvider for MockProvider {
        async fn health(&self) -> Result<AiProviderHealth, AiProviderError> {
            Ok(AiProviderHealth {
                model: "mock".to_owned(),
                latency_ms: 0,
            })
        }

        async fn generate(
            &self,
            request: &AiGenerationRequest,
        ) -> Result<AiGenerationResponse, AiProviderError> {
            self.requests.lock().unwrap().push(request.clone());
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            let _guard = ActiveGuard(Arc::clone(&self.active));
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            let outcome = self.outcomes.lock().unwrap().pop_front();
            match outcome {
                Some(MockOutcome::Response(response)) => Ok(response),
                Some(MockOutcome::Error(error)) => Err(error),
                Some(MockOutcome::Pending) => {
                    std::future::pending::<Result<AiGenerationResponse, AiProviderError>>().await
                }
                None => Ok(response("mock", "primary")),
            }
        }
    }

    struct MockResolver {
        providers: HashMap<String, Arc<dyn AiProvider>>,
        timeouts: Mutex<Vec<(String, Duration)>>,
    }

    impl MockResolver {
        fn new(providers: HashMap<String, Arc<dyn AiProvider>>) -> Arc<Self> {
            Arc::new(Self {
                providers,
                timeouts: Mutex::new(Vec::new()),
            })
        }

        fn timeouts(&self) -> Vec<(String, Duration)> {
            self.timeouts.lock().unwrap().clone()
        }
    }

    impl AiProviderResolver for MockResolver {
        fn resolve(
            &self,
            _profile: &AiProviderProfile,
            model: &AiModel,
            timeout: Duration,
        ) -> Result<Arc<dyn AiProvider>, AiProviderError> {
            self.timeouts
                .lock()
                .unwrap()
                .push((model.id().to_owned(), timeout));
            self.providers
                .get(model.id())
                .cloned()
                .ok_or(AiProviderError::InvalidBaseUrl)
        }
    }

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 22, 8, 0, 0).unwrap()
    }

    fn provider(id: &str) -> AiProviderProfile {
        AiProviderProfile::new(NewAiProviderProfile {
            id: id.to_owned(),
            kind: AiProviderKind::OpenAiCompatible,
            display_name: id.to_owned(),
            base_url: "https://example.com/v1".to_owned(),
            default_model: "model-a".to_owned(),
            timeout_ms: 5_000,
            api_key_env: "TM_TRADING_AI_SECRET_PRIMARY".to_owned(),
            created_at: at(),
        })
        .unwrap()
    }

    fn model(id: &str, provider_id: &str, max_output_tokens: Option<u32>) -> AiModel {
        AiModel::new(NewAiModel {
            id: id.to_owned(),
            provider_id: provider_id.to_owned(),
            model_id: format!("{id}-model"),
            display_name: id.to_owned(),
            context_window: Some(32_000),
            max_output_tokens,
            supports_streaming: false,
            supports_json_mode: true,
            supports_tools: false,
            supports_vision: false,
            created_at: at(),
        })
        .unwrap()
    }

    fn route(
        primary: &str,
        fallbacks: &[&str],
        max_attempts: u8,
        max_output_tokens: Option<u32>,
    ) -> AiTaskRoute {
        AiTaskRoute::new(NewAiTaskRoute {
            task: AiTaskKind::MarketBriefing,
            primary: primary.to_owned(),
            fallbacks: fallbacks.iter().map(|value| (*value).to_owned()).collect(),
            timeout_ms: 5_000,
            max_attempts,
            concurrency: 2,
            max_output_tokens,
            created_at: at(),
        })
        .unwrap()
    }

    fn request(max_output_tokens: u32, json_mode: bool) -> AiGenerationRequest {
        AiGenerationRequest {
            system_prompt: "Return evidence-bound JSON.".to_owned(),
            user_prompt: "Summarize the supplied evidence.".to_owned(),
            max_output_tokens,
            json_mode,
        }
    }

    fn response(model: &str, content: &str) -> AiGenerationResponse {
        AiGenerationResponse {
            content: content.to_owned(),
            model: model.to_owned(),
            prompt_tokens: Some(10),
            completion_tokens: Some(5),
            latency_ms: 1,
        }
    }

    async fn build_executor(
        route: AiTaskRoute,
        providers: Vec<AiProviderProfile>,
        models: Vec<AiModel>,
        resolver: Arc<MockResolver>,
    ) -> AiRouteExecutor {
        let routes = Arc::new(InMemoryAiTaskRouteRepository::new());
        let provider_store = Arc::new(InMemoryAiProviderProfileRepository::new());
        let model_store = Arc::new(InMemoryAiModelRepository::new());
        routes.upsert(&route).await.unwrap();
        for provider in &providers {
            provider_store.upsert(provider).await.unwrap();
        }
        for model in &models {
            model_store.upsert(model).await.unwrap();
        }
        AiRouteExecutor::new(routes, provider_store, model_store, resolver)
    }

    #[tokio::test]
    async fn executes_the_primary_and_records_exact_provenance() {
        let primary = MockProvider::new(
            vec![MockOutcome::Response(response("primary-model", "ok"))],
            Duration::ZERO,
        );
        let resolver = MockResolver::new(HashMap::from([(
            "model-a".to_owned(),
            primary.clone() as Arc<dyn AiProvider>,
        )]));
        let executor = build_executor(
            route("primary:model-a", &[], 1, Some(512)),
            vec![provider("primary")],
            vec![model("model-a", "primary", Some(1_000))],
            resolver,
        )
        .await;

        let result = executor
            .execute(AiTaskKind::MarketBriefing, &request(2_048, true))
            .await
            .unwrap();

        assert_eq!(result.provider_id, "primary");
        assert_eq!(result.model_registry_id, "model-a");
        assert_eq!(result.model_id, "model-a-model");
        assert_eq!(result.response.content, "ok");
        assert_eq!(result.attempts.len(), 1);
        assert_eq!(result.attempts[0].error_code, None);
        assert_eq!(primary.requests()[0].max_output_tokens, 512);
    }

    #[tokio::test]
    async fn rejects_disabled_routes_providers_and_models() {
        let disabled_route = route("primary:model-a", &[], 1, None)
            .with_settings(
                "primary:model-a".to_owned(),
                Vec::new(),
                false,
                5_000,
                1,
                2,
                None,
                at(),
            )
            .unwrap();
        let resolver = MockResolver::new(HashMap::new());
        let executor = build_executor(
            disabled_route,
            vec![provider("primary")],
            vec![model("model-a", "primary", None)],
            resolver.clone(),
        )
        .await;
        assert!(matches!(
            executor
                .execute(AiTaskKind::MarketBriefing, &request(512, true))
                .await,
            Err(AiRouteError::RouteDisabled)
        ));

        let disabled_provider = provider("primary")
            .with_settings(
                "primary",
                "https://example.com/v1",
                "model-a",
                5_000,
                false,
                "TM_TRADING_AI_SECRET_PRIMARY",
                at(),
            )
            .unwrap();
        let executor = build_executor(
            route("primary:model-a", &[], 1, None),
            vec![disabled_provider],
            vec![model("model-a", "primary", None)],
            resolver.clone(),
        )
        .await;
        assert!(matches!(
            executor
                .execute(AiTaskKind::MarketBriefing, &request(512, true))
                .await,
            Err(AiRouteError::ProviderDisabled(_))
        ));

        let disabled_model = model("model-a", "primary", None)
            .with_settings(
                "model-a",
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
        let executor = build_executor(
            route("primary:model-a", &[], 1, None),
            vec![provider("primary")],
            vec![disabled_model],
            resolver,
        )
        .await;
        assert!(matches!(
            executor
                .execute(AiTaskKind::MarketBriefing, &request(512, true))
                .await,
            Err(AiRouteError::ModelDisabled(_))
        ));
    }

    #[tokio::test]
    async fn rejects_model_ownership_mismatch_and_missing_json_support() {
        let resolver = MockResolver::new(HashMap::new());
        let executor = build_executor(
            route("primary:model-a", &[], 1, None),
            vec![provider("primary")],
            vec![model("model-a", "other", None)],
            resolver.clone(),
        )
        .await;
        assert!(matches!(
            executor
                .execute(AiTaskKind::MarketBriefing, &request(512, true))
                .await,
            Err(AiRouteError::ModelProviderMismatch(_))
        ));

        let mut no_json = model("model-a", "primary", None);
        no_json = no_json
            .with_settings(
                "model-a",
                None,
                None,
                false,
                false,
                false,
                false,
                true,
                at(),
            )
            .unwrap();
        let executor = build_executor(
            route("primary:model-a", &[], 1, None),
            vec![provider("primary")],
            vec![no_json],
            resolver,
        )
        .await;
        assert!(matches!(
            executor
                .execute(AiTaskKind::MarketBriefing, &request(512, true))
                .await,
            Err(AiRouteError::JsonModeUnsupported(_))
        ));
    }

    #[tokio::test]
    async fn requires_json_mode_and_caps_the_effective_output_budget() {
        let primary = MockProvider::new(Vec::new(), Duration::ZERO);
        let resolver = MockResolver::new(HashMap::from([(
            "model-a".to_owned(),
            primary.clone() as Arc<dyn AiProvider>,
        )]));
        let executor = build_executor(
            route("primary:model-a", &[], 1, Some(80)),
            vec![provider("primary")],
            vec![model("model-a", "primary", Some(100))],
            resolver,
        )
        .await;

        assert!(matches!(
            executor
                .execute(AiTaskKind::MarketBriefing, &request(2_048, false))
                .await,
            Err(AiRouteError::JsonModeRequired)
        ));
        executor
            .execute(AiTaskKind::MarketBriefing, &request(2_048, true))
            .await
            .unwrap();
        assert_eq!(primary.requests()[0].max_output_tokens, 80);
    }

    #[tokio::test]
    async fn falls_back_only_after_a_transient_failure() {
        let primary = MockProvider::new(
            vec![MockOutcome::Error(AiProviderError::HttpStatus(503))],
            Duration::ZERO,
        );
        let fallback = MockProvider::new(
            vec![MockOutcome::Response(response("fallback-model", "ok"))],
            Duration::ZERO,
        );
        let resolver = MockResolver::new(HashMap::from([
            ("model-a".to_owned(), primary.clone() as Arc<dyn AiProvider>),
            (
                "model-b".to_owned(),
                fallback.clone() as Arc<dyn AiProvider>,
            ),
        ]));
        let executor = build_executor(
            route("primary:model-a", &["fallback:model-b"], 2, None),
            vec![provider("primary"), provider("fallback")],
            vec![
                model("model-a", "primary", None),
                model("model-b", "fallback", None),
            ],
            resolver.clone(),
        )
        .await;

        let result = executor
            .execute(AiTaskKind::MarketBriefing, &request(512, true))
            .await
            .unwrap();

        assert_eq!(result.provider_id, "fallback");
        assert_eq!(result.attempts.len(), 2);
        assert_eq!(
            result.attempts[0].error_code.as_deref(),
            Some("provider_unavailable")
        );
        assert_eq!(primary.requests().len(), 1);
        assert_eq!(fallback.requests().len(), 1);

        let invalid_primary = MockProvider::new(
            vec![MockOutcome::Error(AiProviderError::InvalidResponse)],
            Duration::ZERO,
        );
        let unused_fallback = MockProvider::new(Vec::new(), Duration::ZERO);
        let resolver = MockResolver::new(HashMap::from([
            (
                "model-a".to_owned(),
                invalid_primary.clone() as Arc<dyn AiProvider>,
            ),
            (
                "model-b".to_owned(),
                unused_fallback.clone() as Arc<dyn AiProvider>,
            ),
        ]));
        let executor = build_executor(
            route("primary:model-a", &["fallback:model-b"], 2, None),
            vec![provider("primary"), provider("fallback")],
            vec![
                model("model-a", "primary", None),
                model("model-b", "fallback", None),
            ],
            resolver,
        )
        .await;
        assert!(matches!(
            executor
                .execute(AiTaskKind::MarketBriefing, &request(512, true))
                .await,
            Err(AiRouteError::Provider(AiProviderError::InvalidResponse))
        ));
        assert_eq!(unused_fallback.requests().len(), 0);
    }

    #[tokio::test]
    async fn applies_the_per_attempt_timeout_before_fallback() {
        let primary = MockProvider::new(vec![MockOutcome::Pending], Duration::ZERO);
        let fallback = MockProvider::new(
            vec![MockOutcome::Response(response("fallback-model", "ok"))],
            Duration::ZERO,
        );
        let resolver = MockResolver::new(HashMap::from([
            ("model-a".to_owned(), primary.clone() as Arc<dyn AiProvider>),
            (
                "model-b".to_owned(),
                fallback.clone() as Arc<dyn AiProvider>,
            ),
        ]));
        let route = AiTaskRoute::new(NewAiTaskRoute {
            task: AiTaskKind::MarketBriefing,
            primary: "primary:model-a".to_owned(),
            fallbacks: vec!["fallback:model-b".to_owned()],
            timeout_ms: 500,
            max_attempts: 2,
            concurrency: 2,
            max_output_tokens: None,
            created_at: at(),
        })
        .unwrap();
        let executor = build_executor(
            route,
            vec![provider("primary"), provider("fallback")],
            vec![
                model("model-a", "primary", None),
                model("model-b", "fallback", None),
            ],
            resolver.clone(),
        )
        .await;

        let started = Instant::now();
        let result = executor
            .execute(AiTaskKind::MarketBriefing, &request(512, true))
            .await
            .unwrap();

        assert!(started.elapsed() >= Duration::from_millis(450));
        assert_eq!(result.provider_id, "fallback");
        assert_eq!(result.attempts[0].error_code.as_deref(), Some("timeout"));
        assert_eq!(resolver.timeouts()[0].1, Duration::from_millis(500));
    }

    #[tokio::test]
    async fn stops_at_the_attempt_budget() {
        let primary = MockProvider::new(
            vec![MockOutcome::Error(AiProviderError::Timeout)],
            Duration::ZERO,
        );
        let fallback = MockProvider::new(Vec::new(), Duration::ZERO);
        let resolver = MockResolver::new(HashMap::from([
            ("model-a".to_owned(), primary.clone() as Arc<dyn AiProvider>),
            (
                "model-b".to_owned(),
                fallback.clone() as Arc<dyn AiProvider>,
            ),
        ]));
        let executor = build_executor(
            route("primary:model-a", &["fallback:model-b"], 1, None),
            vec![provider("primary"), provider("fallback")],
            vec![
                model("model-a", "primary", None),
                model("model-b", "fallback", None),
            ],
            resolver,
        )
        .await;

        assert!(matches!(
            executor
                .execute(AiTaskKind::MarketBriefing, &request(512, true))
                .await,
            Err(AiRouteError::Provider(AiProviderError::Timeout))
        ));
        assert_eq!(primary.requests().len(), 1);
        assert_eq!(fallback.requests().len(), 0);
    }

    #[tokio::test]
    async fn enforces_the_route_concurrency_limit() {
        let primary = MockProvider::new(Vec::new(), Duration::from_millis(40));
        let resolver = MockResolver::new(HashMap::from([(
            "model-a".to_owned(),
            primary.clone() as Arc<dyn AiProvider>,
        )]));
        let mut route = route("primary:model-a", &[], 1, None);
        route = route
            .with_settings(
                "primary:model-a".to_owned(),
                Vec::new(),
                true,
                5_000,
                1,
                1,
                None,
                at(),
            )
            .unwrap();
        let executor = Arc::new(
            build_executor(
                route,
                vec![provider("primary")],
                vec![model("model-a", "primary", None)],
                resolver,
            )
            .await,
        );

        let first = {
            let executor = Arc::clone(&executor);
            tokio::spawn(async move {
                executor
                    .execute(AiTaskKind::MarketBriefing, &request(512, true))
                    .await
            })
        };
        let second = {
            let executor = Arc::clone(&executor);
            tokio::spawn(async move {
                executor
                    .execute(AiTaskKind::MarketBriefing, &request(512, true))
                    .await
            })
        };
        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();

        assert_eq!(primary.max_active.load(Ordering::SeqCst), 1);
    }
}
