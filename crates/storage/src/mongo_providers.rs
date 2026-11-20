use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mongodb::bson::{Bson, Document, doc, oid::ObjectId};
use mongodb::{Collection, Database};

use tm_trading_domain::{
    NewProviderInstance, ProviderHealthState, ProviderInstance, StoredProviderInstance,
};

use crate::bson_support::{from_timestamp, number_as_i64, timestamp};
use crate::{ProviderInstanceRepository, RepositoryError};

/// Stamp carried by every document this store writes.
pub const PROVIDER_INSTANCE_SCHEMA_VERSION: &str = "provider-instance.v1";

/// Durable `provider_instances` store.
///
/// This collection is new to the Rust service, so there is no legacy shape to preserve.
/// The document keeps configuration and observed health in separate fields.
#[derive(Clone)]
pub struct MongoProviderInstanceRepository {
    collection: Collection<Document>,
}

impl MongoProviderInstanceRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        Ok(Self {
            collection: database.collection::<Document>("provider_instances"),
        })
    }
}

#[async_trait]
impl ProviderInstanceRepository for MongoProviderInstanceRepository {
    async fn list(&self) -> Result<Vec<ProviderInstance>, RepositoryError> {
        let mut cursor = self
            .collection
            .find(doc! {})
            .sort(doc! { "createdAt": 1, "_id": 1 })
            .await?;
        let mut instances = Vec::new();
        while cursor.advance().await? {
            instances.push(from_document(cursor.deserialize_current()?)?);
        }
        Ok(instances)
    }

    async fn get(
        &self,
        provider_instance_id: &str,
    ) -> Result<Option<ProviderInstance>, RepositoryError> {
        let id = match ObjectId::parse_str(provider_instance_id) {
            Ok(id) => id,
            Err(_) => return Ok(None),
        };
        self.collection
            .find_one(doc! { "_id": id })
            .await?
            .map(from_document)
            .transpose()
    }

    async fn create(
        &self,
        provider_definition_id: &str,
        display_name: &str,
        base_url: &str,
        timeout_ms: u64,
        now: DateTime<Utc>,
    ) -> Result<ProviderInstance, RepositoryError> {
        let instance = ProviderInstance::new(NewProviderInstance {
            id: ObjectId::new().to_hex(),
            provider_definition_id: provider_definition_id.to_owned(),
            display_name: display_name.to_owned(),
            base_url: base_url.to_owned(),
            timeout_ms,
            created_at: now,
        })?;
        self.collection.insert_one(to_document(&instance)?).await?;
        Ok(instance)
    }

    async fn update(
        &self,
        instance: ProviderInstance,
    ) -> Result<ProviderInstance, RepositoryError> {
        let id = ObjectId::parse_str(instance.id())
            .map_err(|_| RepositoryError::ProviderInstanceNotFound)?;
        let result = self
            .collection
            .replace_one(doc! { "_id": id }, to_document(&instance)?)
            .await?;
        if result.matched_count == 0 {
            return Err(RepositoryError::ProviderInstanceNotFound);
        }
        Ok(instance)
    }

    async fn record_health(
        &self,
        provider_instance_id: &str,
        observed_at: DateTime<Utc>,
        healthy: bool,
        latency_ms: Option<u64>,
        error_code: Option<&str>,
    ) -> Result<ProviderInstance, RepositoryError> {
        let id = ObjectId::parse_str(provider_instance_id)
            .map_err(|_| RepositoryError::ProviderInstanceNotFound)?;
        let mut set = doc! {
            "healthState": if healthy { "HEALTHY" } else { "UNHEALTHY" },
            "lastHealthCheckAt": timestamp(observed_at),
            "lastLatencyMs": optional_count(latency_ms),
            "lastErrorCode": error_code.map_or(Bson::Null, |value| Bson::String(value.to_owned())),
        };
        if healthy {
            set.insert("lastSuccessAt", timestamp(observed_at));
        }
        let result = self
            .collection
            .update_one(doc! { "_id": id }, doc! { "$set": set })
            .await?;
        if result.matched_count == 0 {
            return Err(RepositoryError::ProviderInstanceNotFound);
        }
        self.collection
            .find_one(doc! { "_id": id })
            .await?
            .map(from_document)
            .transpose()?
            .ok_or(RepositoryError::ProviderInstanceNotFound)
    }

