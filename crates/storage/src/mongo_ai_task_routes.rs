use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mongodb::bson::{Bson, Document, doc};
use mongodb::options::IndexOptions;
use mongodb::{Collection, Database, IndexModel};

use tm_trading_domain::{AiTaskKind, AiTaskRoute, StoredAiTaskRoute};

use crate::bson_support::{from_timestamp, timestamp};
use crate::{AiTaskRouteRepository, RepositoryError};

pub struct MongoAiTaskRouteRepository {
    collection: Collection<Document>,
}

impl MongoAiTaskRouteRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        let collection = database.collection::<Document>("ai_task_routes");
        let index = IndexModel::builder()
            .keys(doc! { "updatedAt": -1 })
            .options(IndexOptions::builder().name("updatedAt".to_owned()).build())
            .build();
        collection.create_indexes(vec![index]).await?;
        Ok(Self { collection })
    }
}

#[async_trait]
impl AiTaskRouteRepository for MongoAiTaskRouteRepository {
    async fn list(&self) -> Result<Vec<AiTaskRoute>, RepositoryError> {
        let mut cursor = self
            .collection
            .find(Document::new())
            .sort(doc! { "updatedAt": -1 })
            .await?;
        let mut routes = Vec::new();
        while cursor.advance().await? {
            routes.push(from_document(cursor.deserialize_current()?)?);
        }
        Ok(routes)
    }

    async fn get(&self, task: &str) -> Result<Option<AiTaskRoute>, RepositoryError> {
        self.collection
            .find_one(doc! { "_id": task })
            .await?
            .map(from_document)
            .transpose()
    }

    async fn upsert(&self, route: &AiTaskRoute) -> Result<(), RepositoryError> {
        let filter = doc! { "_id": route.task().as_str() };
        let document = doc! {
            "_id": route.task().as_str(),
            "task": route.task().as_str(),
            "primary": route.primary(),
            "fallbacks": route.fallbacks(),
            "enabled": route.enabled(),
            "timeoutMs": i64::try_from(route.timeout_ms()).unwrap_or(0),
            "maxAttempts": i64::from(route.max_attempts()),
            "concurrency": i64::from(route.concurrency()),
            "maxOutputTokens": route.max_output_tokens().map(i64::from),
            "createdAt": timestamp(route.created_at()),
            "updatedAt": timestamp(route.updated_at()),
        };
        self.collection
            .replace_one(filter, document)
            .upsert(true)
            .await?;
        Ok(())
    }

    async fn delete(&self, task: &str) -> Result<(), RepositoryError> {
        let result = self.collection.delete_one(doc! { "_id": task }).await?;
        if result.deleted_count == 0 {
            return Err(RepositoryError::AiTaskRouteNotFound);
        }
        Ok(())
    }
}

fn from_document(document: Document) -> Result<AiTaskRoute, RepositoryError> {
    let task = match optional_text(&document, "task")?.as_deref() {
        Some(value) => AiTaskKind::parse(value).ok_or(RepositoryError::CorruptRecord("task"))?,
        None => AiTaskKind::parse(&required_text(&document, "_id")?)
            .ok_or(RepositoryError::CorruptRecord("task"))?,
    };
    let fallbacks = document
        .get_array("fallbacks")
        .map_err(|_| RepositoryError::CorruptRecord("fallbacks"))?
        .iter()
        .map(|value| match value {
            Bson::String(value) if !value.trim().is_empty() => Ok(value.clone()),
            _ => Err(RepositoryError::CorruptRecord("fallbacks")),
        })
        .collect::<Result<Vec<String>, RepositoryError>>()?;
    AiTaskRoute::from_stored(StoredAiTaskRoute {
        task,
        primary: required_text(&document, "primary")?,
        fallbacks,
        enabled: document
            .get("enabled")
            .and_then(Bson::as_bool)
            .unwrap_or(true),
        timeout_ms: document
            .get("timeoutMs")
            .and_then(Bson::as_i64)
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or(5_000),
        max_attempts: document
            .get("maxAttempts")
            .and_then(Bson::as_i64)
            .and_then(|value| u8::try_from(value).ok())
            .unwrap_or(1),
        concurrency: document
            .get("concurrency")
            .and_then(Bson::as_i64)
            .and_then(|value| u8::try_from(value).ok())
            .unwrap_or(1),
        max_output_tokens: optional_u32(&document, "maxOutputTokens")?,
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

fn required_timestamp(
    document: &Document,
    field: &'static str,
) -> Result<DateTime<Utc>, RepositoryError> {
    match document.get(field) {
        Some(Bson::DateTime(value)) => from_timestamp(*value),
        _ => Err(RepositoryError::CorruptRecord(field)),
    }
}
