use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mongodb::bson::{Bson, Document, doc};
use mongodb::{Collection, Database, IndexModel};

use tm_trading_domain::{
    NEWS_SOURCE_SCHEMA_VERSION, NewNewsSource, NewsHealthState, NewsLicenseStatus, NewsRights,
    NewsSource, NewsSourceHealth, NewsTransport, NewsTrustTier, StoredNewsSource,
    sanitize_news_fetch_error,
};

use crate::bson_support::{from_timestamp, number_as_i64, timestamp};
use crate::{NewsSourceRepository, RepositoryError};

/// Durable `news_sources` store compatible with the legacy collection.
#[derive(Clone)]
pub struct MongoNewsSourceRepository {
    collection: Collection<Document>,
}

impl MongoNewsSourceRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        let collection = database.collection::<Document>("news_sources");
        for keys in [
            doc! { "sourceId": 1 },
            doc! { "trustTier": 1 },
            doc! { "licenseStatus": 1 },
            doc! { "enabled": 1 },
        ] {
            collection
                .create_index(IndexModel::builder().keys(keys).build())
                .await?;
        }
        Ok(Self { collection })
    }
}

#[async_trait]
impl NewsSourceRepository for MongoNewsSourceRepository {
    async fn upsert_seed(&self, source: NewNewsSource) -> Result<NewsSource, RepositoryError> {
        let source = NewsSource::new(source)?;
        let insert = doc! {
            "sourceId": source.source_id(),
            "name": source.name(),
            "provider": optional_string(source.provider()),
            "transport": source.transport().as_str(),
            "feedUrl": optional_string(source.feed_url()),
            "trustTier": source.trust_tier().as_str(),
            "marketGroups": source.market_groups().to_vec(),
            "languages": source.languages().to_vec(),
            "licenseStatus": source.license_status().as_str(),
            "rights": rights_document(source.rights()),
            "enabled": source.enabled(),
            "pollIntervalSec": i64::from(source.poll_interval_sec()),
            "retentionDays": i64::from(source.retention_days()),
            "retryRequestedAt": Bson::Null,
            "health": health_document(NewsSourceHealth::default()),
            "schemaVersion": NEWS_SOURCE_SCHEMA_VERSION,
            "createdAt": timestamp(source.created_at()),
        };
        self.collection
            .update_one(
                doc! { "_id": source.source_id() },
                doc! {
                    "$setOnInsert": insert,
                    "$max": { "updatedAt": timestamp(source.created_at()) },
                },
            )
            .upsert(true)
            .await?;
        self.get(source.source_id())
            .await?
            .ok_or(RepositoryError::NewsSourceNotFound)
    }

    async fn list(&self) -> Result<Vec<NewsSource>, RepositoryError> {
        let mut cursor = self
            .collection
            .find(doc! {})
            .sort(doc! { "sourceId": 1 })
            .await?;
        let mut sources = Vec::new();
        while cursor.advance().await? {
            sources.push(from_document(cursor.deserialize_current()?)?);
        }
        Ok(sources)
    }

    async fn get(&self, source_id: &str) -> Result<Option<NewsSource>, RepositoryError> {
        self.collection
            .find_one(doc! { "_id": source_id })
            .await?
            .map(from_document)
            .transpose()
    }

    async fn set_enabled(
        &self,
        source_id: &str,
        enabled: bool,
        updated_at: DateTime<Utc>,
    ) -> Result<NewsSource, RepositoryError> {
        let result = self
            .collection
            .update_one(
                doc! { "_id": source_id },
                doc! { "$set": {
                    "enabled": enabled,
                    "updatedAt": timestamp(updated_at),
                } },
            )
            .await?;
        if result.matched_count == 0 {
            return Err(RepositoryError::NewsSourceNotFound);
        }
        self.get(source_id)
            .await?
            .ok_or(RepositoryError::NewsSourceNotFound)
    }

    async fn record_health(
        &self,
        source_id: &str,
        observed_at: DateTime<Utc>,
        item_count: u32,
        error: Option<&str>,
    ) -> Result<NewsSource, RepositoryError> {
        let last_error = error.map(sanitize_news_fetch_error);
        let mut set = doc! {
            "health.state": if error.is_some() { "down" } else if item_count > 0 { "ok" } else { "degraded" },
            "health.lastFetchAt": timestamp(observed_at),
            "health.lastError": last_error
                .as_ref()
                .map_or(Bson::Null, |value| Bson::String(value.clone())),
            "updatedAt": timestamp(observed_at),
        };
        let mut update = doc! { "$set": set.clone() };
        if error.is_some() {
            update.insert("$inc", doc! { "health.consecutiveErrors": 1 });
        } else {
            set.insert("health.lastOkAt", timestamp(observed_at));
            set.insert("health.consecutiveErrors", i64::from(0));
            if item_count > 0 {
                set.insert("health.lastItemAt", timestamp(observed_at));
            }
            set.insert("retryRequestedAt", Bson::Null);
            update = doc! { "$set": set };
        }
        let result = self
            .collection
            .update_one(doc! { "_id": source_id }, update)
            .await?;
        if result.matched_count == 0 {
            return Err(RepositoryError::NewsSourceNotFound);
        }
        self.get(source_id)
            .await?
            .ok_or(RepositoryError::NewsSourceNotFound)
    }

