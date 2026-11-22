use chrono::{DateTime, Utc};
use sha1::{Digest, Sha1};
use thiserror::Error;
use uuid::Uuid;

use crate::AiTaskKind;
use crate::ai_briefing::MAX_AI_BRIEFING_LIST_ITEMS;

/// Schema stamp written to every AI job document.
pub const AI_JOB_SCHEMA_VERSION: u32 = 1;

/// Prefix of an AI job identifier.
pub const AI_JOB_ID_PREFIX: &str = "aijob1_";

/// Maximum number of combined research and news inputs in one job.
pub const MAX_AI_JOB_INPUTS: usize = MAX_AI_BRIEFING_LIST_ITEMS;

/// Maximum lifetime of one AI job before it expires.
pub const MAX_AI_JOB_LIFETIME_HOURS: i64 = 24;

/// Canonical fingerprint of one job request.
///
/// The fingerprint covers the task and the exact requested inputs, so replaying the
/// same request is recognised and reusing one idempotency key for a different request
/// is detected.
#[must_use]
pub fn ai_job_request_hash(
    task: AiTaskKind,
    research_event_ids: &[String],
    news_item_ids: &[String],
) -> String {
    let mut canonical = String::from(task.as_str());
    for id in research_event_ids {
        canonical.push_str("\nresearch_event:");
        canonical.push_str(id);
    }
    for id in news_item_ids {
        canonical.push_str("\nnews_item:");
        canonical.push_str(id);
    }
    let digest = Sha1::digest(canonical.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Stable identifier of one AI job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AiJobId(Uuid);

impl AiJobId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }

    /// Parses the stored identifier form written by [`std::fmt::Display`].
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let suffix = value.trim().strip_prefix(AI_JOB_ID_PREFIX)?;
        if suffix.len() != 32
            || !suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return None;
        }
        Uuid::parse_str(suffix).ok().map(Self)
    }
}

impl Default for AiJobId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for AiJobId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{AI_JOB_ID_PREFIX}{}", self.0.simple())
    }
}

/// Durable state of one AI job.
///
/// A claimed job whose lease expires becomes `OUTCOME_UNKNOWN`, never `FAILED`:
/// an unobserved provider call is not evidence of a successful or failed run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiJobStatus {
    Queued,
    Claimed,
    Succeeded,
    Failed,
    Expired,
    OutcomeUnknown,
}

impl AiJobStatus {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Queued | Self::Claimed)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "QUEUED",
            Self::Claimed => "CLAIMED",
            Self::Succeeded => "SUCCEEDED",
            Self::Failed => "FAILED",
            Self::Expired => "EXPIRED",
            Self::OutcomeUnknown => "OUTCOME_UNKNOWN",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "QUEUED" => Some(Self::Queued),
            "CLAIMED" => Some(Self::Claimed),
            "SUCCEEDED" => Some(Self::Succeeded),
            "FAILED" => Some(Self::Failed),
            "EXPIRED" => Some(Self::Expired),
            "OUTCOME_UNKNOWN" => Some(Self::OutcomeUnknown),
            _ => None,
        }
    }
}

/// Fields a producer supplies for one AI job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAiJob {
    pub owner_scope: String,
    pub idempotency_key: String,
    pub request_hash: String,
    pub correlation_id: Uuid,
    pub task: AiTaskKind,
    pub research_event_ids: Vec<String>,
    pub news_item_ids: Vec<String>,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Complete stored state, used to rebuild a job from durable storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedAiJob {
    pub id: AiJobId,
    pub schema_version: u32,
    pub owner_scope: String,
    pub idempotency_key: String,
    pub request_hash: String,
    pub correlation_id: Uuid,
    pub task: AiTaskKind,
    pub research_event_ids: Vec<String>,
    pub news_item_ids: Vec<String>,
    pub status: AiJobStatus,
    pub attempt: u32,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub fencing_token: u64,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub briefing_id: Option<String>,
    pub error_code: Option<String>,
    pub redacted_error: Option<String>,
}

