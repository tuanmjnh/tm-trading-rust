use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::TryStreamExt;
use mongodb::bson::{Bson, Document, doc};
use mongodb::options::IndexOptions;
use mongodb::{Collection, Database, IndexModel};

use tm_trading_domain::{AiJob, AiJobError, AiJobId, AiJobStatus, AiTaskKind, PersistedAiJob};

use crate::bson_support::{from_timestamp, number_as_i64, timestamp};
use crate::{AiJobQuery, AiJobRepository, CreateAiJobOutcome, RepositoryError};

/// Durable `ai_jobs` store.
pub struct MongoAiJobRepository {
    collection: Collection<Document>,
}

impl MongoAiJobRepository {
    pub async fn new(database: &Database) -> Result<Self, RepositoryError> {
        let collection = database.collection::<Document>("ai_jobs");
        let named = |name: &str, unique: bool| {
            IndexOptions::builder()
                .name(name.to_owned())
                .unique(unique)
                .build()
        };
        collection
            .create_indexes(vec![
                IndexModel::builder()
                    .keys(doc! { "ownerScope": 1, "idempotencyKey": 1 })
                    .options(named("uniq_ai_job_idempotency", true))
                    .build(),
                IndexModel::builder()
                    .keys(doc! { "status": 1, "expiresAt": 1 })
                    .options(named("ai_job_status_expiry", false))
                    .build(),
                IndexModel::builder()
                    .keys(doc! { "ownerScope": 1, "requestedAt": -1 })
                    .options(named("ai_job_owner_requested", false))
                    .build(),
            ])
            .await?;
        Ok(Self { collection })
    }

    /// Replaces a document only if its fencing token still matches the value we read.
    async fn replace_if_unchanged(
        &self,
        job: &AiJob,
        previous_fencing_token: u64,
    ) -> Result<bool, RepositoryError> {
        let mut document = to_document(job)?;
        document.remove("_id");
        let result = self
            .collection
            .replace_one(
                doc! {
                    "_id": job.id().to_string(),
                    "fencingToken": bson_i64(previous_fencing_token)?,
                },
                document,
            )
            .await?;
        Ok(result.matched_count == 1)
    }

    /// Reads, transitions, and compare-and-swaps one job.
    async fn transition<F>(
        &self,
        job_id: &AiJobId,
        mut transition: F,
    ) -> Result<AiJob, RepositoryError>
    where
        F: FnMut(&mut AiJob) -> Result<(), AiJobError>,
    {
        for _ in 0..CAS_ATTEMPTS {
            let document = self
                .collection
                .find_one(doc! { "_id": job_id.to_string() })
                .await?
                .ok_or(RepositoryError::AiJobNotFound)?;
            let mut job = from_document(document)?;
            let previous_fencing_token = job.fencing_token();
            transition(&mut job)?;
            if self
                .replace_if_unchanged(&job, previous_fencing_token)
                .await?
            {
                return Ok(job);
            }
        }
        Err(RepositoryError::ConcurrentModification)
    }
}

