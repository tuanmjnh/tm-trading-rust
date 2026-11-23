use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{FutureExt, TryStreamExt};
use mongodb::bson::{Bson, Document, doc};
use mongodb::options::{IndexOptions, ReturnDocument, WriteConcern};
use mongodb::{ClientSession, Collection, Database, IndexModel};

use tm_trading_domain::{
    AiBriefing, AiBriefingReference, AiBriefingReferenceKind, AiJob, AiJobError, AiJobId,
    AiJobStatus, AiTaskKind, NewAiBriefing, PersistedAiJob,
};

use crate::bson_support::{from_timestamp, number_as_i64, timestamp};
use crate::{AiJobQuery, AiJobRepository, CreateAiJobOutcome, RepositoryError};

/// Durable `ai_jobs` store.
#[derive(Clone)]
pub struct MongoAiJobRepository {
    collection: Collection<Document>,
    briefings: Collection<Document>,
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
        crate::mongo_ai_briefings::ensure_indexes(database).await?;
        Ok(Self {
            collection,
            briefings: database.collection::<Document>("ai_briefings"),
        })
    }

    /// Verifies that the briefing sink accepts the same upsert used for publication.
    ///
    /// The probe runs inside a transaction and is deliberately aborted, so it leaves no
    /// briefing document behind while still checking write authorization and validation.
    pub async fn probe_briefing_sink(&self) -> Result<(), RepositoryError> {
        let briefing = AiBriefing::new(NewAiBriefing {
            owner_scope: format!("__ai_worker_probe_{}", uuid::Uuid::new_v4()),
            task: AiTaskKind::ResearchExplanation,
            provider_id: "readiness-probe".to_owned(),
            model_registry_id: "readiness-probe".to_owned(),
            model_id: "readiness-probe".to_owned(),
            prompt_version: "readiness-probe.v1".to_owned(),
            facts: vec!["MongoDB briefing sink write probe".to_owned()],
            interpretations: Vec::new(),
            uncertainties: Vec::new(),
            input_snapshot_ids: vec!["rsnap1_0123456789abcdef".to_owned()],
            references: vec![AiBriefingReference::new(
                AiBriefingReferenceKind::ResearchEvent,
                "rev1_0123456789abcdef",
            )?],
            missing_inputs: Vec::new(),
            stale_inputs: Vec::new(),
            generated_at: Utc::now(),
        })?;
        let expected = crate::mongo_ai_briefings::fields(&briefing)?;
        let mut update = Document::new();
        update.insert("$setOnInsert", expected);
        let mut session = self.briefings.client().start_session().await?;
        session
            .start_transaction()
            .write_concern(WriteConcern::majority())
            .await?;
        let result = self
            .briefings
            .update_one(doc! { "_id": briefing.id() }, update)
            .upsert(true)
            .session(&mut session)
            .await;
        match result {
            Ok(_) => {
                session.abort_transaction().await?;
                Ok(())
            }
            Err(error) => {
                let _ = session.abort_transaction().await;
                Err(error.into())
            }
        }
    }

    async fn complete_with_briefing_transaction(
        &self,
        job_id: &AiJobId,
        worker_id: &str,
        fencing_token: u64,
        briefing: &AiBriefing,
        session: &mut ClientSession,
    ) -> mongodb::error::Result<TransactionOutcome> {
        let document = self
            .collection
            .find_one(doc! { "_id": job_id.to_string() })
            .session(&mut *session)
            .await?
            .ok_or_else(|| mongodb::error::Error::custom(TransactionAbort::AiJobNotFound))?;
        let claimed = from_document(document).map_err(transaction_document_error)?;
        if claimed.owner_scope() != briefing.owner_scope() || claimed.task() != briefing.task() {
            return Ok(TransactionOutcome::BriefingMismatch);
        }
        match claimed.status() {
            AiJobStatus::OutcomeUnknown => return Ok(TransactionOutcome::OutcomeUnknown(claimed)),
            AiJobStatus::Succeeded => {
                return if claimed.briefing_id() == Some(briefing.id()) {
                    Ok(TransactionOutcome::Succeeded(claimed))
                } else {
                    Err(mongodb::error::Error::custom(
                        TransactionAbort::BriefingConflict,
                    ))
                };
            }
            AiJobStatus::Claimed => {}
            _ => {
                return Err(mongodb::error::Error::custom(TransactionAbort::AiJob(
                    AiJobError::InvalidTransition,
                )));
            }
        }
        if let Err(error) = claimed.validate_lease_holder(worker_id, fencing_token) {
            return Ok(TransactionOutcome::LeaseError(error));
        }

        let filter = active_lease_filter(job_id, fencing_token, worker_id)
            .map_err(transaction_document_error)?;
        if let Some(completed) = self
            .apply_terminal_update(
                job_id,
                filter,
                TerminalState::Succeeded {
                    briefing_id: briefing.id().to_owned(),
                },
                Some(&mut *session),
            )
            .await?
        {
            let expected =
                crate::mongo_ai_briefings::fields(briefing).map_err(transaction_document_error)?;
            let mut update = Document::new();
            update.insert("$setOnInsert", expected.clone());
            self.briefings
                .update_one(doc! { "_id": briefing.id() }, update)
                .session(&mut *session)
                .upsert(true)
                .await?;
            let stored = self
                .briefings
                .find_one(doc! { "_id": briefing.id() })
                .session(&mut *session)
                .await?
                .ok_or_else(|| {
                    mongodb::error::Error::custom(TransactionAbort::CorruptRecord("briefing"))
                })?;
            if stored != expected {
                return Err(mongodb::error::Error::custom(
                    TransactionAbort::BriefingConflict,
                ));
            }
            return Ok(TransactionOutcome::Succeeded(completed));
        }

        let filter = lapsed_lease_filter(job_id, fencing_token, worker_id)
            .map_err(transaction_document_error)?;
        if let Some(unknown) = self
            .apply_terminal_update(
                job_id,
                filter,
                TerminalState::OutcomeUnknown,
                Some(&mut *session),
            )
            .await?
        {
            return Ok(TransactionOutcome::OutcomeUnknown(unknown));
        }
        Ok(TransactionOutcome::ConcurrentModification)
    }

    /// Applies one terminal update and returns the state observed in the same transaction.
    async fn apply_terminal_update(
        &self,
        job_id: &AiJobId,
        filter: Document,
        state: TerminalState,
        mut session: Option<&mut ClientSession>,
    ) -> mongodb::error::Result<Option<AiJob>> {
        let update = state.update();
        let result = match &mut session {
            Some(session) => {
                self.collection
                    .update_one(filter, update)
                    .session(&mut **session)
                    .await?
            }
            None => self.collection.update_one(filter, update).await?,
        };
        if result.matched_count != 1 {
            return Ok(None);
        }

        let document = match &mut session {
            Some(session) => {
                self.collection
                    .find_one(doc! { "_id": job_id.to_string() })
                    .session(&mut **session)
                    .await?
            }
            None => {
                self.collection
                    .find_one(doc! { "_id": job_id.to_string() })
                    .await?
            }
        }
        .ok_or_else(|| mongodb::error::Error::custom(TransactionAbort::AiJobNotFound))?;
        Ok(Some(
            from_document(document).map_err(transaction_document_error)?,
        ))
    }

    /// Reads, validates, and compare-and-swaps one job failure.
    async fn transition_failed(
        &self,
        job_id: &AiJobId,
        worker_id: &str,
        fencing_token: u64,
        error_code: String,
        redacted_error: Option<String>,
        _now: DateTime<Utc>,
    ) -> Result<AiJob, RepositoryError> {
        if error_code.trim().is_empty() {
            return Err(RepositoryError::AiJob(AiJobError::EmptyField("errorCode")));
        }
        let document = self
            .collection
            .find_one(doc! { "_id": job_id.to_string() })
            .await?
            .ok_or(RepositoryError::AiJobNotFound)?;
        let claimed = from_document(document)?;
        if claimed.status() == AiJobStatus::OutcomeUnknown {
            return Ok(claimed);
        }
        if let Err(error) = claimed.validate_lease_holder(worker_id, fencing_token) {
            return Err(error.into());
        }

        let filter = active_lease_filter(job_id, fencing_token, worker_id)?;
        if let Some(failed) = self
            .apply_terminal_update(
                job_id,
                filter,
                TerminalState::Failed {
                    error_code,
                    redacted_error,
                },
                None,
            )
            .await
            .map_err(mongodb_error_to_repository)?
        {
            return Ok(failed);
        }

        let filter = lapsed_lease_filter(job_id, fencing_token, worker_id)?;
        if let Some(unknown) = self
            .apply_terminal_update(job_id, filter, TerminalState::OutcomeUnknown, None)
            .await
            .map_err(mongodb_error_to_repository)?
        {
            return Ok(unknown);
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
        let worker_id = worker_id.trim();
        if worker_id.is_empty() {
            return Err(RepositoryError::AiJob(AiJobError::EmptyField("workerId")));
        }
        if lease_duration.is_zero() {
            return Err(RepositoryError::InvalidLeaseDuration);
        }
        let lease_millis = i64::try_from(lease_duration.as_millis())
            .map_err(|_| RepositoryError::LeaseDurationOutOfRange)?
            .max(1);

        self.expire_due(now).await?;

        let document = self
            .collection
            .find_one_and_update(
                queued_claim_filter(),
                vec![claim_update(worker_id, lease_millis)],
            )
            .sort(doc! { "requestedAt": 1, "_id": 1 })
            .return_document(ReturnDocument::After)
            .write_concern(WriteConcern::majority())
            .await?;
        document.map(from_document).transpose()
    }

    async fn complete_with_briefing(
        &self,
        job_id: &AiJobId,
        worker_id: &str,
        fencing_token: u64,
        briefing: &AiBriefing,
        _now: DateTime<Utc>,
    ) -> Result<AiJob, RepositoryError> {
        let mut session = self.collection.client().start_session().await?;
        let transaction = session
            .start_transaction()
            .write_concern(WriteConcern::majority())
            .and_run(
                (
                    self.clone(),
                    briefing.clone(),
                    worker_id.to_owned(),
                    *job_id,
                    fencing_token,
                ),
                |session, context| {
                    let repository = context.0.clone();
                    let briefing = context.1.clone();
                    let worker_id = context.2.clone();
                    let job_id = context.3;
                    let fencing_token = context.4;
                    async move {
                        repository
                            .complete_with_briefing_transaction(
                                &job_id,
                                &worker_id,
                                fencing_token,
                                &briefing,
                                session,
                            )
                            .await
                    }
                    .boxed()
                },
            )
            .await;
        match transaction {
            Ok(outcome) => outcome.into_repository_result(),
            Err(error) => {
                if let Some(abort) = error.get_custom::<TransactionAbort>() {
                    return Err(abort.clone().into_repository_error());
                }
                let error: RepositoryError = error.into();
                if let Some(observed) = self
                    .observed_terminal_outcome(job_id, briefing.id())
                    .await?
                {
                    return Ok(observed);
                }
                Err(error)
            }
        }
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
        self.transition_failed(
            job_id,
            worker_id,
            fencing_token,
            error_code,
            redacted_error,
            now,
        )
        .await
    }

    async fn expire_due(&self, _now: DateTime<Utc>) -> Result<usize, RepositoryError> {
        let expired = self
            .collection
            .update_many(queued_expiry_filter(), TerminalState::Expired.update())
            .await?
            .modified_count;
        let unknown = self
            .collection
            .update_many(
                lapsed_lease_expiry_filter(),
                TerminalState::OutcomeUnknown.update(),
            )
            .await?
            .modified_count;
        Ok(usize::try_from(expired + unknown).unwrap_or(usize::MAX))
    }
}

