use async_trait::async_trait;
use mongodb::bson::{Bson, Document, doc};
use mongodb::options::IndexOptions;
use mongodb::{Collection, Database, IndexModel};

use tm_trading_contracts::{DesiredStateDto, ObservedStateDto, ReadinessDto, ServiceSnapshot};

use crate::bson_support::{from_timestamp, timestamp};
use crate::{RepositoryError, ServiceSnapshotRepository};

/// Schema version of the persisted service snapshot described by
/// `docs/migration/data-contract-map.md` section 8.2.
pub const SERVICE_SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// Durable service read-model projection.
///
/// One document per service instance, keyed by the service instance identifier. The Agent owns
/// its own rows, so a heartbeat replaces that Agent's whole set rather than patching instances.
pub struct MongoServiceSnapshotRepository {
    collection: Collection<Document>,
}

impl MongoServiceSnapshotRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        let collection = database.collection::<Document>("service_instances");
        collection
            .create_indexes(vec![
                IndexModel::builder()
                    .keys(doc! { "agentId": 1 })
                    .options(IndexOptions::builder().name("agent".to_owned()).build())
                    .build(),
            ])
            .await?;
        Ok(Self { collection })
    }
}

#[async_trait]
impl ServiceSnapshotRepository for MongoServiceSnapshotRepository {
    async fn replace_agent_snapshots(
        &self,
        agent_id: &str,
        snapshots: Vec<ServiceSnapshot>,
    ) -> Result<(), RepositoryError> {
        if agent_id.trim().is_empty() {
            return Err(RepositoryError::EmptySnapshotField("agent_id"));
        }
        for snapshot in &snapshots {
            if snapshot.service_instance_id.trim().is_empty() {
                return Err(RepositoryError::EmptySnapshotField("service_instance_id"));
            }
            if snapshot.agent_id != agent_id {
                return Err(RepositoryError::SnapshotAgentMismatch);
            }
        }

        // Upsert before deleting stale rows: a crash between the two steps leaves an extra
        // row until the next heartbeat instead of losing the Agent's state.
        // Heartbeats from one Agent are serialized, so last-writer-wins per document is safe.
        for snapshot in &snapshots {
            self.collection
                .replace_one(
                    doc! { "_id": &snapshot.service_instance_id },
                    to_document(snapshot)?,
                )
                .upsert(true)
                .await?;
        }

        let owned: Vec<&str> = snapshots
            .iter()
            .map(|snapshot| snapshot.service_instance_id.as_str())
            .collect();
        self.collection
            .delete_many(doc! {
                "agentId": agent_id,
                "_id": { "$nin": owned },
            })
            .await?;
        Ok(())
    }

    async fn list(&self) -> Result<Vec<ServiceSnapshot>, RepositoryError> {
        let mut cursor = self
            .collection
            .find(doc! {})
            .sort(doc! { "_id": 1 })
            .await?;
        let mut snapshots = Vec::new();
        while cursor.advance().await? {
            snapshots.push(from_document(cursor.deserialize_current()?)?);
        }
        Ok(snapshots)
    }

    async fn get(
        &self,
        service_instance_id: &str,
    ) -> Result<Option<ServiceSnapshot>, RepositoryError> {
        self.collection
            .find_one(doc! { "_id": service_instance_id })
            .await?
            .map(from_document)
            .transpose()
    }
}

fn to_document(snapshot: &ServiceSnapshot) -> Result<Document, RepositoryError> {
    let mut document = doc! {
        "_id": &snapshot.service_instance_id,
        "agentId": &snapshot.agent_id,
        "serviceDefinitionId": &snapshot.service_definition_id,
        "desiredState": snapshot.desired_state.as_str(),
        "observedState": snapshot.observed_state.as_str(),
        "readiness": snapshot.readiness.as_str(),
        "restartCount": i64::from(snapshot.restart_count),
        "retryBlocked": snapshot.retry_blocked,
        "updatedAt": timestamp(snapshot.updated_at),
        "schemaVersion": i64::from(SERVICE_SNAPSHOT_SCHEMA_VERSION),
    };
    if let Some(value) = snapshot.process_alive {
        document.insert("processAlive", value);
    }
    if let Some(value) = snapshot.last_exit_code {
        document.insert("lastExitCode", i64::from(value));
    }
    if let Some(value) = snapshot.last_heartbeat_at {
        document.insert("lastHeartbeatAt", timestamp(value));
    }
    Ok(document)
}

fn from_document(document: Document) -> Result<ServiceSnapshot, RepositoryError> {
    let text = |field: &'static str| -> Result<String, RepositoryError> {
        document
            .get_str(field)
            .map(str::to_owned)
            .map_err(|_| RepositoryError::CorruptRecord(field))
    };
    let number = |field: &'static str| -> Result<i64, RepositoryError> {
        document
            .get_i64(field)
            .map_err(|_| RepositoryError::CorruptRecord(field))
    };

    let schema_version = u32::try_from(number("schemaVersion")?)
        .map_err(|_| RepositoryError::CorruptRecord("schemaVersion"))?;
    if schema_version != SERVICE_SNAPSHOT_SCHEMA_VERSION {
        return Err(RepositoryError::CorruptRecord("schemaVersion"));
    }

    Ok(ServiceSnapshot {
        service_instance_id: text("_id")?,
        service_definition_id: text("serviceDefinitionId")?,
        agent_id: text("agentId")?,
        desired_state: DesiredStateDto::parse(&text("desiredState")?)
            .ok_or(RepositoryError::CorruptRecord("desiredState"))?,
        observed_state: ObservedStateDto::parse(&text("observedState")?)
            .ok_or(RepositoryError::CorruptRecord("observedState"))?,
        readiness: ReadinessDto::parse(&text("readiness")?)
            .ok_or(RepositoryError::CorruptRecord("readiness"))?,
        process_alive: match document.get("processAlive") {
            None | Some(Bson::Null) => None,
            Some(Bson::Boolean(value)) => Some(*value),
            Some(_) => return Err(RepositoryError::CorruptRecord("processAlive")),
        },
        restart_count: u32::try_from(number("restartCount")?)
            .map_err(|_| RepositoryError::CorruptRecord("restartCount"))?,
        last_exit_code: match document.get("lastExitCode") {
            None | Some(Bson::Null) => None,
            Some(Bson::Int32(value)) => Some(*value),
            Some(Bson::Int64(value)) => Some(
                i32::try_from(*value)
                    .map_err(|_| RepositoryError::CorruptRecord("lastExitCode"))?,
            ),
            Some(_) => return Err(RepositoryError::CorruptRecord("lastExitCode")),
        },
        retry_blocked: document
            .get_bool("retryBlocked")
            .map_err(|_| RepositoryError::CorruptRecord("retryBlocked"))?,
        last_heartbeat_at: match document.get("lastHeartbeatAt") {
            None | Some(Bson::Null) => None,
            Some(Bson::DateTime(value)) => Some(from_timestamp(*value)?),
            Some(_) => return Err(RepositoryError::CorruptRecord("lastHeartbeatAt")),
        },
        updated_at: document
            .get_datetime("updatedAt")
            .map_err(|_| RepositoryError::CorruptRecord("updatedAt"))
            .and_then(|value| from_timestamp(*value))?,
    })
}