#[async_trait]
impl AiJobRepository for MongoAiJobRepository {
    async fn create(&self, job: AiJob) -> Result<CreateAiJobOutcome, RepositoryError> {
        match self.collection.insert_one(to_document(&job)?).await {
            Ok(_) => Ok(CreateAiJobOutcome::Created(job)),
            Err(error) if crate::bson_support::is_duplicate_key(&error) => {
                let existing = self
                    .collection
                    .find_one(doc! {
                        "ownerScope": job.owner_scope(),
                        "idempotencyKey": job.idempotency_key(),
                    })
                    .await?
                    .ok_or(RepositoryError::AiJobNotFound)?;
                let existing = from_document(existing)?;
                if existing.request_hash() == job.request_hash() {
                    Ok(CreateAiJobOutcome::Existing(existing))
                } else {
                    Err(RepositoryError::IdempotencyConflict {
                        requested_by: job.owner_scope().to_owned(),
                        idempotency_key: job.idempotency_key().to_owned(),
                    })
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn get(&self, job_id: &AiJobId) -> Result<Option<AiJob>, RepositoryError> {
        self.collection
            .find_one(doc! { "_id": job_id.to_string() })
            .await?
            .map(from_document)
            .transpose()
    }

    async fn list(&self, query: &AiJobQuery) -> Result<Vec<AiJob>, RepositoryError> {
        let mut filter = doc! { "ownerScope": &query.owner_scope };
        if let Some(task) = query.task {
            filter.insert("task", task.as_str());
        }
        let mut cursor = self
            .collection
            .find(filter)
            .sort(doc! { "requestedAt": -1, "_id": -1 })
            .limit(i64::try_from(query.limit).unwrap_or(i64::MAX))
            .await?;
        let mut jobs = Vec::new();
        while let Some(document) = cursor.try_next().await? {
            jobs.push(from_document(document)?);
        }
        Ok(jobs)
    }

    async fn claim_next(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
        lease_duration: Duration,
    ) -> Result<Option<AiJob>, RepositoryError> {
        if worker_id.trim().is_empty() {
            return Err(RepositoryError::AiJob(AiJobError::EmptyField("workerId")));
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

        // A lost compare-and-swap means another worker won this job; try the next one.
        for _ in 0..CAS_ATTEMPTS {
            let Some(document) = self
                .collection
                .find_one(doc! {
                    "status": AiJobStatus::Queued.as_str(),
                    "expiresAt": { "$gt": timestamp(now) },
                })
                .sort(doc! { "requestedAt": 1, "_id": 1 })
                .await?
            else {
                return Ok(None);
            };
            let mut job = from_document(document)?;
            let previous_fencing_token = job.fencing_token();
            if job.claim(worker_id, now, lease_expires_at).is_err() {
                continue;
            }
            if self
                .replace_if_unchanged(&job, previous_fencing_token)
                .await?
            {
                return Ok(Some(job));
            }
        }
        Ok(None)
    }

    async fn complete(
        &self,
        job_id: &AiJobId,
        worker_id: &str,
        fencing_token: u64,
        briefing_id: &str,
        now: DateTime<Utc>,
    ) -> Result<AiJob, RepositoryError> {
        self.transition(job_id, |job| {
            job.complete(worker_id, fencing_token, briefing_id.to_owned(), now)
        })
        .await
    }

    async fn fail(
        &self,
        job_id: &AiJobId,
        worker_id: &str,
        fencing_token: u64,
        error_code: String,
        redacted_error: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<AiJob, RepositoryError> {
        self.transition(job_id, |job| {
            job.fail(
                worker_id,
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
                "status": { "$in": [
                    AiJobStatus::Queued.as_str(),
                    AiJobStatus::Claimed.as_str(),
                ] },
            })
            .await?;
        let mut closed = 0;
        while let Some(document) = cursor.try_next().await? {
            let mut job = from_document(document)?;
            let previous_fencing_token = job.fencing_token();
            if job.is_expired_at(now) {
                if job.expire(now).is_err() {
                    continue;
                }
            } else if job.lease_lapsed_at(now) {
                if job
                    .mark_outcome_unknown("lease_lapsed".to_owned(), now)
                    .is_err()
                {
                    continue;
                }
            } else {
                continue;
            }
            if self
                .replace_if_unchanged(&job, previous_fencing_token)
                .await?
            {
                closed += 1;
            }
        }
        Ok(closed)
    }
}

/// Compare-and-swap attempts before a concurrent writer is reported as a conflict.
const CAS_ATTEMPTS: usize = 3;

fn bson_i64(value: u64) -> Result<i64, RepositoryError> {
    i64::try_from(value).map_err(|_| RepositoryError::CorruptRecord("fencingToken"))
}

fn to_document(job: &AiJob) -> Result<Document, RepositoryError> {
    let mut document = doc! {
        "_id": job.id().to_string(),
        "schemaVersion": i64::from(job.schema_version()),
        "ownerScope": job.owner_scope(),
        "idempotencyKey": job.idempotency_key(),
        "requestHash": job.request_hash(),
        "correlationId": job.correlation_id().to_string(),
        "task": job.task().as_str(),
        "researchEventIds": job.research_event_ids().to_vec(),
        "newsItemIds": job.news_item_ids().to_vec(),
        "status": job.status().as_str(),
        "attempt": i64::from(job.attempt()),
        "fencingToken": bson_i64(job.fencing_token())?,
        "requestedAt": timestamp(job.requested_at()),
        "expiresAt": timestamp(job.expires_at()),
    };
    for (field, value) in [
        ("leaseOwner", job.lease_owner()),
        ("briefingId", job.briefing_id()),
        ("errorCode", job.error_code()),
        ("redactedError", job.redacted_error()),
    ] {
        if let Some(value) = value {
            document.insert(field, value);
        }
    }
    for (field, value) in [
        ("leaseExpiresAt", job.lease_expires_at()),
        ("finishedAt", job.finished_at()),
    ] {
        if let Some(value) = value {
            document.insert(field, timestamp(value));
        }
    }
    Ok(document)
}

fn from_document(document: Document) -> Result<AiJob, RepositoryError> {
    let id = AiJobId::parse(document.get_str("_id").unwrap_or_default())
        .ok_or(RepositoryError::CorruptRecord("_id"))?;
    let status = AiJobStatus::parse(&required_text(&document, "status")?)
        .ok_or(RepositoryError::CorruptRecord("status"))?;
    let task = AiTaskKind::parse(&required_text(&document, "task")?)
        .ok_or(RepositoryError::CorruptRecord("task"))?;
    let schema_version = u32::try_from(integer(&document, "schemaVersion")?)
        .map_err(|_| RepositoryError::CorruptRecord("schemaVersion"))?;
    let attempt = u32::try_from(integer(&document, "attempt")?)
        .map_err(|_| RepositoryError::CorruptRecord("attempt"))?;
    let fencing_token = u64::try_from(integer(&document, "fencingToken")?)
        .map_err(|_| RepositoryError::CorruptRecord("fencingToken"))?;
    AiJob::restore(PersistedAiJob {
        id,
        schema_version,
        owner_scope: required_text(&document, "ownerScope")?,
        idempotency_key: required_text(&document, "idempotencyKey")?,
        request_hash: required_text(&document, "requestHash")?,
        correlation_id: uuid::Uuid::parse_str(&required_text(&document, "correlationId")?)
            .map_err(|_| RepositoryError::CorruptRecord("correlationId"))?,
        task,
        research_event_ids: string_list(&document, "researchEventIds")?,
        news_item_ids: string_list(&document, "newsItemIds")?,
        status,
        attempt,
        lease_owner: optional_text(&document, "leaseOwner"),
        lease_expires_at: optional_timestamp(&document, "leaseExpiresAt")?,
        fencing_token,
        requested_at: required_timestamp(&document, "requestedAt")?,
        expires_at: required_timestamp(&document, "expiresAt")?,
        finished_at: optional_timestamp(&document, "finishedAt")?,
        briefing_id: optional_text(&document, "briefingId"),
        error_code: optional_text(&document, "errorCode"),
        redacted_error: optional_text(&document, "redactedError"),
    })
    .map_err(RepositoryError::from)
}

fn integer(document: &Document, field: &'static str) -> Result<i64, RepositoryError> {
    document
        .get(field)
        .ok_or(RepositoryError::CorruptRecord(field))
        .and_then(|value| number_as_i64(field, value))
}

fn required_text(document: &Document, field: &'static str) -> Result<String, RepositoryError> {
    match document.get(field) {
        Some(Bson::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
        _ => Err(RepositoryError::CorruptRecord(field)),
    }
}

fn optional_text(document: &Document, field: &'static str) -> Option<String> {
    document.get_str(field).ok().map(str::to_owned)
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
    optional_timestamp(document, field)?.ok_or(RepositoryError::CorruptRecord(field))
}

fn optional_timestamp(
    document: &Document,
    field: &'static str,
) -> Result<Option<DateTime<Utc>>, RepositoryError> {
    match document.get(field) {
        None | Some(Bson::Null) => Ok(None),
        Some(Bson::DateTime(value)) => Ok(Some(from_timestamp(*value)?)),
        Some(_) => Err(RepositoryError::CorruptRecord(field)),
    }
}
