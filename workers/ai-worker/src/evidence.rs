//! Evidence assembly and model-output validation for one briefing request.
//!
//! The worker decides what the model may see. Facts, references, snapshot IDs,
//! disclosures, and provenance stay server-controlled; the model only supplies
//! interpretations and uncertainty.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use thiserror::Error;

use tm_trading_ai_providers::{AiGenerationRequest, MAX_GENERATION_PROMPT_BYTES};
use tm_trading_domain::{
    AiBriefing, AiBriefingError, AiBriefingReference, AiBriefingReferenceKind, AiTaskKind,
    MAX_AI_BRIEFING_CONTENT_CHARS, MAX_AI_BRIEFING_LIST_ITEMS, MAX_AI_BRIEFING_TEXT_CHARS,
    NewAiBriefing, NewsItem, ResearchEvent,
};
use tm_trading_storage::{NewsItemRepository, RepositoryError, ResearchEventRepository};

use crate::AiRouteExecution;

/// Prompt version reported by every briefing produced from this profile.
pub const RESEARCH_EXPLANATION_PROMPT_VERSION: &str = "research-explanation.v1";

/// Evidence older than this is used but disclosed as stale.
pub const DEFAULT_EVIDENCE_MAX_AGE: Duration = Duration::from_secs(6 * 60 * 60);

/// Output budget for the bounded worker-readiness generation probe.
pub(super) const CAPABILITY_TEST_MAX_OUTPUT_TOKENS: u32 = 64;

const SYSTEM_PROMPT: &str = "You explain supplied market research and news evidence. \
Use only the evidence in the user message. Never invent prices, facts, sources, or identifiers. \
Reply with a JSON object that has only the keys \"interpretations\" and \"uncertainties\", \
each an array of short strings. Do not restate facts as certainty.";

fn briefing_output_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "interpretations": {
                "type": "array",
                "items": {"type": "string"}
            },
            "uncertainties": {
                "type": "array",
                "items": {"type": "string"}
            }
        },
        "required": ["interpretations", "uncertainties"],
        "additionalProperties": false
    })
}

/// The exact evidence a caller asks the worker to assemble.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefingEvidenceRequest {
    pub owner_scope: String,
    pub research_event_ids: Vec<String>,
    pub news_item_ids: Vec<String>,
}

/// Evidence accepted for one briefing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefingEvidence {
    facts: Vec<String>,
    references: Vec<AiBriefingReference>,
    input_snapshot_ids: Vec<String>,
    missing_inputs: Vec<String>,
    stale_inputs: Vec<String>,
}

impl BriefingEvidence {
    #[must_use]
    pub fn facts(&self) -> &[String] {
        &self.facts
    }
    #[must_use]
    pub fn references(&self) -> &[AiBriefingReference] {
        &self.references
    }
    #[must_use]
    pub fn input_snapshot_ids(&self) -> &[String] {
        &self.input_snapshot_ids
    }
    #[must_use]
    pub fn missing_inputs(&self) -> &[String] {
        &self.missing_inputs
    }
    #[must_use]
    pub fn stale_inputs(&self) -> &[String] {
        &self.stale_inputs
    }
}

/// Assembles owner-scoped evidence from stored research and news.
#[derive(Clone)]
pub struct BriefingEvidenceAssembler {
    research: Arc<dyn ResearchEventRepository>,
    news: Arc<dyn NewsItemRepository>,
}

impl BriefingEvidenceAssembler {
    #[must_use]
    pub fn new(
        research: Arc<dyn ResearchEventRepository>,
        news: Arc<dyn NewsItemRepository>,
    ) -> Self {
        Self { research, news }
    }

