use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use futures_util::TryStreamExt;
use mongodb::bson::{Bson, DateTime as BsonDateTime, Document, doc};
use mongodb::options::IndexOptions;
use mongodb::{Collection, Database, IndexModel};

use tm_trading_domain::{
    Operation, OperationAction, OperationError, OperationId, OperationStatus, PersistedOperation,
};

use crate::{CreateOperationOutcome, OperationRepository, RepositoryError};

/// Durable operation store described by `docs/migration/data-contract-map.md` section 8.1.
///
/// One document is both the durable operation record and the command the Agent claims.
pub struct MongoOperationRepository {
    collection: Collection<Document>,
}

impl MongoOperationRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        let collection = database.collection::<Document>("operations");
        collection.create_indexes(indexes()).await?;
        Ok(Self { collection })
    }

    /// Replaces a document only if its fencing token still matches the value we read.
    ///
    /// The fencing token is the optimistic concurrency version: every claim increments it,
    /// so a second writer that read the same document cannot overwrite a newer state.
    async fn replace_if_unchanged(
        &self,
        operation: &Operation,
        previous_fencing_token: u64,
    ) -> Result<bool, RepositoryError> {
        let mut document = to_document(operation)?;
        document.remove("_id");
        let result = self
            .collection
            .replace_one(
                doc! {
                    "_id": operation.id().to_string(),
                    "fencingToken": bson_i64(previous_fencing_token)?,
                },
                document,
            )
            .await?;
        Ok(result.matched_count == 1)
    }

    /// Reads, transitions, and compare-and-swaps one operation.
    async fn transition<F>(
        &self,
        operation_id: &OperationId,
        mut transition: F,
    ) -> Result<Operation, RepositoryError>
    where
        F: FnMut(&mut Operation) -> Result<(), OperationError>,
    {
        for _ in 0..CAS_ATTEMPTS {
            let document = self
                .collection
                .find_one(doc! { "_id": operation_id.to_string() })
                .await?
                .ok_or(RepositoryError::OperationNotFound)?;
            let mut operation = from_document(document)?;
            let previous_fencing_token = operation.fencing_token();
            transition(&mut operation)?;
            if self
                .replace_if_unchanged(&operation, previous_fencing_token)
                .await?
            {
                return Ok(operation);
            }
        }
        Err(RepositoryError::ConcurrentModification)
    }
}

