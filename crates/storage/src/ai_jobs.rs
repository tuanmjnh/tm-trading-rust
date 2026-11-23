use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use tm_trading_domain::{AiBriefing, AiJob, AiJobId, AiTaskKind};

use crate::RepositoryError;

/// Default number of jobs returned by an owner-scoped read.
pub const DEFAULT_AI_JOB_LIMIT: usize = 50;

/// Filter of an owner-scoped job read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiJobQuery {
    pub owner_scope: String,
    pub task: Option<AiTaskKind>,
    pub limit: usize,
}

impl Default for AiJobQuery {
    fn default() -> Self {
        Self {
            owner_scope: String::new(),
            task: None,
            limit: DEFAULT_AI_JOB_LIMIT,
        }
    }
}

/// Result of creating a job under an idempotency key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateAiJobOutcome {
    Created(AiJob),
    Existing(AiJob),
}

/// Durable store for owner-scoped AI jobs.
///
/// A job is created once per `{ownerScope, idempotencyKey}`. A claim is an atomic
/// transition guarded by a lease and a fencing token, and an expired lease closes
/// the job as unknown instead of failed.
#[async_trait]
pub trait AiJobRepository: Send + Sync {
    async fn create(&self, job: AiJob) -> Result<CreateAiJobOutcome, RepositoryError>;

    async fn get(&self, job_id: &AiJobId) -> Result<Option<AiJob>, RepositoryError>;

    /// Newest jobs first.
    async fn list(&self, query: &AiJobQuery) -> Result<Vec<AiJob>, RepositoryError>;

    /// Claims the oldest queued, unexpired job for this worker.
    async fn claim_next(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
        lease_duration: Duration,
    ) -> Result<Option<AiJob>, RepositoryError>;

    /// Publishes a briefing and completes its job as one durable transition.
    async fn complete_with_briefing(
        &self,
        job_id: &AiJobId,
        worker_id: &str,
        fencing_token: u64,
        briefing: &AiBriefing,
        now: DateTime<Utc>,
    ) -> Result<AiJob, RepositoryError>;

    async fn fail(
        &self,
        job_id: &AiJobId,
        worker_id: &str,
        fencing_token: u64,
        error_code: String,
        redacted_error: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<AiJob, RepositoryError>;

    /// Expires queued jobs past their expiry and closes lapsed leases as unknown.
    async fn expire_due(&self, now: DateTime<Utc>) -> Result<usize, RepositoryError>;
}