    /// Reads every requested input and keeps facts, references, and disclosures apart.
    ///
    /// A foreign research event and a missing one are disclosed identically so the
    /// worker never confirms that another owner's record exists. News is used only
    /// when its rights snapshot permits both display and summarization.
    pub async fn assemble(
        &self,
        request: &BriefingEvidenceRequest,
        now: DateTime<Utc>,
        max_age: Duration,
    ) -> Result<BriefingEvidence, EvidenceError> {
        let owner_scope = request.owner_scope.trim();
        if owner_scope.is_empty() {
            return Err(EvidenceError::EmptyOwnerScope);
        }
        let research_ids = unique_ids(
            &request.research_event_ids,
            AiBriefingReferenceKind::ResearchEvent,
        )?;
        let news_ids = unique_ids(&request.news_item_ids, AiBriefingReferenceKind::NewsItem)?;
        if research_ids.len() + news_ids.len() > MAX_AI_BRIEFING_LIST_ITEMS {
            return Err(EvidenceError::TooManyInputs);
        }

        let mut evidence = BriefingEvidence {
            facts: Vec::new(),
            references: Vec::new(),
            input_snapshot_ids: Vec::new(),
            missing_inputs: Vec::new(),
            stale_inputs: Vec::new(),
        };

        for id in research_ids {
            match self.research.get(&id).await? {
                Some(event) if event.owner_scope() == owner_scope => {
                    let fact = research_fact(&event)?;
                    if is_stale(now, event.detected_at(), max_age) {
                        evidence.stale_inputs.push(id.clone());
                    }
                    push_unique(&mut evidence.input_snapshot_ids, event.input_snapshot_id());
                    evidence.facts.push(fact);
                    evidence.references.push(AiBriefingReference::new(
                        AiBriefingReferenceKind::ResearchEvent,
                        id,
                    )?);
                }
                _ => evidence
                    .missing_inputs
                    .push(format!("research_event:{id}:missing")),
            }
        }

        for id in news_ids {
            match self.news.get(&id).await? {
                Some(item) if item.display_allowed() && item.rights().allowed_summarize => {
                    evidence.facts.push(news_fact(&item)?);
                    if is_stale(now, item.ingested_at(), max_age) {
                        evidence.stale_inputs.push(id.clone());
                    }
                    evidence.references.push(AiBriefingReference::new(
                        AiBriefingReferenceKind::NewsItem,
                        id,
                    )?);
                }
                Some(_) => evidence
                    .missing_inputs
                    .push(format!("news_item:{id}:restricted")),
                None => evidence
                    .missing_inputs
                    .push(format!("news_item:{id}:missing")),
            }
        }

        if evidence.facts.is_empty() {
            return Err(EvidenceError::NoUsableEvidence);
        }
        Ok(evidence)
    }
}

/// What the model is allowed to return: interpretation and uncertainty only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelBriefing {
    pub interpretations: Vec<String>,
    pub uncertainties: Vec<String>,
}

/// Builds one bounded request that verifies the provider and expected response schema.
pub(super) fn capability_test_request() -> AiGenerationRequest {
    AiGenerationRequest {
        system_prompt: SYSTEM_PROMPT.to_owned(),
        user_prompt: "Readiness test only. Return one short interpretation and one short \
                      uncertainty, and make no market claim."
            .to_owned(),
        max_output_tokens: CAPABILITY_TEST_MAX_OUTPUT_TOKENS,
        json_mode: true,
        json_schema: Some(briefing_output_schema()),
    }
}

/// Builds the bounded `research_explanation` request from assembled evidence.
pub fn research_explanation_request(
    evidence: &BriefingEvidence,
    max_output_tokens: u32,
) -> Result<AiGenerationRequest, EvidenceError> {
    let mut user_prompt = String::from("Evidence available to you:\n");
    for fact in &evidence.facts {
        user_prompt.push_str("- ");
        user_prompt.push_str(fact);
        user_prompt.push('\n');
    }
    if !evidence.missing_inputs.is_empty() {
        user_prompt.push_str("\nMissing or unusable inputs:\n");
        for input in &evidence.missing_inputs {
            user_prompt.push_str("- ");
            user_prompt.push_str(input);
            user_prompt.push('\n');
        }
    }
    if !evidence.stale_inputs.is_empty() {
        user_prompt.push_str("\nStale inputs:\n");
        for input in &evidence.stale_inputs {
            user_prompt.push_str("- ");
            user_prompt.push_str(input);
            user_prompt.push('\n');
        }
    }
    if SYSTEM_PROMPT.len() + user_prompt.len() > MAX_GENERATION_PROMPT_BYTES {
        return Err(EvidenceError::PromptTooLong);
    }
    Ok(AiGenerationRequest {
        system_prompt: SYSTEM_PROMPT.to_owned(),
        user_prompt,
        max_output_tokens,
        json_mode: true,
        json_schema: Some(briefing_output_schema()),
    })
}