#[async_trait]
impl OperationRepository for MongoOperationRepository {
    async fn create(
        &self,
        operation: Operation,
    ) -> Result<CreateOperationOutcome, RepositoryError> {
        match self.collection.insert_one(to_document(&operation)?).await {
            Ok(_) => Ok(CreateOperationOutcome::Created(operation)),
            Err(error) if is_duplicate_key(&error) => {
                let existing = self
                    .collection
                    .find_one(doc! {
                        "requestedBy": operation.requested_by(),
                        "idempotencyKey": operation.idempotency_key(),
                    })
                    .await?
                    .ok_or(RepositoryError::OperationNotFound)?;
                let existing = from_document(existing)?;
                if existing.request_hash() == operation.request_hash() {
                    Ok(CreateOperationOutcome::Existing(existing))
                } else {
                    Err(RepositoryError::IdempotencyConflict {
                        requested_by: operation.requested_by().to_owned(),
                        idempotency_key: operation.idempotency_key().to_owned(),
                    })
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn get(&self, operation_id: &OperationId) -> Result<Option<Operation>, RepositoryError> {
        self.collection
            .find_one(doc! { "_id": operation_id.to_string() })
            .await?
            .map(from_document)
            .transpose()
    }

    async fn claim_next(
        &self,
        agent_id: &str,
        now: DateTime<Utc>,
        lease_duration: Duration,
    ) -> Result<Option<Operation>, RepositoryError> {
        if agent_id.trim().is_empty() {
            return Err(RepositoryError::Domain(OperationError::EmptyField(
                "agent_id",
            )));
        }
        if lease_duration.is_zero() {
            return Err(RepositoryError::InvalidLeaseDuration);
        }
        let lease_duration = chrono::Duration::from_std(lease_duration)
            .map_err(|_| RepositoryError::LeaseDurationOutOfRange)?;
        let lease_expires_at = now
            .checked_add_signed(lease_duration)
            .ok_or(RepositoryError::LeaseDurationOutOfRange)?;

        self.expire_due(now).await?;

        // Each loser of the compare-and-swap retries against the next candidate instead of
        // reporting a conflict, because another Agent winning the race is not an error.
        for _ in 0..CAS_ATTEMPTS {
            let candidate = self
                .collection
                .find_one(doc! {
                    "agentId": agent_id,
                    "status": { "$nin": terminal_statuses() },
                    "expiresAt": { "$gt": bson_timestamp(now)? },
                    "$or": [
                        { "leaseExpiresAt": Bson::Null },
                        { "leaseExpiresAt": { "$lte": bson_timestamp(now)? } },
                    ],
                })
                .sort(doc! { "requestedAt": 1 })
                .await?;
            let Some(document) = candidate else {
                return Ok(None);
            };
            let mut operation = from_document(document)?;
            let previous_fencing_token = operation.fencing_token();
            if operation.claim(agent_id, now, lease_expires_at).is_err() {
                continue;
            }
            if self
                .replace_if_unchanged(&operation, previous_fencing_token)
                .await?
            {
                return Ok(Some(operation));
            }
        }
        Ok(None)
    }

    async fn mark_running(
        &self,
        operation_id: &OperationId,
        agent_id: &str,
        fencing_token: u64,
        now: DateTime<Utc>,
    ) -> Result<Operation, RepositoryError> {
        self.transition(operation_id, |operation| {
            operation.mark_running(agent_id, fencing_token, now)
        })
        .await
    }

    async fn mark_succeeded(
        &self,
        operation_id: &OperationId,
        agent_id: &str,
        fencing_token: u64,
        result_summary: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<Operation, RepositoryError> {
        self.transition(operation_id, |operation| {
            operation.mark_succeeded(agent_id, fencing_token, result_summary.clone(), now)
        })
        .await
    }

    async fn mark_failed(
        &self,
        operation_id: &OperationId,
        agent_id: &str,
        fencing_token: u64,
        error_code: Option<String>,
        redacted_error: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<Operation, RepositoryError> {
        self.transition(operation_id, |operation| {
            operation.mark_failed(
                agent_id,
                fencing_token,
                error_code.clone(),
                redacted_error.clone(),
                now,
            )
        })
        .await
    }

    async fn expire_due(&self, now: DateTime<Utc>) -> Result<usize, RepositoryError> {
        let mut cursor = self
            .collection
            .find(doc! {
                "status": { "$nin": terminal_statuses() },
                "expiresAt": { "$lte": bson_timestamp(now)? },
            })
            .await?;
        let mut expired = 0;
        while let Some(document) = cursor.try_next().await? {
            let mut operation = from_document(document)?;
            let previous_fencing_token = operation.fencing_token();
            if operation.mark_expired(now).is_err() {
                continue;
            }
            if self
                .replace_if_unchanged(&operation, previous_fencing_token)
                .await?
            {
                expired += 1;
            }
        }
        Ok(expired)
    }
}

/// Compare-and-swap attempts before a concurrent writer is reported as a conflict.
const CAS_ATTEMPTS: usize = 3;

fn terminal_statuses() -> Vec<&'static str> {
    [
        OperationStatus::Succeeded,
        OperationStatus::Failed,
        OperationStatus::Cancelled,
        OperationStatus::Expired,
    ]
    .into_iter()
    .map(OperationStatus::as_str)
    .collect()
}

fn indexes() -> Vec<IndexModel> {
    let named = |name: &str, unique: bool| {
        IndexOptions::builder()
            .name(name.to_owned())
            .unique(unique)
            .build()
    };
    vec![
        IndexModel::builder()
            .keys(doc! { "requestedBy": 1, "idempotencyKey": 1 })
            .options(named("uniq_request_idempotency", true))
            .build(),
        IndexModel::builder()
            .keys(doc! { "status": 1, "agentId": 1, "expiresAt": 1 })
            .options(named("status_agent_expiry", false))
            .build(),
        IndexModel::builder()
            .keys(doc! { "agentId": 1, "leaseExpiresAt": 1, "status": 1 })
            .options(named("agent_lease_status", false))
            .build(),
        IndexModel::builder()
            .keys(doc! { "serviceInstanceId": 1, "requestedAt": 1 })
            .options(named("service_requested", false))
            .build(),
        IndexModel::builder()
            .keys(doc! { "correlationId": 1 })
            .options(named("correlation", false))
            .build(),
    ]
}

fn is_duplicate_key(error: &mongodb::error::Error) -> bool {
    matches!(
        *error.kind,
        mongodb::error::ErrorKind::Write(mongodb::error::WriteFailure::WriteError(
            mongodb::error::WriteError { code: 11000, .. }
        ))
    )
}

fn bson_timestamp(value: DateTime<Utc>) -> Result<BsonDateTime, RepositoryError> {
    Ok(BsonDateTime::from_millis(value.timestamp_millis()))
}

fn bson_i64(value: u64) -> Result<i64, RepositoryError> {
    i64::try_from(value).map_err(|_| RepositoryError::CorruptRecord("fencing token overflow"))
}

fn from_bson_timestamp(value: BsonDateTime) -> Result<DateTime<Utc>, RepositoryError> {
    Utc.timestamp_millis_opt(value.timestamp_millis())
        .single()
        .ok_or(RepositoryError::CorruptRecord("invalid timestamp"))
}

fn to_document(operation: &Operation) -> Result<Document, RepositoryError> {
    let mut document = doc! {
        "_id": operation.id().to_string(),
        "schemaVersion": i64::from(operation.schema_version()),
        "idempotencyKey": operation.idempotency_key(),
        "requestHash": operation.request_hash(),
        "requestedBy": operation.requested_by(),
        "requestedAt": bson_timestamp(operation.requested_at())?,
        "agentId": operation.target_agent_id(),
        "serviceInstanceId": operation.service_instance_id(),
        "serviceDefinitionId": operation.service_definition_id(),
        "action": operation.action().as_str(),
        "status": operation.status().as_str(),
        "attempt": i64::from(operation.attempt()),
        "fencingToken": bson_i64(operation.fencing_token())?,
        "expiresAt": bson_timestamp(operation.expires_at())?,
        "correlationId": operation.correlation_id().to_string(),
    };
    for (field, value) in [
        ("leaseOwner", operation.lease_owner()),
        ("resultSummary", operation.result_summary()),
        ("redactedError", operation.redacted_error()),
        ("errorCode", operation.error_code()),
    ] {
        if let Some(value) = value {
            document.insert(field, value);
        }
    }
    if let Some(value) = operation.lease_expires_at() {
        document.insert("leaseExpiresAt", bson_timestamp(value)?);
    }
    if let Some(value) = operation.started_at() {
        document.insert("startedAt", bson_timestamp(value)?);
    }
    if let Some(value) = operation.finished_at() {
        document.insert("finishedAt", bson_timestamp(value)?);
    }
    Ok(document)
}

fn from_document(document: Document) -> Result<Operation, RepositoryError> {
    let text = |field: &'static str| -> Result<String, RepositoryError> {
        document
            .get_str(field)
            .map(str::to_owned)
            .map_err(|_| RepositoryError::CorruptRecord(field))
    };
    let optional_text = |field: &'static str| -> Result<Option<String>, RepositoryError> {
        match document.get(field) {
            None | Some(Bson::Null) => Ok(None),
            Some(Bson::String(value)) => Ok(Some(value.clone())),
            Some(_) => Err(RepositoryError::CorruptRecord(field)),
        }
    };
    let timestamp = |field: &'static str| -> Result<DateTime<Utc>, RepositoryError> {
        document
            .get_datetime(field)
            .map_err(|_| RepositoryError::CorruptRecord(field))
            .and_then(|value| from_bson_timestamp(*value))
    };
    let optional_timestamp =
        |field: &'static str| -> Result<Option<DateTime<Utc>>, RepositoryError> {
            match document.get(field) {
                None | Some(Bson::Null) => Ok(None),
                Some(Bson::DateTime(value)) => from_bson_timestamp(*value).map(Some),
                Some(_) => Err(RepositoryError::CorruptRecord(field)),
            }
        };
    let number = |field: &'static str| -> Result<i64, RepositoryError> {
        document
            .get_i64(field)
            .map_err(|_| RepositoryError::CorruptRecord(field))
    };

