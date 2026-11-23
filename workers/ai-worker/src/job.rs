//! Claims one durable AI job, generates a briefing from owner-scoped evidence, and publishes it.
//!
//! Briefing publication and job completion are one durable transition. A lease that
//! lapses before completion leaves the job unknown instead of reporting an unobserved success.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};

use tm_trading_domain::{AiBriefing, AiJob, AiJobId, AiJobStatus, AiTaskKind};
use tm_trading_storage::{AiJobRepository, RepositoryError};

use crate::evidence::{
    BriefingEvidenceAssembler, BriefingEvidenceRequest, EvidenceError,
    RESEARCH_EXPLANATION_PROMPT_VERSION, build_briefing, parse_model_output,
    research_explanation_request,
};
use crate::{AiRouteError, AiRouteExecutor};

/// Output budget requested from the configured route for one briefing.
pub const BRIEFING_MAX_OUTPUT_TOKENS: u32 = 1_024;

trait AiJobClock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

struct SystemAiJobClock;

impl AiJobClock for SystemAiJobClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Result of one claim attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiJobOutcome {
    /// No queued job was available.
    Idle,
    Published {
        job_id: AiJobId,
        briefing_id: String,
    },
    Failed {
        job_id: AiJobId,
        error_code: &'static str,
    },
    /// The lease expired before the result could be observed.
    OutcomeUnknown { job_id: AiJobId },
}

/// Claims and processes durable AI jobs against one provider route.
#[derive(Clone)]
pub struct AiJobProcessor {
    jobs: Arc<dyn AiJobRepository>,
    assembler: BriefingEvidenceAssembler,
    executor: AiRouteExecutor,
    evidence_max_age: Duration,
    clock: Arc<dyn AiJobClock>,
}

impl AiJobProcessor {
    #[must_use]
    pub fn new(
        jobs: Arc<dyn AiJobRepository>,
        assembler: BriefingEvidenceAssembler,
        executor: AiRouteExecutor,
        evidence_max_age: Duration,
    ) -> Self {
        Self::new_with_clock(
            jobs,
            assembler,
            executor,
            evidence_max_age,
            Arc::new(SystemAiJobClock),
        )
    }

    fn new_with_clock(
        jobs: Arc<dyn AiJobRepository>,
        assembler: BriefingEvidenceAssembler,
        executor: AiRouteExecutor,
        evidence_max_age: Duration,
        clock: Arc<dyn AiJobClock>,
    ) -> Self {
        Self {
            jobs,
            assembler,
            executor,
            evidence_max_age,
            clock,
        }
    }

    /// Claims at most one job and processes it.
    ///
    /// Every failure is recorded on the job before it is returned, so a caller never
    /// has to guess whether the provider was called.
    pub async fn process_next(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
        lease_duration: Duration,
    ) -> Result<AiJobOutcome, RepositoryError> {
        let Some(job) = self.jobs.claim_next(worker_id, now, lease_duration).await? else {
            return Ok(AiJobOutcome::Idle);
        };
        let job_id = job.id();
        let fencing_token = job.fencing_token();

        let prepared = self.prepare(&job, now).await;
        let transition_at = self.clock.now();
        let briefing = match prepared {
            Ok(briefing) => briefing,
            Err(error_code) => {
                let failed = self
                    .jobs
                    .fail(
                        &job_id,
                        worker_id,
                        fencing_token,
                        error_code.to_owned(),
                        None,
                        transition_at,
                    )
                    .await?;
                return if failed.status() == AiJobStatus::OutcomeUnknown {
                    Ok(AiJobOutcome::OutcomeUnknown { job_id })
                } else {
                    Ok(AiJobOutcome::Failed { job_id, error_code })
                };
            }
        };

        let completed = match self
            .jobs
            .complete_with_briefing(&job_id, worker_id, fencing_token, &briefing, transition_at)
            .await
        {
            Ok(completed) => completed,
            Err(RepositoryError::AiBriefingConflict | RepositoryError::AiBriefingMismatch) => {
                let failed_at = self.clock.now();
                let failed = self
                    .jobs
                    .fail(
                        &job_id,
                        worker_id,
                        fencing_token,
                        "publish_failed".to_owned(),
                        None,
                        failed_at,
                    )
                    .await?;
                return if failed.status() == AiJobStatus::OutcomeUnknown {
                    Ok(AiJobOutcome::OutcomeUnknown { job_id })
                } else {
                    Ok(AiJobOutcome::Failed {
                        job_id,
                        error_code: "publish_failed",
                    })
                };
            }
            Err(error) => return Err(error),
        };
        match completed.status() {
            AiJobStatus::Succeeded => Ok(AiJobOutcome::Published {
                job_id,
                briefing_id: briefing.id().to_owned(),
            }),
            AiJobStatus::OutcomeUnknown => Ok(AiJobOutcome::OutcomeUnknown { job_id }),
            _ => Err(RepositoryError::ConcurrentModification),
        }
    }