    async fn request_retry(
        &self,
        source_id: &str,
        requested_at: DateTime<Utc>,
    ) -> Result<NewsSource, RepositoryError> {
        let result = self
            .collection
            .update_one(
                doc! { "_id": source_id },
                doc! { "$set": {
                    "retryRequestedAt": timestamp(requested_at),
                    "updatedAt": timestamp(requested_at),
                } },
            )
            .await?;
        if result.matched_count == 0 {
            return Err(RepositoryError::NewsSourceNotFound);
        }
        self.get(source_id)
            .await?
            .ok_or(RepositoryError::NewsSourceNotFound)
    }
}

fn rights_document(rights: NewsRights) -> Document {
    doc! {
        "allowedStoreMetadata": rights.allowed_store_metadata,
        "allowedDisplay": rights.allowed_display,
        "allowedSummarize": rights.allowed_summarize,
        "allowedTraining": rights.allowed_training,
    }
}

fn health_document(health: NewsSourceHealth) -> Document {
    doc! {
        "state": health.state.as_str(),
        "lastFetchAt": optional_timestamp(health.last_fetch_at),
        "lastOkAt": optional_timestamp(health.last_ok_at),
        "lastItemAt": optional_timestamp(health.last_item_at),
        "lastError": optional_string(health.last_error.as_deref()),
        "consecutiveErrors": i64::from(health.consecutive_errors),
    }
}

fn optional_string(value: Option<&str>) -> Bson {
    value.map_or(Bson::Null, |value| Bson::String(value.to_owned()))
}

fn optional_timestamp(value: Option<DateTime<Utc>>) -> Bson {
    value.map_or(Bson::Null, |value| Bson::DateTime(timestamp(value)))
}

fn from_document(document: Document) -> Result<NewsSource, RepositoryError> {
    let source_id = match document.get("_id") {
        Some(Bson::String(value)) => value.clone(),
        _ => return Err(RepositoryError::CorruptRecord("_id")),
    };
    let stored_source_id = required_text(&document, "sourceId")?;
    if stored_source_id != source_id {
        return Err(RepositoryError::CorruptRecord("sourceId"));
    }
    let transport = NewsTransport::parse(&required_text(&document, "transport")?)
        .ok_or(RepositoryError::CorruptRecord("transport"))?;
    let trust_tier = NewsTrustTier::parse(&required_text(&document, "trustTier")?)
        .ok_or(RepositoryError::CorruptRecord("trustTier"))?;
    let license_status = NewsLicenseStatus::parse(&required_text(&document, "licenseStatus")?)
        .ok_or(RepositoryError::CorruptRecord("licenseStatus"))?;
    let rights = rights_from_document(document.get("rights"))?;
    let health = health_from_document(document.get("health"))?;
    let poll_interval_sec = optional_count(&document, "pollIntervalSec", 600)?;
    let retention_days = optional_count(&document, "retentionDays", 7)?;

    NewsSource::from_stored(StoredNewsSource {
        source_id,
        name: required_text(&document, "name")?,
        provider: optional_text(&document, "provider")?,
        transport,
        feed_url: optional_text(&document, "feedUrl")?,
        trust_tier,
        market_groups: string_array_or_default(&document, "marketGroups", "crypto")?,
        languages: string_array_or_default(&document, "languages", "en")?,
        license_status,
        rights,
        enabled: enabled_from_document(&document),
        poll_interval_sec,
        retention_days,
        retry_requested_at: optional_datetime(&document, "retryRequestedAt")?,
        health,
        created_at: required_datetime(&document, "createdAt")?,
        updated_at: required_datetime(&document, "updatedAt")?,
    })
    .map_err(RepositoryError::from)
}

fn enabled_from_document(document: &Document) -> bool {
    match document.get("enabled") {
        None | Some(Bson::Null) => true,
        Some(Bson::Boolean(value)) => *value,
        Some(Bson::String(value)) => matches!(value.to_ascii_lowercase().as_str(), "true" | "1"),
        Some(Bson::Int32(value)) => *value == 1,
        Some(Bson::Int64(value)) => *value == 1,
        Some(Bson::Double(value)) => *value == 1.0,
        Some(_) => false,
    }
}