/// One durable request for an evidence-bound AI briefing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiJob {
    id: AiJobId,
    schema_version: u32,
    owner_scope: String,
    idempotency_key: String,
    request_hash: String,
    correlation_id: Uuid,
    task: AiTaskKind,
    research_event_ids: Vec<String>,
    news_item_ids: Vec<String>,
    status: AiJobStatus,
    attempt: u32,
    lease_owner: Option<String>,
    lease_expires_at: Option<DateTime<Utc>>,
    fencing_token: u64,
    requested_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    briefing_id: Option<String>,
    error_code: Option<String>,
    redacted_error: Option<String>,
}

impl AiJob {
    pub fn new(input: NewAiJob) -> Result<Self, AiJobError> {
        let validated = NormalizedAiJob::new(input)?;
        Ok(Self::from_validated(
            validated,
            AiJobState {
                id: AiJobId::new(),
                status: AiJobStatus::Queued,
                attempt: 0,
                fencing_token: 0,
                lease: None,
                finished_at: None,
                briefing_id: None,
                error_code: None,
                redacted_error: None,
            },
        ))
    }

    /// Rebuilds a stored job, rejecting a record that would violate an invariant.
    pub fn restore(input: PersistedAiJob) -> Result<Self, AiJobError> {
        if input.schema_version != AI_JOB_SCHEMA_VERSION {
            return Err(AiJobError::UnsupportedSchemaVersion {
                found: input.schema_version,
            });
        }
        let validated = NormalizedAiJob::new(NewAiJob {
            owner_scope: input.owner_scope,
            idempotency_key: input.idempotency_key,
            request_hash: input.request_hash,
            correlation_id: input.correlation_id,
            task: input.task,
            research_event_ids: input.research_event_ids,
            news_item_ids: input.news_item_ids,
            requested_at: input.requested_at,
            expires_at: input.expires_at,
        })?;
        if (input.status == AiJobStatus::Claimed)
            != (input.lease_owner.is_some() && input.lease_expires_at.is_some())
        {
            return Err(AiJobError::InconsistentLease);
        }
        Ok(Self::from_validated(
            validated,
            AiJobState {
                id: input.id,
                status: input.status,
                attempt: input.attempt,
                fencing_token: input.fencing_token,
                lease: input.lease_owner.zip(input.lease_expires_at),
                finished_at: input.finished_at,
                briefing_id: input.briefing_id,
                error_code: input.error_code,
                redacted_error: input.redacted_error,
            },
        ))
    }

    fn from_validated(validated: NormalizedAiJob, state: AiJobState) -> Self {
        Self {
            id: state.id,
            schema_version: AI_JOB_SCHEMA_VERSION,
            owner_scope: validated.owner_scope,
            idempotency_key: validated.idempotency_key,
            request_hash: validated.request_hash,
            correlation_id: validated.correlation_id,
            task: validated.task,
            research_event_ids: validated.research_event_ids,
            news_item_ids: validated.news_item_ids,
            status: state.status,
            attempt: state.attempt,
            lease_owner: state.lease.as_ref().map(|(owner, _)| owner.clone()),
            lease_expires_at: state.lease.map(|(_, expires_at)| expires_at),
            fencing_token: state.fencing_token,
            requested_at: validated.requested_at,
            expires_at: validated.expires_at,
            finished_at: state.finished_at,
            briefing_id: state.briefing_id,
            error_code: state.error_code,
            redacted_error: state.redacted_error,
        }
    }

