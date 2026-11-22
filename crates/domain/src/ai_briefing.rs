use chrono::{DateTime, Utc};
use sha1::{Digest, Sha1};
use thiserror::Error;
use url::Url;

use crate::AiTaskKind;
use crate::news_item::NEWS_ITEM_ID_PREFIX;
use crate::research::{RESEARCH_EVENT_ID_PREFIX, RESEARCH_SNAPSHOT_ID_PREFIX};

/// Schema stamp written to every AI briefing document.
pub const AI_BRIEFING_SCHEMA_VERSION: &str = "ai-briefing.v1";

/// Prefix of an AI briefing identifier.
pub const AI_BRIEFING_ID_PREFIX: &str = "aibr1_";

/// Producer that wrote the briefing. It changes whenever the prompt or the
/// evidence assembly changes.
pub const AI_BRIEFING_PRODUCER_VERSION: &str = "tm-trading-ai-worker/0.1.0";

/// Maximum number of entries in any briefing list.
pub const MAX_AI_BRIEFING_LIST_ITEMS: usize = 50;

/// Maximum number of characters in one stored text item.
pub const MAX_AI_BRIEFING_TEXT_CHARS: usize = 4_000;

/// Maximum number of characters across facts, interpretations, and uncertainty.
pub const MAX_AI_BRIEFING_CONTENT_CHARS: usize = 20_000;

/// Kind of source cited by a briefing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiBriefingReferenceKind {
    ResearchEvent,
    NewsItem,
    SourceUrl,
}

impl AiBriefingReferenceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ResearchEvent => "research_event",
            Self::NewsItem => "news_item",
            Self::SourceUrl => "source_url",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "research_event" => Some(Self::ResearchEvent),
            "news_item" => Some(Self::NewsItem),
            "source_url" => Some(Self::SourceUrl),
            _ => None,
        }
    }
}

/// One exact source reference cited by a briefing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiBriefingReference {
    kind: AiBriefingReferenceKind,
    value: String,
}

impl AiBriefingReference {
    pub fn new(
        kind: AiBriefingReferenceKind,
        value: impl Into<String>,
    ) -> Result<Self, AiBriefingError> {
        let value = required("reference.value", value.into())?;
        if value.chars().count() > MAX_AI_BRIEFING_TEXT_CHARS {
            return Err(AiBriefingError::TextTooLong("reference.value"));
        }
        let valid = match kind {
            AiBriefingReferenceKind::ResearchEvent => {
                is_exact_identifier(&value, RESEARCH_EVENT_ID_PREFIX, 16)
            }
            AiBriefingReferenceKind::NewsItem => {
                is_exact_identifier(&value, NEWS_ITEM_ID_PREFIX, 40)
            }
            AiBriefingReferenceKind::SourceUrl => is_source_url(&value),
        };
        if !valid {
            return Err(AiBriefingError::InvalidReferenceValue(kind.as_str()));
        }
        Ok(Self { kind, value })
    }