    /// Runs evidence assembly, generation, and validation for a claimed job.
    async fn prepare(&self, job: &AiJob, now: DateTime<Utc>) -> Result<AiBriefing, &'static str> {
        if job.task() != AiTaskKind::ResearchExplanation {
            return Err("unsupported_task");
        }
        let evidence = self
            .assembler
            .assemble(
                &BriefingEvidenceRequest {
                    owner_scope: job.owner_scope().to_owned(),
                    research_event_ids: job.research_event_ids().to_vec(),
                    news_item_ids: job.news_item_ids().to_vec(),
                },
                now,
                self.evidence_max_age,
            )
            .await
            .map_err(evidence_code)?;
        let request = research_explanation_request(&evidence, BRIEFING_MAX_OUTPUT_TOKENS)
            .map_err(evidence_code)?;
        let execution = self
            .executor
            .execute(AiTaskKind::ResearchExplanation, &request)
            .await
            .map_err(route_code)?;
        let output = parse_model_output(&execution.response.content).map_err(evidence_code)?;
        build_briefing(
            job.owner_scope(),
            AiTaskKind::ResearchExplanation,
            RESEARCH_EXPLANATION_PROMPT_VERSION,
            &execution,
            &evidence,
            &output,
            self.clock.now(),
        )
        .map_err(|_| "invalid_briefing")
    }
}

fn evidence_code(error: EvidenceError) -> &'static str {
    match error {
        EvidenceError::NoUsableEvidence => "no_usable_evidence",
        EvidenceError::EmptyOwnerScope | EvidenceError::InvalidInputId(_) => "invalid_request",
        EvidenceError::TooManyInputs
        | EvidenceError::PromptTooLong
        | EvidenceError::FactTooLong(_) => "evidence_too_large",
        EvidenceError::InvalidModelOutput
        | EvidenceError::UnknownModelField(_)
        | EvidenceError::EmptyModelOutput => "invalid_model_output",
        _ => "evidence_unavailable",
    }
}