/// Parses one model response, rejecting anything the model may not author.
pub fn parse_model_output(content: &str) -> Result<ModelBriefing, EvidenceError> {
    let value: Value =
        serde_json::from_str(content.trim()).map_err(|_| EvidenceError::InvalidModelOutput)?;
    let Value::Object(object) = value else {
        return Err(EvidenceError::InvalidModelOutput);
    };
    let allowed = ["interpretations", "uncertainties"];
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(EvidenceError::UnknownModelField(key.clone()));
        }
    }
    let interpretations = model_text_list(&object, "interpretations")?;
    let uncertainties = model_text_list(&object, "uncertainties")?;
    let content_chars = interpretations
        .iter()
        .chain(&uncertainties)
        .map(|value| value.chars().count())
        .sum::<usize>();
    if content_chars > MAX_AI_BRIEFING_CONTENT_CHARS {
        return Err(EvidenceError::InvalidModelOutput);
    }
    if interpretations.is_empty() && uncertainties.is_empty() {
        return Err(EvidenceError::EmptyModelOutput);
    }
    Ok(ModelBriefing {
        interpretations,
        uncertainties,
    })
}

/// Combines server-owned evidence with the validated model output.
pub fn build_briefing(
    owner_scope: &str,
    task: AiTaskKind,
    prompt_version: &str,
    execution: &AiRouteExecution,
    evidence: &BriefingEvidence,
    output: &ModelBriefing,
    generated_at: DateTime<Utc>,
) -> Result<AiBriefing, AiBriefingError> {
    AiBriefing::new(NewAiBriefing {
        owner_scope: owner_scope.to_owned(),
        task,
        provider_id: execution.provider_id.clone(),
        model_registry_id: execution.model_registry_id.clone(),
        model_id: execution.model_id.clone(),
        prompt_version: prompt_version.to_owned(),
        facts: evidence.facts.clone(),
        interpretations: output.interpretations.clone(),
        uncertainties: output.uncertainties.clone(),
        input_snapshot_ids: evidence.input_snapshot_ids.clone(),
        references: evidence.references.clone(),
        missing_inputs: evidence.missing_inputs.clone(),
        stale_inputs: evidence.stale_inputs.clone(),
        generated_at,
    })
}

fn unique_ids(
    values: &[String],
    kind: AiBriefingReferenceKind,
) -> Result<Vec<String>, EvidenceError> {
    if values.len() > MAX_AI_BRIEFING_LIST_ITEMS {
        return Err(EvidenceError::TooManyInputs);
    }
    let mut unique = Vec::with_capacity(values.len());
    for value in values {
        let reference = AiBriefingReference::new(kind, value.trim())
            .map_err(|_| EvidenceError::InvalidInputId(kind.as_str()))?;
        push_unique(&mut unique, reference.value());
    }
    Ok(unique)
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_owned());
    }
}

fn research_fact(event: &ResearchEvent) -> Result<String, EvidenceError> {
    let evidence = event
        .evidence()
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(", ");
    let fact = format!(
        "research_event {}: symbol {}, timeframe {}, event type {}, direction {:+}, \
         score {:.4}, method {}/{}, quality {}, observed_at {}, detected_at {}, evidence [{}]",
        event.id(),
        event.symbol(),
        event.timeframe(),
        event.event_type(),
        event.direction(),
        event.score(),
        event.method(),
        event.method_version(),
        event.quality_state().as_str(),
        event.observed_at().to_rfc3339(),
        event.detected_at().to_rfc3339(),
        evidence,
    );
    bounded_fact(event.id(), fact)
}