    async fn delete(&self, provider_instance_id: &str) -> Result<(), RepositoryError> {
        let id = ObjectId::parse_str(provider_instance_id)
            .map_err(|_| RepositoryError::ProviderInstanceNotFound)?;
        let result = self.collection.delete_one(doc! { "_id": id }).await?;
        if result.deleted_count == 0 {
            return Err(RepositoryError::ProviderInstanceNotFound);
        }
        Ok(())
    }
}

fn to_document(instance: &ProviderInstance) -> Result<Document, RepositoryError> {
    let id =
        ObjectId::parse_str(instance.id()).map_err(|_| RepositoryError::CorruptRecord("_id"))?;
    Ok(doc! {
        "_id": id,
        "schemaVersion": PROVIDER_INSTANCE_SCHEMA_VERSION,
        "providerDefinitionId": instance.provider_definition_id(),
        "displayName": instance.display_name(),
        "enabled": instance.enabled(),
        "baseUrl": instance.base_url(),
        "timeoutMs": i64::try_from(instance.timeout_ms())
            .map_err(|_| RepositoryError::CorruptRecord("timeoutMs"))?,
        "healthState": instance.health_state().as_str(),
        "lastHealthCheckAt": optional_timestamp(instance.last_health_check_at()),
        "lastSuccessAt": optional_timestamp(instance.last_success_at()),
        "lastLatencyMs": optional_count(instance.last_latency_ms()),
        "lastErrorCode": instance.last_error_code().map_or(Bson::Null, |value| Bson::String(value.to_owned())),
        "createdAt": timestamp(instance.created_at()),
        "updatedAt": timestamp(instance.updated_at()),
    })
}

fn from_document(document: Document) -> Result<ProviderInstance, RepositoryError> {
    let id = match document.get("_id") {
        Some(Bson::ObjectId(id)) => id.to_hex(),
        _ => return Err(RepositoryError::CorruptRecord("_id")),
    };
    let text = |field: &'static str| -> Result<String, RepositoryError> {
        document
            .get_str(field)
            .map(str::to_owned)
            .map_err(|_| RepositoryError::CorruptRecord(field))
    };
    let health_state = match document.get("healthState") {
        None | Some(Bson::Null) => ProviderHealthState::Unknown,
        Some(Bson::String(value)) => ProviderHealthState::parse(value)
            .ok_or(RepositoryError::CorruptRecord("healthState"))?,
        _ => return Err(RepositoryError::CorruptRecord("healthState")),
    };
    let timeout_ms = document
        .get("timeoutMs")
        .ok_or(RepositoryError::CorruptRecord("timeoutMs"))
        .and_then(|value| number_as_i64("timeoutMs", value))
        .and_then(|value| {
            u64::try_from(value).map_err(|_| RepositoryError::CorruptRecord("timeoutMs"))
        })?;

    ProviderInstance::from_stored(StoredProviderInstance {
        id,
        provider_definition_id: text("providerDefinitionId")?,
        display_name: text("displayName")?,
        enabled: document.get_bool("enabled").unwrap_or(true),
        base_url: text("baseUrl")?,
        timeout_ms,
        health_state,
        last_health_check_at: optional_datetime(&document, "lastHealthCheckAt")?,
        last_success_at: optional_datetime(&document, "lastSuccessAt")?,
        last_latency_ms: optional_count_from(&document, "lastLatencyMs")?,
        last_error_code: optional_text(&document, "lastErrorCode")?,
        created_at: required_datetime(&document, "createdAt")?,
        updated_at: required_datetime(&document, "updatedAt")?,
    })
    .map_err(RepositoryError::from)
}

fn optional_timestamp(value: Option<DateTime<Utc>>) -> Bson {
    value.map_or(Bson::Null, |value| Bson::DateTime(timestamp(value)))
}

fn optional_count(value: Option<u64>) -> Bson {
    value.map_or(Bson::Null, |value| {
        Bson::Int64(i64::try_from(value).unwrap_or(i64::MAX))
    })
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

fn required_datetime(
    document: &Document,
    field: &'static str,
) -> Result<DateTime<Utc>, RepositoryError> {
    optional_datetime(document, field)?.ok_or(RepositoryError::CorruptRecord(field))
}

fn optional_count_from(
    document: &Document,
    field: &'static str,
) -> Result<Option<u64>, RepositoryError> {
    match document.get(field) {
        None | Some(Bson::Null) => Ok(None),
        Some(value) => number_as_i64(field, value)
            .and_then(|value| {
                u64::try_from(value).map_err(|_| RepositoryError::CorruptRecord(field))
            })
            .map(Some),
    }
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
