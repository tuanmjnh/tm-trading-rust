use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mongodb::bson::{Bson, Document, doc};
use mongodb::options::IndexOptions;
use mongodb::{Collection, Database, IndexModel};

use tm_trading_domain::{
    AiProviderHealthState, AiProviderKind, AiProviderProfile, DEFAULT_AI_PROVIDER_API_KEY_ENV,
    StoredAiProviderProfile,
};

use crate::bson_support::{from_timestamp, timestamp};
use crate::{AiProviderProfileRepository, RepositoryError};

pub struct MongoAiProviderProfileRepository {
    collection: Collection<Document>,
}

impl MongoAiProviderProfileRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        let collection = database.collection::<Document>("ai_provider_profiles");
        let index = IndexModel::builder()
            .keys(doc! { "updatedAt": -1 })
            .options(IndexOptions::builder().name("updatedAt".to_owned()).build())
            .build();
        collection.create_indexes(vec![index]).await?;
        Ok(Self { collection })
    }
}

#[async_trait]
impl AiProviderProfileRepository for MongoAiProviderProfileRepository {
    async fn list(&self) -> Result<Vec<AiProviderProfile>, RepositoryError> {
        let mut cursor = self
            .collection
            .find(Document::new())
            .sort(doc! { "updatedAt": -1 })
            .await?;
        let mut profiles = Vec::new();
        while cursor.advance().await? {
            profiles.push(from_document(cursor.deserialize_current()?)?);
        }
        Ok(profiles)
    }

    async fn get(&self, id: &str) -> Result<Option<AiProviderProfile>, RepositoryError> {
        self.collection
            .find_one(doc! { "_id": id })
            .await?
            .map(from_document)
            .transpose()
    }

    async fn upsert(&self, profile: &AiProviderProfile) -> Result<(), RepositoryError> {
        let filter = doc! { "_id": profile.id() };
        let document = doc! {
            "_id": profile.id(),
            "kind": profile.kind().as_str(),
            "displayName": profile.display_name(),
            "enabled": profile.enabled(),
            "baseUrl": profile.base_url(),
            "defaultModel": profile.default_model(),
            "timeoutMs": i64::try_from(profile.timeout_ms()).unwrap_or(0),
            "apiKeyEnv": profile.api_key_env(),
            "healthState": profile.health_state().as_str(),
            "lastHealthCheckAt": profile.last_health_check_at().map(timestamp),
            "lastErrorCode": profile.last_error_code(),
            "createdAt": timestamp(profile.created_at()),
            "updatedAt": timestamp(profile.updated_at()),
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
            return Err(RepositoryError::AiProviderProfileNotFound);
        }
        Ok(())
    }
}

fn from_document(document: Document) -> Result<AiProviderProfile, RepositoryError> {
    let id = required_text(&document, "_id")?;
    let kind = match optional_text(&document, "kind")?.as_deref() {
        Some(value) => {
            AiProviderKind::parse(value).ok_or(RepositoryError::CorruptRecord("kind"))?
        }
        None => AiProviderKind::OpenAiCompatible,
    };
    let display_name = required_text(&document, "displayName")?;
    let enabled = document
        .get("enabled")
        .and_then(Bson::as_bool)
        .unwrap_or(true);
    let base_url = required_text(&document, "baseUrl")?;
    let default_model = required_text(&document, "defaultModel")?;
    let timeout_ms = document
        .get("timeoutMs")
        .and_then(Bson::as_i64)
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or(5000);
    let api_key_env = optional_text(&document, "apiKeyEnv")?
        .unwrap_or_else(|| DEFAULT_AI_PROVIDER_API_KEY_ENV.to_owned());
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
    let last_error_code = optional_text(&document, "lastErrorCode")?;
    let created_at = required_timestamp(&document, "createdAt")?;
    let updated_at = required_timestamp(&document, "updatedAt")?;
    AiProviderProfile::from_stored(StoredAiProviderProfile {
        id,
        kind,
        display_name,
        enabled,
        base_url,
        default_model,
        timeout_ms,
        api_key_env,
        health_state,
        last_health_check_at,
        last_error_code,
        created_at,
        updated_at,
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

fn required_timestamp(
    document: &Document,
    field: &'static str,
) -> Result<DateTime<Utc>, RepositoryError> {
    match document.get(field) {
        Some(Bson::DateTime(value)) => from_timestamp(*value),
        _ => Err(RepositoryError::CorruptRecord(field)),
    }
}