fn news_fact(item: &NewsItem) -> Result<String, EvidenceError> {
    let published = item
        .published_at()
        .map_or_else(|| "unknown".to_owned(), |at| at.to_rfc3339());
    let fact = format!(
        "news_item {}: title \"{}\", excerpt \"{}\", source {}, level {}, direction {}, \
         published_at {}, ingested_at {}",
        item.item_id(),
        item.title(),
        item.excerpt(),
        item.source_id(),
        item.level().as_str(),
        item.direction().as_str(),
        published,
        item.ingested_at().to_rfc3339(),
    );
    bounded_fact(item.item_id(), fact)
}

fn bounded_fact(id: &str, fact: String) -> Result<String, EvidenceError> {
    if fact.chars().count() > MAX_AI_BRIEFING_TEXT_CHARS {
        return Err(EvidenceError::FactTooLong(id.to_owned()));
    }
    Ok(fact)
}

fn is_stale(now: DateTime<Utc>, at: DateTime<Utc>, max_age: Duration) -> bool {
    now.signed_duration_since(at)
        .to_std()
        .is_ok_and(|age| age > max_age)
}

fn model_text_list(object: &Map<String, Value>, key: &str) -> Result<Vec<String>, EvidenceError> {
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    let Value::Array(items) = value else {
        return Err(EvidenceError::InvalidModelOutput);
    };
    if items.len() > MAX_AI_BRIEFING_LIST_ITEMS {
        return Err(EvidenceError::InvalidModelOutput);
    }
    items
        .iter()
        .map(|item| {
            let Value::String(text) = item else {
                return Err(EvidenceError::InvalidModelOutput);
            };
            let text = text.trim();
            if text.is_empty() || text.chars().count() > MAX_AI_BRIEFING_TEXT_CHARS {
                return Err(EvidenceError::InvalidModelOutput);
            }
            Ok(text.to_owned())
        })
        .collect()
}