impl MongoAiJobRepository {
    async fn observed_terminal_outcome(
        &self,
        job_id: &AiJobId,
        briefing_id: &str,
    ) -> Result<Option<AiJob>, RepositoryError> {
        let Some(job) = self.get(job_id).await? else {
            return Ok(None);
        };
        match job.status() {
            AiJobStatus::OutcomeUnknown => Ok(Some(job)),
            AiJobStatus::Succeeded if job.briefing_id() == Some(briefing_id) => Ok(Some(job)),
            _ => Ok(None),
        }
    }
}

#[derive(Debug, Clone)]
enum TransactionOutcome {
    Succeeded(AiJob),
    OutcomeUnknown(AiJob),
    BriefingMismatch,
    LeaseError(AiJobError),
    ConcurrentModification,
}

impl TransactionOutcome {
    fn into_repository_result(self) -> Result<AiJob, RepositoryError> {
        match self {
            Self::Succeeded(job) | Self::OutcomeUnknown(job) => Ok(job),
            Self::BriefingMismatch => Err(RepositoryError::AiBriefingMismatch),
            Self::LeaseError(error) => Err(RepositoryError::AiJob(error)),
            Self::ConcurrentModification => Err(RepositoryError::ConcurrentModification),
        }
    }
}

#[derive(Debug, Clone)]
enum TransactionAbort {
    AiJob(AiJobError),
    AiJobNotFound,
    BriefingConflict,
    CorruptRecord(&'static str),
}

impl TransactionAbort {
    fn into_repository_error(self) -> RepositoryError {
        match self {
            Self::AiJob(error) => RepositoryError::AiJob(error),
            Self::AiJobNotFound => RepositoryError::AiJobNotFound,
            Self::BriefingConflict => RepositoryError::AiBriefingConflict,
            Self::CorruptRecord(field) => RepositoryError::CorruptRecord(field),
        }
    }
}

enum TerminalState {
    Succeeded {
        briefing_id: String,
    },
    Failed {
        error_code: String,
        redacted_error: Option<String>,
    },
    Expired,
    OutcomeUnknown,
}

impl TerminalState {
    fn update(self) -> Document {
        let mut set = Document::new();
        let mut unset = doc! { "leaseOwner": "", "leaseExpiresAt": "" };
        match self {
            Self::Succeeded { briefing_id } => {
                set.insert("status", AiJobStatus::Succeeded.as_str());
                set.insert("briefingId", briefing_id);
                unset.insert("errorCode", "");
                unset.insert("redactedError", "");
            }
            Self::Failed {
                error_code,
                redacted_error,
            } => {
                set.insert("status", AiJobStatus::Failed.as_str());
                set.insert("errorCode", error_code);
                if let Some(redacted_error) = redacted_error {
                    set.insert("redactedError", redacted_error);
                } else {
                    unset.insert("redactedError", "");
                }
                unset.insert("briefingId", "");
            }
            Self::Expired => {
                set.insert("status", AiJobStatus::Expired.as_str());
                unset.insert("briefingId", "");
                unset.insert("errorCode", "");
                unset.insert("redactedError", "");
            }
            Self::OutcomeUnknown => {
                set.insert("status", AiJobStatus::OutcomeUnknown.as_str());
                set.insert("errorCode", "lease_lapsed");
                unset.insert("briefingId", "");
                unset.insert("redactedError", "");
            }
        }
        doc! {
            "$currentDate": { "finishedAt": true },
            "$set": set,
            "$unset": unset,
        }
    }
}

fn transaction_document_error(error: RepositoryError) -> mongodb::error::Error {
    match error {
        RepositoryError::AiJob(error) => {
            mongodb::error::Error::custom(TransactionAbort::AiJob(error))
        }
        RepositoryError::CorruptRecord(field) => {
            mongodb::error::Error::custom(TransactionAbort::CorruptRecord(field))
        }
        RepositoryError::AiJobNotFound => {
            mongodb::error::Error::custom(TransactionAbort::AiJobNotFound)
        }
        _ => mongodb::error::Error::custom(TransactionAbort::CorruptRecord("transaction")),
    }
}

fn mongodb_error_to_repository(error: mongodb::error::Error) -> RepositoryError {
    if let Some(abort) = error.get_custom::<TransactionAbort>() {
        return abort.clone().into_repository_error();
    }
    error.into()
}

fn bson_i64(value: u64) -> Result<i64, RepositoryError> {
    i64::try_from(value).map_err(|_| RepositoryError::CorruptRecord("fencingToken"))
}

fn queued_claim_filter() -> Document {
    doc! {
        "status": AiJobStatus::Queued.as_str(),
        "$expr": { "$gt": ["$expiresAt", "$$NOW"] },
    }
}

fn claim_update(worker_id: &str, lease_millis: i64) -> Document {
    doc! {
        "$set": {
            "status": AiJobStatus::Claimed.as_str(),
            "attempt": { "$add": [{ "$ifNull": ["$attempt", 0_i64] }, 1_i64] },
            "fencingToken": { "$add": [{ "$ifNull": ["$fencingToken", 0_i64] }, 1_i64] },
            "leaseOwner": worker_id,
            "leaseExpiresAt": {
                "$min": [
                    { "$add": ["$$NOW", lease_millis] },
                    "$expiresAt",
                ],
            },
        },
    }
}

fn active_lease_filter(
    job_id: &AiJobId,
    fencing_token: u64,
    worker_id: &str,
) -> Result<Document, RepositoryError> {
    Ok(doc! {
        "_id": job_id.to_string(),
        "status": AiJobStatus::Claimed.as_str(),
        "fencingToken": bson_i64(fencing_token)?,
        "leaseOwner": worker_id,
        "$expr": { "$gt": ["$leaseExpiresAt", "$$NOW"] },
    })
}

fn lapsed_lease_filter(
    job_id: &AiJobId,
    fencing_token: u64,
    worker_id: &str,
) -> Result<Document, RepositoryError> {
    Ok(doc! {
        "_id": job_id.to_string(),
        "status": AiJobStatus::Claimed.as_str(),
        "fencingToken": bson_i64(fencing_token)?,
        "leaseOwner": worker_id,
        "$expr": { "$lte": ["$leaseExpiresAt", "$$NOW"] },
    })
}

fn queued_expiry_filter() -> Document {
    doc! {
        "status": AiJobStatus::Queued.as_str(),
        "$expr": { "$lte": ["$expiresAt", "$$NOW"] },
    }
}

fn lapsed_lease_expiry_filter() -> Document {
    doc! {
        "status": AiJobStatus::Claimed.as_str(),
        "$expr": { "$lte": ["$leaseExpiresAt", "$$NOW"] },
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn claims_use_the_database_clock_and_fence_atomically() {
        assert_eq!(
            queued_claim_filter(),
            doc! {
                "status": "QUEUED",
                "$expr": { "$gt": ["$expiresAt", "$$NOW"] },
            }
        );
        assert_eq!(
            claim_update("worker-1", 300_000),
            doc! {
                "$set": {
                    "status": "CLAIMED",
                    "attempt": { "$add": [{ "$ifNull": ["$attempt", 0_i64] }, 1_i64] },
                    "fencingToken": { "$add": [{ "$ifNull": ["$fencingToken", 0_i64] }, 1_i64] },
                    "leaseOwner": "worker-1",
                    "leaseExpiresAt": {
                        "$min": [
                            { "$add": ["$$NOW", 300_000_i64] },
                            "$expiresAt",
                        ],
                    },
                },
            }
        );
    }

    #[test]
    fn terminal_writes_check_the_lease_against_server_time() {
        let job_id = AiJobId::from_uuid(Uuid::from_u128(3));
        let active = active_lease_filter(&job_id, 1, "worker-1").unwrap();
        let lapsed = lapsed_lease_filter(&job_id, 1, "worker-1").unwrap();

        assert_eq!(
            active,
            doc! {
                "_id": job_id.to_string(),
                "status": "CLAIMED",
                "fencingToken": 1_i64,
                "leaseOwner": "worker-1",
                "$expr": { "$gt": ["$leaseExpiresAt", "$$NOW"] },
            }
        );
        assert_eq!(
            lapsed,
            doc! {
                "_id": job_id.to_string(),
                "status": "CLAIMED",
                "fencingToken": 1_i64,
                "leaseOwner": "worker-1",
                "$expr": { "$lte": ["$leaseExpiresAt", "$$NOW"] },
            }
        );
    }

    #[test]
    fn expiry_updates_check_server_time() {
        assert_eq!(
            queued_expiry_filter(),
            doc! {
                "status": "QUEUED",
                "$expr": { "$lte": ["$expiresAt", "$$NOW"] },
            }
        );
        assert_eq!(
            lapsed_lease_expiry_filter(),
            doc! {
                "status": "CLAIMED",
                "$expr": { "$lte": ["$leaseExpiresAt", "$$NOW"] },
            }
        );
    }

    #[test]
    fn terminal_updates_use_the_database_clock() {
        let update = TerminalState::OutcomeUnknown.update();

        assert_eq!(
            update,
            doc! {
                "$currentDate": { "finishedAt": true },
                "$set": {
                    "status": "OUTCOME_UNKNOWN",
                    "errorCode": "lease_lapsed",
                },
                "$unset": {
                    "leaseOwner": "",
                    "leaseExpiresAt": "",
                    "briefingId": "",
                    "redactedError": "",
                },
            }
        );
    }
}
