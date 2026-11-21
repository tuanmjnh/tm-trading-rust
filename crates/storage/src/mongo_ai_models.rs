use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mongodb::bson::{Bson, Document, doc};
use mongodb::options::IndexOptions;
use mongodb::{Collection, Database, IndexModel};

use tm_trading_domain::{AiModel, AiProviderHealthState, StoredAiModel};

use crate::bson_support::{from_timestamp, timestamp};
use crate::{AiModelRepository, RepositoryError};

pub struct MongoAiModelRepository {
    collection: Collection<Document>,
}

impl MongoAiModelRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        let collection = database.collection::<Document>("ai_models");
        let indexes = vec![
            IndexModel::builder()
                .keys(doc! { "updatedAt": -1 })
                .options(IndexOptions::builder().name("updatedAt".to_owned()).build())
                .build(),
            IndexModel::builder()
                .keys(doc! { "providerId": 1 })
                .options(
                    IndexOptions::builder()
                        .name("providerId".to_owned())
                        .build(),
                )
                .build(),
        ];
        collection.create_indexes(indexes).await?;
        Ok(Self { collection })
    }
}

#[async_trait]
impl AiModelRepository for MongoAiModelRepository {
    async fn list(&self) -> Result<Vec<AiModel>, RepositoryError> {
        let mut cursor = self
            .collection
            .find(Document::new())
            .sort(doc! { "updatedAt": -1 })
            .await?;
        let mut models = Vec::new();
        while cursor.advance().await? {
            models.push(from_document(cursor.deserialize_current()?)?);
        }
        Ok(models)
    }

    async fn get(&self, id: &str) -> Result<Option<AiModel>, RepositoryError> {
        self.collection
            .find_one(doc! { "_id": id })
            .await?
            .map(from_document)
            .transpose()
    }

    async fn upsert(&self, model: &AiModel) -> Result<(), RepositoryError> {
        let filter = doc! { "_id": model.id() };
        let document = doc! {
            "_id": model.id(),
            "providerId": model.provider_id(),
            "modelId": model.model_id(),
            "displayName": model.display_name(),
            "contextWindow": model.context_window().map(i64::from),
            "maxOutputTokens": model.max_output_tokens().map(i64::from),
            "supportsStreaming": model.supports_streaming(),
            "supportsJsonMode": model.supports_json_mode(),
            "supportsTools": model.supports_tools(),
            "supportsVision": model.supports_vision(),
            "enabled": model.enabled(),
            "healthState": model.health_state().as_str(),
            "lastHealthCheckAt": model.last_health_check_at().map(timestamp),
            "lastErrorCode": model.last_error_code(),
            "createdAt": timestamp(model.created_at()),
            "updatedAt": timestamp(model.updated_at()),
        };
        self.collection
            .replace_one(filter, document)
            .upsert(true)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), RepositoryError> {
        let result = self.collection.delete_one(doc! { "_id": id }).await?;
        if result.deleted_count == 0 {
            return Err(RepositoryError::AiModelNotFound);
        }
        Ok(())
    }

    async fn delete_by_provider(&self, provider_id: &str) -> Result<u64, RepositoryError> {
        let result = self
            .collection
            .delete_many(doc! { "providerId": provider_id })
            .await?;
        Ok(result.deleted_count)
    }
}

fn from_document(document: Document) -> Result<AiModel, RepositoryError> {
    let health_state = match optional_text(&document, "healthState")?.as_deref() {
        None => AiProviderHealthState::Unknown,
        Some(value) => AiProviderHealthState::parse(value)
            .ok_or(RepositoryError::CorruptRecord("healthState"))?,
    };
    let last_health_check_at = document
        .get("lastHealthCheckAt")
        .and_then(|value| match value {
            Bson::DateTime(dt) => from_timestamp(*dt).ok(),
            _ => None,
        });
    AiModel::from_stored(StoredAiModel {
        id: required_text(&document, "_id")?,
        provider_id: required_text(&document, "providerId")?,
        model_id: required_text(&document, "modelId")?,
        display_name: required_text(&document, "displayName")?,
        context_window: optional_u32(&document, "contextWindow")?,
        max_output_tokens: optional_u32(&document, "maxOutputTokens")?,
        supports_streaming: optional_bool(&document, "supportsStreaming"),
        supports_json_mode: optional_bool(&document, "supportsJsonMode"),
        supports_tools: optional_bool(&document, "supportsTools"),
        supports_vision: optional_bool(&document, "supportsVision"),
        enabled: optional_bool(&document, "enabled"),
        health_state,
        last_health_check_at,
        last_error_code: optional_text(&document, "lastErrorCode")?,
        created_at: required_timestamp(&document, "createdAt")?,
        updated_at: required_timestamp(&document, "updatedAt")?,
    })
    .map_err(Into::into)
}

fn required_text(document: &Document, field: &'static str) -> Result<String, RepositoryError> {
    match document.get(field) {
        Some(Bson::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
        _ => Err(RepositoryError::CorruptRecord(field)),
    }
}

fn optional_text(
    document: &Document,
    field: &'static str,
) -> Result<Option<String>, RepositoryError> {
    match document.get(field) {
        None | Some(Bson::Null) => Ok(None),
        Some(Bson::String(value)) => {
            let value = value.trim();
            if value.is_empty() {
                Ok(None)
            } else {
                Ok(Some(value.to_owned()))
            }
        }
        Some(_) => Err(RepositoryError::CorruptRecord(field)),
    }
}

fn optional_u32(document: &Document, field: &'static str) -> Result<Option<u32>, RepositoryError> {
    match document.get(field) {
        None | Some(Bson::Null) => Ok(None),
        Some(Bson::Int32(value)) => u32::try_from(*value)
            .map(Some)
            .map_err(|_| RepositoryError::CorruptRecord(field)),
        Some(Bson::Int64(value)) => u32::try_from(*value)
            .map(Some)
            .map_err(|_| RepositoryError::CorruptRecord(field)),
        Some(_) => Err(RepositoryError::CorruptRecord(field)),
    }
}

fn optional_bool(document: &Document, field: &'static str) -> bool {
    document.get(field).and_then(Bson::as_bool).unwrap_or(false)
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