    Operation::restore(PersistedOperation {
        id: OperationId::from_uuid(
            uuid::Uuid::parse_str(&text("_id")?)
                .map_err(|_| RepositoryError::CorruptRecord("_id"))?,
        ),
        schema_version: u32::try_from(number("schemaVersion")?)
            .map_err(|_| RepositoryError::CorruptRecord("schemaVersion"))?,
        idempotency_key: text("idempotencyKey")?,
        request_hash: text("requestHash")?,
        requested_by: text("requestedBy")?,
        requested_at: timestamp("requestedAt")?,
        target_agent_id: text("agentId")?,
        service_instance_id: text("serviceInstanceId")?,
        service_definition_id: text("serviceDefinitionId")?,
        action: OperationAction::parse(&text("action")?)
            .ok_or(RepositoryError::CorruptRecord("action"))?,
        status: OperationStatus::parse(&text("status")?)
            .ok_or(RepositoryError::CorruptRecord("status"))?,
        attempt: u32::try_from(number("attempt")?)
            .map_err(|_| RepositoryError::CorruptRecord("attempt"))?,
        lease_owner: optional_text("leaseOwner")?,
        lease_expires_at: optional_timestamp("leaseExpiresAt")?,
        fencing_token: u64::try_from(number("fencingToken")?)
            .map_err(|_| RepositoryError::CorruptRecord("fencingToken"))?,
        started_at: optional_timestamp("startedAt")?,
        finished_at: optional_timestamp("finishedAt")?,
        expires_at: timestamp("expiresAt")?,
        result_summary: optional_text("resultSummary")?,
        redacted_error: optional_text("redactedError")?,
        error_code: optional_text("errorCode")?,
        correlation_id: uuid::Uuid::parse_str(&text("correlationId")?)
            .map_err(|_| RepositoryError::CorruptRecord("correlationId"))?,
    })
    .map_err(RepositoryError::Domain)
}