fn rights_from_document(value: Option<&Bson>) -> Result<NewsRights, RepositoryError> {
    let Some(Bson::Document(document)) = value else {
        return Ok(NewsRights {
            allowed_store_metadata: false,
            allowed_display: false,
            allowed_summarize: false,
            allowed_training: false,
        });
    };
    Ok(NewsRights {
        allowed_store_metadata: document.get_bool("allowedStoreMetadata").unwrap_or(false),
        allowed_display: document.get_bool("allowedDisplay").unwrap_or(false),
        allowed_summarize: document.get_bool("allowedSummarize").unwrap_or(false),
        allowed_training: document.get_bool("allowedTraining").unwrap_or(false),
    })
}

fn health_from_document(value: Option<&Bson>) -> Result<NewsSourceHealth, RepositoryError> {
    let Some(Bson::Document(document)) = value else {
        return Ok(NewsSourceHealth::default());
    };
    let state = match document.get("state") {
        None | Some(Bson::Null) => NewsHealthState::Unknown,
        Some(Bson::String(value)) => {
            NewsHealthState::parse(value).ok_or(RepositoryError::CorruptRecord("health.state"))?
        }
        _ => return Err(RepositoryError::CorruptRecord("health.state")),
    };
    Ok(NewsSourceHealth {
        state,
        last_fetch_at: optional_nested_datetime(document, "lastFetchAt")?,
        last_ok_at: optional_nested_datetime(document, "lastOkAt")?,
        last_item_at: optional_nested_datetime(document, "lastItemAt")?,
        last_error: optional_nested_text(document, "lastError")?
            .as_deref()
            .map(sanitize_news_fetch_error),
        consecutive_errors: optional_nested_count(document, "consecutiveErrors", 0)?,
    })
}

fn required_text(document: &Document, field: &'static str) -> Result<String, RepositoryError> {
    document
        .get_str(field)
        .map(str::to_owned)
        .map_err(|_| RepositoryError::CorruptRecord(field))
}

fn optional_text(
    document: &Document,
    field: &'static str,
) -> Result<Option<String>, RepositoryError> {
    match document.get(field) {
        None | Some(Bson::Null) => Ok(None),
        Some(Bson::String(value)) => Ok(Some(value.clone())),
        _ => Err(RepositoryError::CorruptRecord(field)),
    }
}

fn string_array_or_default(
    document: &Document,
    field: &'static str,
    default: &str,
) -> Result<Vec<String>, RepositoryError> {
    let Some(values) = document.get(field) else {
        return Ok(vec![default.to_owned()]);
    };
    if matches!(values, Bson::Null) {
        return Ok(vec![default.to_owned()]);
    }
    let values = values
        .as_array()
        .ok_or(RepositoryError::CorruptRecord(field))?;
    values
        .iter()
        .map(|value| match value {
            Bson::String(value) => Ok(value.clone()),
            _ => Err(RepositoryError::CorruptRecord(field)),
        })
        .collect()
}

fn optional_count(
    document: &Document,
    field: &'static str,
    default: u32,
) -> Result<u32, RepositoryError> {
    match document.get(field) {
        None | Some(Bson::Null) => Ok(default),
        Some(value) => {
            let value = number_as_i64(field, value)?;
            u32::try_from(value).map_err(|_| RepositoryError::CorruptRecord(field))
        }
    }
}

fn required_datetime(
    document: &Document,
    field: &'static str,
) -> Result<DateTime<Utc>, RepositoryError> {
    optional_datetime(document, field)?.ok_or(RepositoryError::CorruptRecord(field))
}

fn optional_datetime(
    document: &Document,
    field: &'static str,
) -> Result<Option<DateTime<Utc>>, RepositoryError> {
    match document.get(field) {
        None | Some(Bson::Null) => Ok(None),
        Some(Bson::DateTime(value)) => from_timestamp(*value).map(Some),
        _ => Err(RepositoryError::CorruptRecord(field)),
    }
}

fn optional_nested_datetime(
    document: &Document,
    field: &'static str,
) -> Result<Option<DateTime<Utc>>, RepositoryError> {
    match document.get(field) {
        None | Some(Bson::Null) => Ok(None),
        Some(Bson::DateTime(value)) => from_timestamp(*value).map(Some),
        _ => Err(RepositoryError::CorruptRecord("health")),
    }
}

fn optional_nested_text(
    document: &Document,
    field: &'static str,
) -> Result<Option<String>, RepositoryError> {
    match document.get(field) {
        None | Some(Bson::Null) => Ok(None),
        Some(Bson::String(value)) => Ok(Some(value.clone())),
        _ => Err(RepositoryError::CorruptRecord("health")),
    }
}

fn optional_nested_count(
    document: &Document,
    field: &'static str,
    default: u32,
) -> Result<u32, RepositoryError> {
    match document.get(field) {
        None | Some(Bson::Null) => Ok(default),
        Some(value) => {
            let value = number_as_i64(field, value)?;
            u32::try_from(value).map_err(|_| RepositoryError::CorruptRecord("health"))
        }
    }
}