    #[must_use]
    pub fn id(&self) -> AiJobId {
        self.id
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[must_use]
    pub fn owner_scope(&self) -> &str {
        &self.owner_scope
    }

    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    #[must_use]
    pub fn request_hash(&self) -> &str {
        &self.request_hash
    }

    #[must_use]
    pub const fn correlation_id(&self) -> Uuid {
        self.correlation_id
    }

    #[must_use]
    pub const fn task(&self) -> AiTaskKind {
        self.task
    }

    #[must_use]
    pub fn research_event_ids(&self) -> &[String] {
        &self.research_event_ids
    }

    #[must_use]
    pub fn news_item_ids(&self) -> &[String] {
        &self.news_item_ids
    }

    #[must_use]
    pub const fn status(&self) -> AiJobStatus {
        self.status
    }

    #[must_use]
    pub const fn attempt(&self) -> u32 {
        self.attempt
    }

    #[must_use]
    pub fn lease_owner(&self) -> Option<&str> {
        self.lease_owner.as_deref()
    }

    #[must_use]
    pub const fn lease_expires_at(&self) -> Option<DateTime<Utc>> {
        self.lease_expires_at
    }

    #[must_use]
    pub const fn fencing_token(&self) -> u64 {
        self.fencing_token
    }

    #[must_use]
    pub const fn requested_at(&self) -> DateTime<Utc> {
        self.requested_at
    }

    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    #[must_use]
    pub const fn finished_at(&self) -> Option<DateTime<Utc>> {
        self.finished_at
    }

    #[must_use]
    pub fn briefing_id(&self) -> Option<&str> {
        self.briefing_id.as_deref()
    }

    #[must_use]
    pub fn error_code(&self) -> Option<&str> {
        self.error_code.as_deref()
    }

    #[must_use]
    pub fn redacted_error(&self) -> Option<&str> {
        self.redacted_error.as_deref()
    }

    /// Whether a queued, unexpired job may be claimed now.
    #[must_use]
    pub fn is_claimable_at(&self, now: DateTime<Utc>) -> bool {
        self.status == AiJobStatus::Queued && self.expires_at > now
    }

    /// Whether a claimed job's lease has already lapsed.
    #[must_use]
    pub fn lease_lapsed_at(&self, now: DateTime<Utc>) -> bool {
        self.status == AiJobStatus::Claimed
            && self.lease_expires_at.is_some_and(|expires| expires <= now)
    }

    /// Whether a queued job is past its expiry.
    #[must_use]
    pub fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        self.status == AiJobStatus::Queued && self.expires_at <= now
    }

