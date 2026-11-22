use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mongodb::bson::{Bson, Document, doc};
use mongodb::options::IndexOptions;
use mongodb::{Collection, Database, IndexModel};

use tm_trading_domain::{
    AiBriefing, AiBriefingReference, AiBriefingReferenceKind, AiTaskKind, NewAiBriefing,
};

use crate::bson_support::{from_timestamp, timestamp};
use crate::{AiBriefingQuery, AiBriefingRepository, RepositoryError};

/// Durable `ai_briefings` store.
pub struct MongoAiBriefingRepository {
    collection: Collection<Document>,
}

impl MongoAiBriefingRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        let collection = database.collection::<Document>("ai_briefings");
        let index = IndexModel::builder()
            .keys(doc! { "ownerScope": 1, "generatedAt": -1 })
            .options(
                IndexOptions::builder()
                    .name("ownerScope_generatedAt".to_owned())
                    .build(),
            )
            .build();
        collection.create_indexes(vec![index]).await?;
        Ok(Self { collection })
    }
}

#[async_trait]
impl AiBriefingRepository for MongoAiBriefingRepository {
    async fn upsert(&self, briefing: &AiBriefing) -> Result<(), RepositoryError> {
        let expected = fields(briefing)?;
        let mut update = Document::new();
        update.insert("$setOnInsert", expected.clone());
        self.collection
            .update_one(doc! { "_id": briefing.id() }, update)
            .upsert(true)
            .await?;
        let stored = self
            .collection
            .find_one(doc! { "_id": briefing.id() })
            .await?
            .ok_or(RepositoryError::CorruptRecord("briefing"))?;
        if stored != expected {
            return Err(RepositoryError::AiBriefingConflict);
        }
        Ok(())
    }

    async fn list(&self, query: &AiBriefingQuery) -> Result<Vec<AiBriefing>, RepositoryError> {
        let mut filter = doc! { "ownerScope": query.owner_scope.clone() };
        if let Some(task) = query.task {
            filter.insert("task", task.as_str());
        }
        let limit = i64::try_from(query.limit).unwrap_or(i64::MAX);
        let mut cursor = self
            .collection
            .find(filter)
            .sort(doc! { "generatedAt": -1, "_id": -1 })
            .limit(limit)
            .await?;
        let mut briefings = Vec::new();
        while cursor.advance().await? {
            briefings.push(from_document(cursor.deserialize_current()?)?);
        }
        Ok(briefings)
    }

    async fn get(
        &self,
        owner_scope: &str,
        briefing_id: &str,
    ) -> Result<Option<AiBriefing>, RepositoryError> {
        let Some(document) = self
            .collection
            .find_one(doc! { "_id": briefing_id, "ownerScope": owner_scope })
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(from_document(document)?))
    }
}

fn fields(briefing: &AiBriefing) -> Result<Document, RepositoryError> {
    let references: Vec<Bson> = briefing
        .references()
        .iter()
        .map(|reference| {
            Bson::Document(doc! {
                "kind": reference.kind().as_str(),
                "value": reference.value(),
            })
        })
        .collect();
    Ok(doc! {
        "_id": briefing.id(),
        "ownerScope": briefing.owner_scope(),
        "task": briefing.task().as_str(),
        "providerId": briefing.provider_id(),
        "modelRegistryId": briefing.model_registry_id(),
        "modelId": briefing.model_id(),
        "promptVersion": briefing.prompt_version(),
        "facts": briefing.facts().to_vec(),
        "interpretations": briefing.interpretations().to_vec(),
        "uncertainties": briefing.uncertainties().to_vec(),
        "inputSnapshotIds": briefing.input_snapshot_ids().to_vec(),
        "references": references,
        "missingInputs": briefing.missing_inputs().to_vec(),
        "staleInputs": briefing.stale_inputs().to_vec(),
        "generatedAt": timestamp(briefing.generated_at()),
        "producerVersion": briefing.producer_version(),
        "schemaVersion": briefing.schema_version(),
    })
}

fn from_document(document: Document) -> Result<AiBriefing, RepositoryError> {
    let task = AiTaskKind::parse(&required_text(&document, "task")?)
        .ok_or(RepositoryError::CorruptRecord("task"))?;
    let references = document
        .get_array("references")
        .map_err(|_| RepositoryError::CorruptRecord("references"))?
        .iter()
        .map(|value| {
            let reference = value
                .as_document()
                .ok_or(RepositoryError::CorruptRecord("references"))?;
            let kind = AiBriefingReferenceKind::parse(&required_text(reference, "kind")?)
                .ok_or(RepositoryError::CorruptRecord("references.kind"))?;
            AiBriefingReference::new(kind, required_text(reference, "value")?)
                .map_err(RepositoryError::from)
        })
        .collect::<Result<Vec<_>, RepositoryError>>()?;
    let briefing = AiBriefing::from_stored(
        NewAiBriefing {
            owner_scope: required_text(&document, "ownerScope")?,
            task,
            provider_id: required_text(&document, "providerId")?,
            model_registry_id: required_text(&document, "modelRegistryId")?,
            model_id: required_text(&document, "modelId")?,
            prompt_version: required_text(&document, "promptVersion")?,
            facts: string_list(&document, "facts")?,
            interpretations: string_list(&document, "interpretations")?,
            uncertainties: string_list(&document, "uncertainties")?,
            input_snapshot_ids: string_list(&document, "inputSnapshotIds")?,
            references,
            missing_inputs: string_list(&document, "missingInputs")?,
            stale_inputs: string_list(&document, "staleInputs")?,
            generated_at: required_timestamp(&document, "generatedAt")?,
        },
        required_text(&document, "producerVersion")?,
        required_text(&document, "schemaVersion")?,
    )?;
    if document.get_str("_id").ok() != Some(briefing.id()) {
        return Err(RepositoryError::CorruptRecord("_id"));
    }
    Ok(briefing)
}

fn required_text(document: &Document, field: &'static str) -> Result<String, RepositoryError> {
    match document.get(field) {
        Some(Bson::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
        _ => Err(RepositoryError::CorruptRecord(field)),
    }
}

fn string_list(document: &Document, field: &'static str) -> Result<Vec<String>, RepositoryError> {
    document
        .get_array(field)
        .map_err(|_| RepositoryError::CorruptRecord(field))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or(RepositoryError::CorruptRecord(field))
        })
        .collect()
}

fn required_timestamp(
    document: &Document,
    field: &'static str,
) -> Result<DateTime<Utc>, RepositoryError> {
    match document.get(field) {
        Some(Bson::DateTime(value)) => from_timestamp(*value),
        _ => Err(RepositoryError::CorruptRecord(field)),
    }
}