fn route_code(error: AiRouteError) -> &'static str {
    match error {
        AiRouteError::Provider(_) => "provider_failed",
        AiRouteError::Repository(_) => "evidence_unavailable",
        _ => "route_unavailable",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use chrono::TimeZone;

    use super::*;
    use tm_trading_ai_providers::{
        AiGenerationResponse, AiProvider, AiProviderError, AiProviderHealth,
    };
    use tm_trading_domain::{
        AiModel, AiProviderKind, AiProviderProfile, AiTaskRoute, NewAiJob, NewAiModel,
        NewAiProviderProfile, NewAiTaskRoute, NewResearchEvent, ResearchEvent, ResearchQuality,
        input_snapshot_id,
    };
    use tm_trading_storage::{
        AiBriefingQuery, AiModelRepository, AiProviderProfileRepository, AiTaskRouteRepository,
        InMemoryAiJobRepository, InMemoryAiModelRepository, InMemoryAiProviderProfileRepository,
        InMemoryAiTaskRouteRepository, InMemoryResearchEventRepository, ResearchEventRepository,
    };

    use crate::AiProviderResolver;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 22, 8, 0, 0).unwrap()
    }

    struct TestClock {
        now: Mutex<DateTime<Utc>>,
    }

    impl TestClock {
        fn new() -> Self {
            Self {
                now: Mutex::new(at()),
            }
        }

        fn advance(&self, duration: Duration) {
            let mut now = self.now.lock().unwrap();
            *now += chrono::Duration::from_std(duration).unwrap();
        }
    }

    impl AiJobClock for TestClock {
        fn now(&self) -> DateTime<Utc> {
            *self.now.lock().unwrap()
        }
    }

    fn research_event(owner_scope: &str) -> ResearchEvent {
        let at = at();
        ResearchEvent::new(NewResearchEvent {
            owner_scope: owner_scope.to_owned(),
            symbol: "BTCUSDT".to_owned(),
            timeframe: "15m".to_owned(),
            method: "vsa".to_owned(),
            method_version: "1".to_owned(),
            event_type: "ST LONG".to_owned(),
            direction: 1,
            score: 1.0,
            observed_at: at,
            detected_at: at,
            ingested_at: at,
            source_id: "binance".to_owned(),
            provider_method: "binance.klines".to_owned(),
            input_snapshot_id: input_snapshot_id("BTCUSDT", "15m", "vsa", "1", 1, 2, 2),
            quality_state: ResearchQuality::Complete,
            quality_reasons: Vec::new(),
            evidence: BTreeMap::from([("close".to_owned(), 100.0)]),
            context: BTreeMap::new(),
        })
        .unwrap()
    }

    struct MockProvider {
        content: String,
        calls: std::sync::Mutex<usize>,
        clock: Arc<TestClock>,
        advance_on_generate: Option<Duration>,
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
            _request: &tm_trading_ai_providers::AiGenerationRequest,
        ) -> Result<AiGenerationResponse, AiProviderError> {
            *self.calls.lock().unwrap() += 1;
            if let Some(duration) = self.advance_on_generate {
                self.clock.advance(duration);
            }
            Ok(AiGenerationResponse {
                content: self.content.clone(),
                model: "mock".to_owned(),
                prompt_tokens: Some(10),
                completion_tokens: Some(5),
                latency_ms: 1,
            })
        }
    }

    struct MockResolver(Arc<MockProvider>);

    impl AiProviderResolver for MockResolver {
        fn resolve(
            &self,
            _profile: &AiProviderProfile,
            _model: &AiModel,
            _timeout: Duration,
        ) -> Result<Arc<dyn AiProvider>, AiProviderError> {
            Ok(Arc::clone(&self.0) as Arc<dyn AiProvider>)
        }
    }

    fn provider() -> AiProviderProfile {
        AiProviderProfile::new(NewAiProviderProfile {
            id: "provider-1".to_owned(),
            kind: AiProviderKind::OpenAiCompatible,
            display_name: "provider-1".to_owned(),
            base_url: "https://example.com/v1".to_owned(),
            default_model: "model-1".to_owned(),
            timeout_ms: 5_000,
            api_key_env: "TM_TRADING_AI_SECRET_PRIMARY".to_owned(),
            created_at: at(),
        })
        .unwrap()
    }

    fn model() -> AiModel {
        AiModel::new(NewAiModel {
            id: "model-1".to_owned(),
            provider_id: "provider-1".to_owned(),
            model_id: "gpt-x".to_owned(),
            display_name: "model-1".to_owned(),
            context_window: Some(32_000),
            max_output_tokens: Some(2_000),
            supports_streaming: false,
            supports_json_mode: true,
            supports_tools: false,
            supports_vision: false,
            created_at: at(),
        })
        .unwrap()
    }

    fn route() -> AiTaskRoute {
        AiTaskRoute::new(NewAiTaskRoute {
            task: AiTaskKind::ResearchExplanation,
            primary: "provider-1:model-1".to_owned(),
            fallbacks: Vec::new(),
            timeout_ms: 5_000,
            max_attempts: 1,
            concurrency: 1,
            max_output_tokens: Some(1_024),
            created_at: at(),
        })
        .unwrap()
    }

    struct Harness {
        processor: AiJobProcessor,
        jobs: Arc<InMemoryAiJobRepository>,
        briefings: Arc<InMemoryAiJobRepository>,
        provider: Arc<MockProvider>,
    }

    async fn harness(content: &str, event_owner: &str) -> Harness {
        harness_with_delay(content, event_owner, None).await
    }

    async fn harness_with_delay(
        content: &str,
        event_owner: &str,
        advance_on_generate: Option<Duration>,
    ) -> Harness {
        let research = Arc::new(InMemoryResearchEventRepository::new());
        let event = research_event(event_owner);
        research.upsert(std::slice::from_ref(&event)).await.unwrap();

        let news = Arc::new(tm_trading_storage::InMemoryNewsItemRepository::new());
        let assembler = BriefingEvidenceAssembler::new(research, news);

        let routes = Arc::new(InMemoryAiTaskRouteRepository::new());
        routes.upsert(&route()).await.unwrap();
        let providers = Arc::new(InMemoryAiProviderProfileRepository::new());
        providers.upsert(&provider()).await.unwrap();
        let models = Arc::new(InMemoryAiModelRepository::new());
        models.upsert(&model()).await.unwrap();

        let clock = Arc::new(TestClock::new());
        let provider = Arc::new(MockProvider {
            content: content.to_owned(),
            calls: std::sync::Mutex::new(0),
            clock: Arc::clone(&clock),
            advance_on_generate,
        });
        let executor = AiRouteExecutor::new(
            routes,
            providers,
            models,
            Arc::new(MockResolver(Arc::clone(&provider))),
        );

        let jobs = Arc::new(InMemoryAiJobRepository::new());
        let briefings = Arc::clone(&jobs);
        let processor = AiJobProcessor::new_with_clock(
            Arc::clone(&jobs) as Arc<dyn tm_trading_storage::AiJobRepository>,
            assembler,
            executor,
            crate::evidence::DEFAULT_EVIDENCE_MAX_AGE,
            Arc::clone(&clock) as Arc<dyn AiJobClock>,
        );
        Harness {
            processor,
            jobs,
            briefings,
            provider,
        }
    }

    async fn enqueue(harness: &Harness, event_id: &str, owner_scope: &str) -> AiJobId {
        let job = AiJob::new(NewAiJob {
            owner_scope: owner_scope.to_owned(),
            idempotency_key: "explain-1".to_owned(),
            request_hash: "hash-1".to_owned(),
            correlation_id: uuid::Uuid::new_v4(),
            task: AiTaskKind::ResearchExplanation,
            research_event_ids: vec![event_id.to_owned()],
            news_item_ids: Vec::new(),
            requested_at: at(),
            expires_at: at() + chrono::Duration::hours(1),
        })
        .unwrap();
        let job_id = job.id();
        harness.jobs.create(job).await.unwrap();
        job_id
    }

    #[tokio::test]
    async fn publishes_an_evidence_bound_briefing_and_completes_the_job() {
        let event = research_event("local-operator");
        let harness = harness(
            r#"{"interpretations":["Momentum is strengthening."],"uncertainties":["Volume may fade."]}"#,
            "local-operator",
        )
        .await;
        let job_id = enqueue(&harness, event.id(), "local-operator").await;

        let outcome = harness
            .processor
            .process_next("ai-worker", at(), Duration::from_secs(300))
            .await
            .unwrap();
        let AiJobOutcome::Published {
            job_id: published_job,
            briefing_id,
        } = outcome
        else {
            panic!("expected a published briefing");
        };
        assert_eq!(published_job, job_id);

        let stored = harness
            .briefings
            .get_briefing("local-operator", &briefing_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.facts().len(), 1);
        assert_eq!(stored.interpretations().len(), 1);
        assert_eq!(stored.references().len(), 1);
        assert_eq!(stored.provider_id(), "provider-1");
        assert_eq!(stored.prompt_version(), "research-explanation.v1");
        assert_eq!(*harness.provider.calls.lock().unwrap(), 1);

        let job = harness.jobs.get(&job_id).await.unwrap().unwrap();
        assert_eq!(job.status(), tm_trading_domain::AiJobStatus::Succeeded);
        assert_eq!(job.briefing_id(), Some(briefing_id.as_str()));

        assert_eq!(
            harness
                .processor
                .process_next("ai-worker", at(), Duration::from_secs(300))
                .await
                .unwrap(),
            AiJobOutcome::Idle
        );
    }

    #[tokio::test]
    async fn a_lapsed_lease_is_unknown_and_publishes_no_briefing() {
        let event = research_event("local-operator");
        let harness = harness_with_delay(
            r#"{"interpretations":["Momentum is strengthening."],"uncertainties":[]}"#,
            "local-operator",
            Some(Duration::from_secs(61)),
        )
        .await;
        let job_id = enqueue(&harness, event.id(), "local-operator").await;

        let outcome = harness
            .processor
            .process_next("ai-worker", at(), Duration::from_secs(60))
            .await
            .unwrap();
        assert_eq!(outcome, AiJobOutcome::OutcomeUnknown { job_id });

        let job = harness.jobs.get(&job_id).await.unwrap().unwrap();
        assert_eq!(job.status(), tm_trading_domain::AiJobStatus::OutcomeUnknown);
        assert_eq!(job.error_code(), Some("lease_lapsed"));
        assert!(
            harness
                .briefings
                .list_briefings(&AiBriefingQuery {
                    owner_scope: "local-operator".to_owned(),
                    task: None,
                    limit: 10,
                })
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn foreign_evidence_fails_the_job_before_any_provider_call() {
        let event = research_event("another-owner");
        let harness = harness(
            r#"{"interpretations":["ignored"],"uncertainties":[]}"#,
            "another-owner",
        )
        .await;
        let job_id = enqueue(&harness, event.id(), "local-operator").await;

        let outcome = harness
            .processor
            .process_next("ai-worker", at(), Duration::from_secs(300))
            .await
            .unwrap();
        assert_eq!(
            outcome,
            AiJobOutcome::Failed {
                job_id,
                error_code: "no_usable_evidence"
            }
        );
        assert_eq!(*harness.provider.calls.lock().unwrap(), 0);
        assert!(
            harness
                .briefings
                .list_briefings(&AiBriefingQuery {
                    owner_scope: "local-operator".to_owned(),
                    task: None,
                    limit: 10,
                })
                .await
                .unwrap()
                .is_empty()
        );
        let job = harness.jobs.get(&job_id).await.unwrap().unwrap();
        assert_eq!(job.status(), tm_trading_domain::AiJobStatus::Failed);
        assert_eq!(job.error_code(), Some("no_usable_evidence"));
    }

    #[tokio::test]
    async fn a_model_cannot_author_facts_or_citations() {
        let event = research_event("local-operator");
        let harness = harness(
            r#"{"interpretations":["ok"],"uncertainties":[],"facts":["invented"],"references":[]}"#,
            "local-operator",
        )
        .await;
        let job_id = enqueue(&harness, event.id(), "local-operator").await;

        let outcome = harness
            .processor
            .process_next("ai-worker", at(), Duration::from_secs(300))
            .await
            .unwrap();
        assert_eq!(
            outcome,
            AiJobOutcome::Failed {
                job_id,
                error_code: "invalid_model_output"
            }
        );
        let job = harness.jobs.get(&job_id).await.unwrap().unwrap();
        assert_eq!(job.status(), tm_trading_domain::AiJobStatus::Failed);
        assert_eq!(job.error_code(), Some("invalid_model_output"));
    }
}