    #[must_use]
    pub fn kind(&self) -> AiBriefingReferenceKind {
        self.kind
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// The fields a producer supplies for one AI briefing.
#[derive(Debug, Clone, PartialEq)]
pub struct NewAiBriefing {
    pub owner_scope: String,
    pub task: AiTaskKind,
    /// Provider profile that served the request.
    pub provider_id: String,
    /// Registered model that served the request.
    pub model_registry_id: String,
    /// Model name sent to the provider.
    pub model_id: String,
    pub prompt_version: String,
    /// Reported facts copied from the evidence.
    pub facts: Vec<String>,
    /// Model interpretation, kept separate from reported facts.
    pub interpretations: Vec<String>,
    /// Explicit uncertainty or caveats.
    pub uncertainties: Vec<String>,
    /// Identifiers of the exact input snapshots the model was given.
    pub input_snapshot_ids: Vec<String>,
    /// Research, news, or source references cited by the output.
    pub references: Vec<AiBriefingReference>,
    /// Inputs the request could not use.
    pub missing_inputs: Vec<String>,
    /// Inputs that were used but older than the caller allows.
    pub stale_inputs: Vec<String>,
    pub generated_at: DateTime<Utc>,
}

/// One evidence-bound AI briefing.
///
/// A briefing is information, never an instruction. It records what the model
/// said, which model said it, and which evidence it was allowed to see. It never
/// mutates a research event and never reaches an execution path.
#[derive(Debug, Clone, PartialEq)]
pub struct AiBriefing {
    id: String,
    owner_scope: String,
    task: AiTaskKind,
    provider_id: String,
    model_registry_id: String,
    model_id: String,
    prompt_version: String,
    facts: Vec<String>,
    interpretations: Vec<String>,
    uncertainties: Vec<String>,
    input_snapshot_ids: Vec<String>,
    references: Vec<AiBriefingReference>,
    missing_inputs: Vec<String>,
    stale_inputs: Vec<String>,
    generated_at: DateTime<Utc>,
    producer_version: String,
    schema_version: String,
}

impl AiBriefing {
    /// Builds a new briefing and derives its stable identifier.
    pub fn new(input: NewAiBriefing) -> Result<Self, AiBriefingError> {
        Self::build(
            input,
            AI_BRIEFING_PRODUCER_VERSION.to_owned(),
            AI_BRIEFING_SCHEMA_VERSION.to_owned(),
        )
    }

    /// Restores a briefing with the producer and schema stamps found in storage.
    pub fn from_stored(
        input: NewAiBriefing,
        producer_version: String,
        schema_version: String,
    ) -> Result<Self, AiBriefingError> {
        Self::build(
            input,
            required("producerVersion", producer_version)?,
            required("schemaVersion", schema_version)?,
        )
    }

    fn build(
        input: NewAiBriefing,
        producer_version: String,
        schema_version: String,
    ) -> Result<Self, AiBriefingError> {
        let owner_scope = required("ownerScope", input.owner_scope)?;
        let provider_id = required("providerId", input.provider_id)?;
        let model_registry_id = required("modelRegistryId", input.model_registry_id)?;
        let model_id = required("modelId", input.model_id)?;
        let prompt_version = required("promptVersion", input.prompt_version)?;
        let facts = text_list("facts", input.facts)?;
        let interpretations = text_list("interpretations", input.interpretations)?;
        let uncertainties = text_list("uncertainties", input.uncertainties)?;
        let content_chars = facts
            .iter()
            .chain(&interpretations)
            .chain(&uncertainties)
            .map(|value| value.chars().count())
            .sum::<usize>();
        if content_chars > MAX_AI_BRIEFING_CONTENT_CHARS {
            return Err(AiBriefingError::ContentTooLong);
        }
        if facts.is_empty() && interpretations.is_empty() && uncertainties.is_empty() {
            return Err(AiBriefingError::NoContent);
        }
        let input_snapshot_ids = snapshot_id_list(input.input_snapshot_ids)?;
        if input.references.len() > MAX_AI_BRIEFING_LIST_ITEMS {
            return Err(AiBriefingError::TooManyItems("references"));
        }
        if input_snapshot_ids.is_empty() && input.references.is_empty() {
            return Err(AiBriefingError::NoEvidence);
        }
        let missing_inputs = text_list("missingInputs", input.missing_inputs)?;
        let stale_inputs = text_list("staleInputs", input.stale_inputs)?;
        let mut briefing = Self {
            id: String::new(),
            owner_scope,
            task: input.task,
            provider_id,
            model_registry_id,
            model_id,
            prompt_version,
            facts,
            interpretations,
            uncertainties,
            input_snapshot_ids,
            references: input.references,
            missing_inputs,
            stale_inputs,
            generated_at: input.generated_at,
            producer_version,
            schema_version,
        };
        briefing.id = briefing_id(&briefing);
        Ok(briefing)
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
    #[must_use]
    pub fn owner_scope(&self) -> &str {
        &self.owner_scope
    }
    #[must_use]
    pub fn task(&self) -> AiTaskKind {
        self.task
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
    pub fn model_id(&self) -> &str {
        &self.model_id
    }
    #[must_use]
    pub fn prompt_version(&self) -> &str {
        &self.prompt_version
    }
    #[must_use]
    pub fn facts(&self) -> &[String] {
        &self.facts
    }
    #[must_use]
    pub fn interpretations(&self) -> &[String] {
        &self.interpretations
    }
    #[must_use]
    pub fn uncertainties(&self) -> &[String] {
        &self.uncertainties
    }
    #[must_use]
    pub fn input_snapshot_ids(&self) -> &[String] {
        &self.input_snapshot_ids
    }
    #[must_use]
    pub fn references(&self) -> &[AiBriefingReference] {
        &self.references
    }
    #[must_use]
    pub fn missing_inputs(&self) -> &[String] {
        &self.missing_inputs
    }
    #[must_use]
    pub fn stale_inputs(&self) -> &[String] {
        &self.stale_inputs
    }
    #[must_use]
    pub fn generated_at(&self) -> DateTime<Utc> {
        self.generated_at
    }
    #[must_use]
    pub fn producer_version(&self) -> &str {
        &self.producer_version
    }
    #[must_use]
    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }
}

/// Stable identifier of one briefing.
///
/// The identifier covers the complete content and evidence payload, so two
/// distinct briefings generated in the same millisecond cannot overwrite each other.
fn briefing_id(briefing: &AiBriefing) -> String {
    let mut canonical = String::new();
    push_component(&mut canonical, briefing.owner_scope());
    push_component(&mut canonical, briefing.task().as_str());
    push_component(&mut canonical, briefing.provider_id());
    push_component(&mut canonical, briefing.model_registry_id());
    push_component(&mut canonical, briefing.model_id());
    push_component(&mut canonical, briefing.prompt_version());
    push_list(&mut canonical, briefing.facts());
    push_list(&mut canonical, briefing.interpretations());
    push_list(&mut canonical, briefing.uncertainties());
    push_list(&mut canonical, briefing.input_snapshot_ids());
    push_component(&mut canonical, &briefing.references().len().to_string());
    for reference in briefing.references() {
        push_component(&mut canonical, reference.kind().as_str());
        push_component(&mut canonical, reference.value());
    }
    push_list(&mut canonical, briefing.missing_inputs());
    push_list(&mut canonical, briefing.stale_inputs());
    push_component(
        &mut canonical,
        &briefing.generated_at().timestamp_millis().to_string(),
    );
    push_component(&mut canonical, briefing.producer_version());
    push_component(&mut canonical, briefing.schema_version());
    format!(
        "{AI_BRIEFING_ID_PREFIX}{}",
        short_sha1_hex(canonical.as_bytes())
    )
}

fn push_list(canonical: &mut String, values: &[String]) {
    push_component(canonical, &values.len().to_string());
    for value in values {
        push_component(canonical, value);
    }
}

fn push_component(canonical: &mut String, value: &str) {
    canonical.push_str(&value.len().to_string());
    canonical.push(':');
    canonical.push_str(value);
    canonical.push(';');
}

fn short_sha1_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha1::digest(bytes);
    let mut encoded = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

/// Whether a value is an exact AI briefing identifier.
#[must_use]
pub fn is_valid_ai_briefing_id(value: &str) -> bool {
    is_exact_identifier(value, AI_BRIEFING_ID_PREFIX, 16)
}

fn required(field: &'static str, value: String) -> Result<String, AiBriefingError> {
    if value.trim().is_empty() {
        return Err(AiBriefingError::EmptyField(field));
    }
    Ok(value.trim().to_owned())
}

fn snapshot_id_list(values: Vec<String>) -> Result<Vec<String>, AiBriefingError> {
    if values.len() > MAX_AI_BRIEFING_LIST_ITEMS {
        return Err(AiBriefingError::TooManyItems("inputSnapshotIds"));
    }
    values
        .into_iter()
        .map(|value| {
            let value = required("inputSnapshotIds", value)?;
            if !is_exact_identifier(&value, RESEARCH_SNAPSHOT_ID_PREFIX, 16) {
                return Err(AiBriefingError::InvalidInputSnapshotId);
            }
            Ok(value)
        })
        .collect()
}

fn is_exact_identifier(value: &str, prefix: &str, suffix_len: usize) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.len() == suffix_len
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn is_source_url(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn text_list(field: &'static str, values: Vec<String>) -> Result<Vec<String>, AiBriefingError> {
    if values.len() > MAX_AI_BRIEFING_LIST_ITEMS {
        return Err(AiBriefingError::TooManyItems(field));
    }
    values
        .into_iter()
        .map(|value| {
            let value = required(field, value)?;
            if value.chars().count() > MAX_AI_BRIEFING_TEXT_CHARS {
                return Err(AiBriefingError::TextTooLong(field));
            }
            Ok(value)
        })
        .collect()
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AiBriefingError {
    #[error("ai briefing field '{0}' must not be empty")]
    EmptyField(&'static str),
    #[error("ai briefing field '{0}' contains too many items")]
    TooManyItems(&'static str),
    #[error("ai briefing field '{0}' contains an item that is too long")]
    TextTooLong(&'static str),
    #[error("ai briefing content is too long")]
    ContentTooLong,
    #[error("ai briefing reference '{0}' has an invalid value")]
    InvalidReferenceValue(&'static str),
    #[error("ai briefing contains an invalid input snapshot identifier")]
    InvalidInputSnapshotId,
    #[error("ai briefing must contain facts, interpretations, or uncertainty")]
    NoContent,
    #[error("ai briefing must cite at least one input snapshot or reference")]
    NoEvidence,
}