#[derive(Debug, Error)]
pub enum EvidenceError {
    #[error("briefing evidence requires an owner scope")]
    EmptyOwnerScope,
    #[error("briefing evidence request contains too many inputs")]
    TooManyInputs,
    #[error("briefing evidence request contains an invalid {0} identifier")]
    InvalidInputId(&'static str),
    #[error("evidence fact for '{0}' is too long to store")]
    FactTooLong(String),
    #[error("briefing evidence contains no usable input")]
    NoUsableEvidence,
    #[error("ai provider response is not a valid briefing object")]
    InvalidModelOutput,
    #[error("ai provider response contains a field the model may not author: {0}")]
    UnknownModelField(String),
    #[error("ai provider response contains no interpretation or uncertainty")]
    EmptyModelOutput,
    #[error("briefing prompt exceeds the provider request bound")]
    PromptTooLong,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error(transparent)]
    Briefing(#[from] AiBriefingError),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::TimeZone;

    use super::*;
    use tm_trading_ai_providers::AiGenerationResponse;
    use tm_trading_domain::{
        NewNewsItem, NewResearchEvent, NewsDirection, NewsItemRights, NewsLevel, ResearchQuality,
        input_snapshot_id,
    };
    use tm_trading_storage::{InMemoryNewsItemRepository, InMemoryResearchEventRepository};

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 22, 8, 0, 0).unwrap()
    }

    fn research_event(owner_scope: &str, detected_at: DateTime<Utc>) -> ResearchEvent {
        ResearchEvent::new(NewResearchEvent {
            owner_scope: owner_scope.to_owned(),
            symbol: "BTCUSDT".to_owned(),
            timeframe: "15m".to_owned(),
            method: "vsa".to_owned(),
            method_version: "1".to_owned(),
            event_type: "ST LONG".to_owned(),
            direction: 1,
            score: 1.0,
            observed_at: detected_at,
            detected_at,
            ingested_at: detected_at,
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

    fn news_item(url: &str, allowed_summarize: bool) -> NewsItem {
        NewsItem::new(NewNewsItem {
            source_id: "coindesk".to_owned(),
            url: url.to_owned(),
            origin_url: url.to_owned(),
            title: "Bitcoin holds above support".to_owned(),
            excerpt: "A short summary.".to_owned(),
            language: "en".to_owned(),
            published_at: None,
            first_seen_at: at(),
            ingested_at: at(),
            market_group: None,
            source_market_groups: vec!["crypto".to_owned()],
            event_type: None,
            tags: Vec::new(),
            level: NewsLevel::Med,
            score: 0.5,
            direction: NewsDirection::Flat,
            rights: NewsItemRights {
                allowed_display: true,
                allowed_summarize,
            },
        })
        .unwrap()
    }

    async fn assembler(
        research: Vec<ResearchEvent>,
        news: Vec<NewsItem>,
    ) -> BriefingEvidenceAssembler {
        let research_repository = Arc::new(InMemoryResearchEventRepository::new());
        research_repository.upsert(&research).await.unwrap();
        let news_repository = Arc::new(InMemoryNewsItemRepository::new());
        news_repository.upsert_seen(&news).await.unwrap();
        BriefingEvidenceAssembler::new(research_repository, news_repository)
    }

    fn execution() -> AiRouteExecution {
        AiRouteExecution {
            provider_id: "provider-1".to_owned(),
            model_registry_id: "model-1".to_owned(),
            model_id: "gpt-x".to_owned(),
            response: AiGenerationResponse {
                content: "{}".to_owned(),
                model: "gpt-x".to_owned(),
                prompt_tokens: Some(10),
                completion_tokens: Some(5),
                latency_ms: 12,
            },
            attempts: Vec::new(),
        }
    }

    async fn one_of_each() -> (
        BriefingEvidenceAssembler,
        ResearchEvent,
        NewsItem,
        DateTime<Utc>,
    ) {
        let event = research_event("local-operator", at());
        let item = news_item("https://www.coindesk.com/story", true);
        let assembler = assembler(vec![event.clone()], vec![item.clone()]).await;
        (assembler, event, item, at() + chrono::Duration::minutes(1))
    }

    #[tokio::test]
    async fn assembles_owned_research_and_summarizable_news() {
        let (assembler, event, item, now) = one_of_each().await;
        let evidence = assembler
            .assemble(
                &BriefingEvidenceRequest {
                    owner_scope: "local-operator".to_owned(),
                    research_event_ids: vec![event.id().to_owned()],
                    news_item_ids: vec![item.item_id().to_owned()],
                },
                now,
                DEFAULT_EVIDENCE_MAX_AGE,
            )
            .await
            .unwrap();

        assert_eq!(evidence.facts().len(), 2);
        assert!(evidence.facts()[0].contains(event.id()));
        assert!(evidence.facts()[1].contains(item.item_id()));
        assert_eq!(
            evidence.input_snapshot_ids(),
            [event.input_snapshot_id().to_owned()]
        );
        assert_eq!(evidence.references().len(), 2);
        assert_eq!(
            evidence.references()[0].kind(),
            AiBriefingReferenceKind::ResearchEvent
        );
        assert_eq!(evidence.references()[0].value(), event.id());
        assert_eq!(
            evidence.references()[1].kind(),
            AiBriefingReferenceKind::NewsItem
        );
        assert!(evidence.missing_inputs().is_empty());
        assert!(evidence.stale_inputs().is_empty());
    }

    #[tokio::test]
    async fn foreign_and_restricted_inputs_become_disclosures_only() {
        let owned = research_event("local-operator", at());
        let foreign = research_event("another-owner", at());
        let restricted = news_item("https://www.coindesk.com/restricted", false);
        let assembler = assembler(
            vec![owned.clone(), foreign.clone()],
            vec![restricted.clone()],
        )
        .await;

        let evidence = assembler
            .assemble(
                &BriefingEvidenceRequest {
                    owner_scope: "local-operator".to_owned(),
                    research_event_ids: vec![foreign.id().to_owned(), owned.id().to_owned()],
                    news_item_ids: vec![restricted.item_id().to_owned()],
                },
                at(),
                DEFAULT_EVIDENCE_MAX_AGE,
            )
            .await
            .unwrap();

        assert_eq!(
            evidence.missing_inputs(),
            [
                format!("research_event:{}:missing", foreign.id()),
                format!("news_item:{}:restricted", restricted.item_id()),
            ]
        );
        assert!(
            evidence
                .facts()
                .iter()
                .all(|fact| !fact.contains(foreign.id()) && !fact.contains(restricted.item_id()))
        );
        assert_eq!(evidence.references().len(), 1);
        assert_eq!(evidence.references()[0].value(), owned.id());
    }

    #[tokio::test]
    async fn stale_evidence_is_used_but_disclosed() {
        let (assembler, event, item, _) = one_of_each().await;
        let evidence = assembler
            .assemble(
                &BriefingEvidenceRequest {
                    owner_scope: "local-operator".to_owned(),
                    research_event_ids: vec![event.id().to_owned()],
                    news_item_ids: vec![item.item_id().to_owned()],
                },
                at() + chrono::Duration::hours(7),
                DEFAULT_EVIDENCE_MAX_AGE,
            )
            .await
            .unwrap();

        assert_eq!(
            evidence.stale_inputs(),
            [event.id().to_owned(), item.item_id().to_owned()]
        );
        assert_eq!(evidence.facts().len(), 2);
        assert_eq!(evidence.references().len(), 2);
    }

    #[tokio::test]
    async fn rejects_an_empty_owner_invalid_ids_and_oversized_requests() {
        let (assembler, event, _, now) = one_of_each().await;
        assert!(matches!(
            assembler
                .assemble(
                    &BriefingEvidenceRequest {
                        owner_scope: " ".to_owned(),
                        research_event_ids: Vec::new(),
                        news_item_ids: Vec::new(),
                    },
                    now,
                    DEFAULT_EVIDENCE_MAX_AGE,
                )
                .await,
            Err(EvidenceError::EmptyOwnerScope)
        ));
        assert!(matches!(
            assembler
                .assemble(
                    &BriefingEvidenceRequest {
                        owner_scope: "local-operator".to_owned(),
                        research_event_ids: vec!["rev1_NOTHEX".to_owned()],
                        news_item_ids: Vec::new(),
                    },
                    now,
                    DEFAULT_EVIDENCE_MAX_AGE,
                )
                .await,
            Err(EvidenceError::InvalidInputId("research_event"))
        ));
        let too_many = vec![event.id().to_owned(); MAX_AI_BRIEFING_LIST_ITEMS + 1];
        assert!(matches!(
            assembler
                .assemble(
                    &BriefingEvidenceRequest {
                        owner_scope: "local-operator".to_owned(),
                        research_event_ids: too_many,
                        news_item_ids: Vec::new(),
                    },
                    now,
                    DEFAULT_EVIDENCE_MAX_AGE,
                )
                .await,
            Err(EvidenceError::TooManyInputs)
        ));
        assert!(matches!(
            assembler
                .assemble(
                    &BriefingEvidenceRequest {
                        owner_scope: "local-operator".to_owned(),
                        research_event_ids: Vec::new(),
                        news_item_ids: Vec::new(),
                    },
                    now,
                    DEFAULT_EVIDENCE_MAX_AGE,
                )
                .await,
            Err(EvidenceError::NoUsableEvidence)
        ));
    }

    #[test]
    fn accepts_only_model_interpretation_and_uncertainty() {
        let parsed = parse_model_output(
            r#"{"interpretations":["Momentum may continue"],"uncertainties":["Thin sample"]}"#,
        )
        .unwrap();
        assert_eq!(parsed.interpretations, ["Momentum may continue"]);
        assert_eq!(parsed.uncertainties, ["Thin sample"]);

        assert!(matches!(
            parse_model_output(r#"{"facts":["BTC is 100"]}"#),
            Err(EvidenceError::UnknownModelField(field)) if field == "facts"
        ));
        assert!(matches!(
            parse_model_output(r#"{"references":["rev1_0123456789abcdef"]}"#),
            Err(EvidenceError::UnknownModelField(_))
        ));
        assert!(matches!(
            parse_model_output(r#"{"interpretations":[]}"#),
            Err(EvidenceError::EmptyModelOutput)
        ));
        assert!(matches!(
            parse_model_output(r#"{"interpretations":[1]}"#),
            Err(EvidenceError::InvalidModelOutput)
        ));
        assert!(matches!(
            parse_model_output(r#"{"interpretations":["  "]}"#),
            Err(EvidenceError::InvalidModelOutput)
        ));
        assert!(matches!(
            parse_model_output("not json"),
            Err(EvidenceError::InvalidModelOutput)
        ));
    }

    #[test]
    fn rejects_model_output_that_exceeds_the_final_content_budget() {
        let content = serde_json::json!({
            "interpretations": vec!["x".repeat(MAX_AI_BRIEFING_TEXT_CHARS); 6],
            "uncertainties": [],
        })
        .to_string();

        assert!(matches!(
            parse_model_output(&content),
            Err(EvidenceError::InvalidModelOutput)
        ));
    }

    #[test]
    fn builds_a_briefing_from_server_facts_and_model_output() {
        let event = research_event("local-operator", at());
        let evidence = BriefingEvidence {
            facts: vec![format!("research_event {}", event.id())],
            references: vec![
                AiBriefingReference::new(AiBriefingReferenceKind::ResearchEvent, event.id())
                    .unwrap(),
            ],
            input_snapshot_ids: vec![event.input_snapshot_id().to_owned()],
            missing_inputs: vec!["news_item:news_x:missing".to_owned()],
            stale_inputs: Vec::new(),
        };
        let output = ModelBriefing {
            interpretations: vec!["Momentum may continue".to_owned()],
            uncertainties: vec!["Only one input was usable".to_owned()],
        };

        let briefing = build_briefing(
            "local-operator",
            AiTaskKind::ResearchExplanation,
            RESEARCH_EXPLANATION_PROMPT_VERSION,
            &execution(),
            &evidence,
            &output,
            at(),
        )
        .unwrap();

        assert!(
            briefing
                .id()
                .starts_with(tm_trading_domain::AI_BRIEFING_ID_PREFIX)
        );
        assert_eq!(briefing.provider_id(), "provider-1");
        assert_eq!(briefing.model_registry_id(), "model-1");
        assert_eq!(briefing.model_id(), "gpt-x");
        assert_eq!(
            briefing.prompt_version(),
            RESEARCH_EXPLANATION_PROMPT_VERSION
        );
        assert_eq!(briefing.facts(), evidence.facts());
        assert_eq!(briefing.interpretations(), output.interpretations);
        assert_eq!(briefing.uncertainties(), output.uncertainties);
        assert_eq!(briefing.references(), evidence.references());
        assert_eq!(briefing.input_snapshot_ids(), evidence.input_snapshot_ids());
        assert_eq!(briefing.missing_inputs(), evidence.missing_inputs());
        assert_eq!(
            briefing.producer_version(),
            tm_trading_domain::AI_BRIEFING_PRODUCER_VERSION
        );
    }

    #[tokio::test]
    async fn prompt_carries_only_supplied_evidence_and_disclosures() {
        let (assembler, event, item, now) = one_of_each().await;
        let evidence = assembler
            .assemble(
                &BriefingEvidenceRequest {
                    owner_scope: "local-operator".to_owned(),
                    research_event_ids: vec![event.id().to_owned()],
                    news_item_ids: vec![item.item_id().to_owned()],
                },
                now,
                DEFAULT_EVIDENCE_MAX_AGE,
            )
            .await
            .unwrap();

        let request = research_explanation_request(&evidence, 512).unwrap();
        assert!(request.json_mode);
        assert_eq!(
            request.json_schema.as_ref().expect("output schema")["required"],
            serde_json::json!(["interpretations", "uncertainties"])
        );
        assert_eq!(request.max_output_tokens, 512);
        assert!(request.system_prompt.contains("\"interpretations\""));
        for fact in evidence.facts() {
            assert!(request.user_prompt.contains(fact));
        }
        assert!(!request.user_prompt.contains("Missing or unusable inputs"));
    }
}