    pub fn claim(
        &mut self,
        worker_id: &str,
        now: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<(), AiJobError> {
        if worker_id.trim().is_empty() {
            return Err(AiJobError::EmptyField("workerId"));
        }
        if self.is_expired_at(now) {
            return Err(AiJobError::Expired);
        }
        if !self.is_claimable_at(now) {
            return Err(AiJobError::NotClaimable);
        }
        if lease_expires_at <= now || lease_expires_at > self.expires_at {
            return Err(AiJobError::InvalidLease);
        }
        self.attempt = self
            .attempt
            .checked_add(1)
            .ok_or(AiJobError::AttemptOverflow)?;
        self.fencing_token = self
            .fencing_token
            .checked_add(1)
            .ok_or(AiJobError::FencingTokenOverflow)?;
        self.status = AiJobStatus::Claimed;
        self.lease_owner = Some(worker_id.trim().to_owned());
        self.lease_expires_at = Some(lease_expires_at);
        Ok(())
    }

    /// Records the briefing a claimed job produced.
    pub fn complete(
        &mut self,
        worker_id: &str,
        fencing_token: u64,
        briefing_id: String,
        now: DateTime<Utc>,
    ) -> Result<(), AiJobError> {
        let briefing_id = required("briefingId", briefing_id)?;
        self.finish(AiJobStatus::Succeeded, worker_id, fencing_token, now)?;
        self.briefing_id = Some(briefing_id);
        Ok(())
    }

    /// Records a known failure of a claimed job.
    pub fn fail(
        &mut self,
        worker_id: &str,
        fencing_token: u64,
        error_code: String,
        redacted_error: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<(), AiJobError> {
        let error_code = required("errorCode", error_code)?;
        self.finish(AiJobStatus::Failed, worker_id, fencing_token, now)?;
        self.error_code = Some(error_code);
        self.redacted_error = redacted_error;
        Ok(())
    }

    /// Closes a job whose result can no longer be observed.
    pub fn mark_outcome_unknown(
        &mut self,
        reason_code: String,
        now: DateTime<Utc>,
    ) -> Result<(), AiJobError> {
        if self.status != AiJobStatus::Claimed {
            return Err(AiJobError::InvalidTransition);
        }
        self.status = AiJobStatus::OutcomeUnknown;
        self.finished_at = Some(now);
        self.lease_owner = None;
        self.lease_expires_at = None;
        self.error_code = Some(required("errorCode", reason_code)?);
        Ok(())
    }

    /// Expires a queued job that was never claimed.
    pub fn expire(&mut self, now: DateTime<Utc>) -> Result<(), AiJobError> {
        if !self.is_expired_at(now) {
            return Err(AiJobError::NotExpired);
        }
        self.status = AiJobStatus::Expired;
        self.finished_at = Some(now);
        Ok(())
    }

    fn finish(
        &mut self,
        next: AiJobStatus,
        worker_id: &str,
        fencing_token: u64,
        now: DateTime<Utc>,
    ) -> Result<(), AiJobError> {
        if self.status != AiJobStatus::Claimed {
            return Err(AiJobError::InvalidTransition);
        }
        if self.lease_owner.as_deref() != Some(worker_id) {
            return Err(AiJobError::NotLeaseHolder);
        }
        if self.fencing_token != fencing_token {
            return Err(AiJobError::FencingMismatch);
        }
        if self.lease_expires_at.is_some_and(|expires| expires <= now) {
            return Err(AiJobError::LeaseLapsed);
        }
        self.status = next;
        self.finished_at = Some(now);
        self.lease_owner = None;
        self.lease_expires_at = None;
        Ok(())
    }
}

/// The part of a job that is not part of its immutable request.
struct AiJobState {
    id: AiJobId,
    status: AiJobStatus,
    attempt: u32,
    fencing_token: u64,
    lease: Option<(String, DateTime<Utc>)>,
    finished_at: Option<DateTime<Utc>>,
    briefing_id: Option<String>,
    error_code: Option<String>,
    redacted_error: Option<String>,
}

struct NormalizedAiJob {
    owner_scope: String,
    idempotency_key: String,
    request_hash: String,
    correlation_id: Uuid,
    task: AiTaskKind,
    research_event_ids: Vec<String>,
    news_item_ids: Vec<String>,
    requested_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl NormalizedAiJob {
    fn new(input: NewAiJob) -> Result<Self, AiJobError> {
        let owner_scope = required("ownerScope", input.owner_scope)?;
        let idempotency_key = required("idempotencyKey", input.idempotency_key)?;
        let request_hash = required("requestHash", input.request_hash)?;
        let research_event_ids = input_ids(input.research_event_ids)?;
        let news_item_ids = input_ids(input.news_item_ids)?;
        if research_event_ids.is_empty() && news_item_ids.is_empty() {
            return Err(AiJobError::NoInputs);
        }
        if research_event_ids.len() + news_item_ids.len() > MAX_AI_JOB_INPUTS {
            return Err(AiJobError::TooManyInputs);
        }
        if input.expires_at <= input.requested_at {
            return Err(AiJobError::InvalidExpiry);
        }
        if input.expires_at
            > input.requested_at + chrono::Duration::hours(MAX_AI_JOB_LIFETIME_HOURS)
        {
            return Err(AiJobError::ExpiryTooFar);
        }
        Ok(Self {
            owner_scope,
            idempotency_key,
            request_hash,
            correlation_id: input.correlation_id,
            task: input.task,
            research_event_ids,
            news_item_ids,
            requested_at: input.requested_at,
            expires_at: input.expires_at,
        })
    }
}

fn input_ids(values: Vec<String>) -> Result<Vec<String>, AiJobError> {
    if values.len() > MAX_AI_JOB_INPUTS {
        return Err(AiJobError::TooManyInputs);
    }
    values
        .into_iter()
        .map(|value| required("inputId", value))
        .collect()
}

fn required(field: &'static str, value: String) -> Result<String, AiJobError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AiJobError::EmptyField(field));
    }
    Ok(value.to_owned())
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AiJobError {
    #[error("ai job field '{0}' must not be empty")]
    EmptyField(&'static str),
    #[error("ai job must name at least one input")]
    NoInputs,
    #[error("ai job contains too many inputs")]
    TooManyInputs,
    #[error("ai job expiry must be after the request time")]
    InvalidExpiry,
    #[error("ai job expiry is further than {MAX_AI_JOB_LIFETIME_HOURS} hours away")]
    ExpiryTooFar,
    #[error("unsupported ai job schema version {found}")]
    UnsupportedSchemaVersion { found: u32 },
    #[error("ai job lease fields do not match its status")]
    InconsistentLease,
    #[error("ai job has expired")]
    Expired,
    #[error("ai job has not expired yet")]
    NotExpired,
    #[error("ai job is not claimable")]
    NotClaimable,
    #[error("ai job lease must end after now and no later than the job expiry")]
    InvalidLease,
    #[error("ai job is not held by this worker")]
    NotLeaseHolder,
    #[error("ai job lease has already lapsed")]
    LeaseLapsed,
    #[error("ai job fencing token does not match the claim")]
    FencingMismatch,
    #[error("ai job transition is not allowed from its current status")]
    InvalidTransition,
    #[error("ai job attempt counter overflowed")]
    AttemptOverflow,
    #[error("ai job fencing token overflowed")]
    FencingTokenOverflow,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 22, 8, 0, 0).unwrap()
    }

    fn input() -> NewAiJob {
        NewAiJob {
            owner_scope: "local-operator".to_owned(),
            idempotency_key: "explain-1".to_owned(),
            request_hash: "hash-1".to_owned(),
            correlation_id: Uuid::new_v4(),
            task: AiTaskKind::ResearchExplanation,
            research_event_ids: vec!["rev1_0123456789abcdef".to_owned()],
            news_item_ids: Vec::new(),
            requested_at: at(),
            expires_at: at() + chrono::Duration::hours(1),
        }
    }

    #[test]
    fn a_request_hash_covers_the_task_and_input_kind() {
        let research = vec!["rev1_0123456789abcdef".to_owned()];
        let news = vec!["news_0123456789abcdef0123456789abcdef01234567".to_owned()];
        let hash = ai_job_request_hash(AiTaskKind::ResearchExplanation, &research, &[]);
        assert_eq!(
            hash,
            ai_job_request_hash(AiTaskKind::ResearchExplanation, &research, &[])
        );
        assert_ne!(
            hash,
            ai_job_request_hash(AiTaskKind::ResearchExplanation, &[], &news)
        );
        assert_ne!(
            hash,
            ai_job_request_hash(AiTaskKind::NewsSummary, &research, &[])
        );
    }

    #[test]
    fn a_new_job_is_queued_and_claimable() {
        let job = AiJob::new(input()).unwrap();
        assert_eq!(job.status(), AiJobStatus::Queued);
        assert_eq!(job.attempt(), 0);
        assert!(job.is_claimable_at(at()));
        assert!(job.id().to_string().starts_with(AI_JOB_ID_PREFIX));
        assert_eq!(AiJobId::parse(&job.id().to_string()), Some(job.id()));
        assert_eq!(AiJobId::parse("aijob1_not-a-uuid"), None);
        assert_eq!(
            AiJobId::parse("aijob1_01234567-89ab-cdef-0123-456789abcdef"),
            None
        );
        assert_eq!(
            AiJobId::parse("aijob1_0123456789ABCDEF0123456789ABCDEF"),
            None
        );
    }

    #[test]
    fn a_claim_is_leased_and_fenced() {
        let mut job = AiJob::new(input()).unwrap();
        let lease = at() + chrono::Duration::minutes(5);
        job.claim("ai-worker", at(), lease).unwrap();
        assert_eq!(job.status(), AiJobStatus::Claimed);
        assert_eq!(job.attempt(), 1);
        assert_eq!(job.fencing_token(), 1);
        assert_eq!(job.lease_owner(), Some("ai-worker"));
        assert_eq!(job.lease_expires_at(), Some(lease));
        assert!(!job.is_claimable_at(at()));
        assert_eq!(
            job.claim("ai-worker", at(), lease).unwrap_err(),
            AiJobError::NotClaimable
        );
    }

    #[test]
    fn a_claim_rejects_an_expired_job_and_a_lease_beyond_expiry() {
        let mut job = AiJob::new(input()).unwrap();
        assert_eq!(
            job.claim(
                "ai-worker",
                at() + chrono::Duration::hours(1),
                at() + chrono::Duration::minutes(5)
            )
            .unwrap_err(),
            AiJobError::Expired
        );
        assert_eq!(
            job.claim("ai-worker", at(), at() + chrono::Duration::hours(2))
                .unwrap_err(),
            AiJobError::InvalidLease
        );
        assert_eq!(
            job.claim(" ", at(), at() + chrono::Duration::minutes(5))
                .unwrap_err(),
            AiJobError::EmptyField("workerId")
        );
    }

    #[test]
    fn completion_needs_the_current_lease_holder_and_fence() {
        let mut job = AiJob::new(input()).unwrap();
        let lease = at() + chrono::Duration::minutes(5);
        job.claim("ai-worker", at(), lease).unwrap();
        assert_eq!(
            job.complete("other-worker", 1, "aibr1_0123456789abcdef".to_owned(), at())
                .unwrap_err(),
            AiJobError::NotLeaseHolder
        );
        assert_eq!(
            job.complete("ai-worker", 2, "aibr1_0123456789abcdef".to_owned(), at())
                .unwrap_err(),
            AiJobError::FencingMismatch
        );
        job.complete(
            "ai-worker",
            1,
            "aibr1_0123456789abcdef".to_owned(),
            at() + chrono::Duration::minutes(1),
        )
        .unwrap();
        assert_eq!(job.status(), AiJobStatus::Succeeded);
        assert_eq!(job.briefing_id(), Some("aibr1_0123456789abcdef"));
        assert_eq!(job.lease_owner(), None);
        assert_eq!(
            job.complete("ai-worker", 1, "aibr1_0123456789abcdef".to_owned(), at())
                .unwrap_err(),
            AiJobError::InvalidTransition
        );
    }

    #[test]
    fn a_late_completion_is_rejected_after_the_lease_lapses() {
        let mut job = AiJob::new(input()).unwrap();
        let lease = at() + chrono::Duration::minutes(5);
        job.claim("ai-worker", at(), lease).unwrap();
        assert!(job.lease_lapsed_at(lease));
        assert_eq!(
            job.complete("ai-worker", 1, "aibr1_0123456789abcdef".to_owned(), lease)
                .unwrap_err(),
            AiJobError::LeaseLapsed
        );
    }

    #[test]
    fn failure_and_unknown_outcomes_are_distinct() {
        let mut failed = AiJob::new(input()).unwrap();
        failed
            .claim("ai-worker", at(), at() + chrono::Duration::minutes(5))
            .unwrap();
        failed
            .fail(
                "ai-worker",
                1,
                "provider_unavailable".to_owned(),
                Some("provider request failed".to_owned()),
                at() + chrono::Duration::minutes(1),
            )
            .unwrap();
        assert_eq!(failed.status(), AiJobStatus::Failed);
        assert_eq!(failed.error_code(), Some("provider_unavailable"));

        let mut unknown = AiJob::new(input()).unwrap();
        unknown
            .claim("ai-worker", at(), at() + chrono::Duration::minutes(5))
            .unwrap();
        unknown
            .mark_outcome_unknown(
                "lease_lapsed".to_owned(),
                at() + chrono::Duration::minutes(6),
            )
            .unwrap();
        assert_eq!(unknown.status(), AiJobStatus::OutcomeUnknown);
        assert_eq!(unknown.redacted_error(), None);
        assert_eq!(
            unknown
                .mark_outcome_unknown("again".to_owned(), at())
                .unwrap_err(),
            AiJobError::InvalidTransition
        );
    }

    #[test]
    fn only_a_due_queued_job_expires() {
        let mut job = AiJob::new(input()).unwrap();
        assert_eq!(job.expire(at()).unwrap_err(), AiJobError::NotExpired);
        job.expire(at() + chrono::Duration::hours(1)).unwrap();
        assert_eq!(job.status(), AiJobStatus::Expired);
        assert_eq!(job.finished_at(), Some(at() + chrono::Duration::hours(1)));
    }

    #[test]
    fn rejects_an_empty_request_a_missing_input_and_a_far_expiry() {
        let mut empty = input();
        empty.research_event_ids = Vec::new();
        assert_eq!(AiJob::new(empty).unwrap_err(), AiJobError::NoInputs);

        let mut blank = input();
        blank.owner_scope = "  ".to_owned();
        assert_eq!(
            AiJob::new(blank).unwrap_err(),
            AiJobError::EmptyField("ownerScope")
        );

        let mut far = input();
        far.expires_at = at() + chrono::Duration::hours(MAX_AI_JOB_LIFETIME_HOURS + 1);
        assert_eq!(AiJob::new(far).unwrap_err(), AiJobError::ExpiryTooFar);

        let mut inverted = input();
        inverted.expires_at = at();
        assert_eq!(AiJob::new(inverted).unwrap_err(), AiJobError::InvalidExpiry);
    }

    #[test]
    fn a_stored_job_round_trips_and_rejects_an_inconsistent_lease() {
        let mut job = AiJob::new(input()).unwrap();
        job.claim("ai-worker", at(), at() + chrono::Duration::minutes(5))
            .unwrap();
        let persisted = PersistedAiJob {
            id: job.id(),
            schema_version: job.schema_version(),
            owner_scope: job.owner_scope().to_owned(),
            idempotency_key: job.idempotency_key().to_owned(),
            request_hash: job.request_hash().to_owned(),
            correlation_id: job.correlation_id(),
            task: job.task(),
            research_event_ids: job.research_event_ids().to_vec(),
            news_item_ids: job.news_item_ids().to_vec(),
            status: job.status(),
            attempt: job.attempt(),
            lease_owner: job.lease_owner().map(str::to_owned),
            lease_expires_at: job.lease_expires_at(),
            fencing_token: job.fencing_token(),
            requested_at: job.requested_at(),
            expires_at: job.expires_at(),
            finished_at: job.finished_at(),
            briefing_id: job.briefing_id().map(str::to_owned),
            error_code: job.error_code().map(str::to_owned),
            redacted_error: job.redacted_error().map(str::to_owned),
        };
        assert_eq!(AiJob::restore(persisted.clone()).unwrap(), job);

        let mut broken = persisted.clone();
        broken.lease_expires_at = None;
        assert_eq!(
            AiJob::restore(broken).unwrap_err(),
            AiJobError::InconsistentLease
        );

        let mut unsupported = persisted;
        unsupported.schema_version += 1;
        assert!(matches!(
            AiJob::restore(unsupported).unwrap_err(),
            AiJobError::UnsupportedSchemaVersion { .. }
        ));
    }
}
