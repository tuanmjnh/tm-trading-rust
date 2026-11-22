use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use tm_trading_domain::{AiJob, AiJobId};

use crate::{AiJobRepository, CreateAiJobOutcome, RepositoryError};

#[derive(Default)]
pub struct InMemoryAiJobRepository {
    jobs: Mutex<HashMap<AiJobId, AiJob>>,
}

impl InMemoryAiJobRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> Result<std::sync::MutexGuard<'_, HashMap<AiJobId, AiJob>>, RepositoryError> {
        self.jobs.lock().map_err(|_| RepositoryError::LockPoisoned)
    }
}

#[async_trait]
impl AiJobRepository for InMemoryAiJobRepository {
    async fn create(&self, job: AiJob) -> Result<CreateAiJobOutcome, RepositoryError> {
        let mut state = self.state()?;
        let existing = state.values().find(|existing| {
            existing.owner_scope() == job.owner_scope()
                && existing.idempotency_key() == job.idempotency_key()
        });
        if let Some(existing) = existing {
            if existing.request_hash() != job.request_hash() {
                return Err(RepositoryError::IdempotencyConflict {
                    requested_by: job.owner_scope().to_owned(),
                    idempotency_key: job.idempotency_key().to_owned(),
                });
            }
            return Ok(CreateAiJobOutcome::Existing(existing.clone()));
        }
        state.insert(job.id(), job.clone());
        Ok(CreateAiJobOutcome::Created(job))
    }

    async fn get(&self, job_id: &AiJobId) -> Result<Option<AiJob>, RepositoryError> {
        Ok(self.state()?.get(job_id).cloned())
    }

    async fn claim_next(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
        lease_duration: Duration,
    ) -> Result<Option<AiJob>, RepositoryError> {
        if worker_id.trim().is_empty() {
            return Err(RepositoryError::AiJob(
                tm_trading_domain::AiJobError::EmptyField("workerId"),
            ));
        }
        if lease_duration.is_zero() {
            return Err(RepositoryError::InvalidLeaseDuration);
        }
        let lease_duration = chrono::Duration::from_std(lease_duration)
            .map_err(|_| RepositoryError::LeaseDurationOutOfRange)?;
        let lease_expires_at = now
            .checked_add_signed(lease_duration)
            .ok_or(RepositoryError::LeaseDurationOutOfRange)?;

        let mut state = self.state()?;
        expire_due_locked(&mut state, now);
        let candidate_id = state
            .values()
            .filter(|job| job.is_claimable_at(now))
            .min_by_key(|job| (job.requested_at(), job.id().to_string()))
            .map(AiJob::id);
        let Some(candidate_id) = candidate_id else {
            return Ok(None);
        };
        let job = state
            .get_mut(&candidate_id)
            .ok_or(RepositoryError::LockPoisoned)?;
        job.claim(worker_id, now, lease_expires_at)?;
        Ok(Some(job.clone()))
    }

    async fn complete(
        &self,
        job_id: &AiJobId,
        worker_id: &str,
        fencing_token: u64,
        briefing_id: &str,
        now: DateTime<Utc>,
    ) -> Result<AiJob, RepositoryError> {
        let mut state = self.state()?;
        let job = state
            .get_mut(job_id)
            .ok_or(RepositoryError::AiJobNotFound)?;
        job.complete(worker_id, fencing_token, briefing_id.to_owned(), now)?;
        Ok(job.clone())
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
        let mut state = self.state()?;
        let job = state
            .get_mut(job_id)
            .ok_or(RepositoryError::AiJobNotFound)?;
        job.fail(worker_id, fencing_token, error_code, redacted_error, now)?;
        Ok(job.clone())
    }

    async fn expire_due(&self, now: DateTime<Utc>) -> Result<usize, RepositoryError> {
        let mut state = self.state()?;
        Ok(expire_due_locked(&mut state, now))
    }
}

fn expire_due_locked(jobs: &mut HashMap<AiJobId, AiJob>, now: DateTime<Utc>) -> usize {
    let mut closed = 0;
    for job in jobs.values_mut() {
        let transitioned = if job.is_expired_at(now) {
            job.expire(now).is_ok()
        } else if job.lease_lapsed_at(now) {
            job.mark_outcome_unknown("lease_lapsed".to_owned(), now)
                .is_ok()
        } else {
            false
        };
        if transitioned {
            closed += 1;
        }
    }
    closed
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use tm_trading_domain::{AiJobStatus, AiTaskKind, NewAiJob};

    use super::*;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 22, 8, 0, 0).unwrap()
    }

    fn job(idempotency_key: &str, request_hash: &str, expires_at: DateTime<Utc>) -> AiJob {
        job_requested_at(idempotency_key, request_hash, at(), expires_at)
    }

    fn job_requested_at(
        idempotency_key: &str,
        request_hash: &str,
        requested_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> AiJob {
        AiJob::new(NewAiJob {
            owner_scope: "local-operator".to_owned(),
            idempotency_key: idempotency_key.to_owned(),
            request_hash: request_hash.to_owned(),
            task: AiTaskKind::ResearchExplanation,
            research_event_ids: vec!["rev1_0123456789abcdef".to_owned()],
            news_item_ids: Vec::new(),
            requested_at,
            expires_at,
        })
        .unwrap()
    }

    #[tokio::test]
    async fn creation_is_idempotent_and_rejects_a_different_request() {
        let repository = InMemoryAiJobRepository::new();
        let first = job("explain-1", "hash-1", at() + chrono::Duration::hours(1));
        let created = repository.create(first.clone()).await.unwrap();
        assert!(matches!(created, CreateAiJobOutcome::Created(_)));

        let replay = repository
            .create(job(
                "explain-1",
                "hash-1",
                at() + chrono::Duration::hours(1),
            ))
            .await
            .unwrap();
        let CreateAiJobOutcome::Existing(existing) = replay else {
            panic!("expected the stored job");
        };
        assert_eq!(existing.id(), first.id());

        let conflict = repository
            .create(job(
                "explain-1",
                "hash-2",
                at() + chrono::Duration::hours(1),
            ))
            .await;
        assert!(matches!(
            conflict,
            Err(RepositoryError::IdempotencyConflict { .. })
        ));
        assert_eq!(repository.get(&first.id()).await.unwrap(), Some(first));
    }

    #[tokio::test]
    async fn claims_are_leased_oldest_first_and_fenced() {
        let repository = InMemoryAiJobRepository::new();
        let first = job("explain-1", "hash-1", at() + chrono::Duration::hours(1));
        let second = job_requested_at(
            "explain-2",
            "hash-2",
            at() + chrono::Duration::seconds(1),
            at() + chrono::Duration::hours(1),
        );
        repository.create(first.clone()).await.unwrap();
        repository.create(second.clone()).await.unwrap();

        let claimed = repository
            .claim_next("ai-worker", at(), Duration::from_secs(300))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claimed.id(), first.id());
        assert_eq!(claimed.status(), AiJobStatus::Claimed);
        assert_eq!(claimed.attempt(), 1);
        assert_eq!(claimed.fencing_token(), 1);

        let other = repository
            .claim_next("ai-worker", at(), Duration::from_secs(300))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(other.id(), second.id());
        assert_eq!(
            repository
                .claim_next("ai-worker", at(), Duration::from_secs(300))
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn completion_and_failure_need_the_current_lease() {
        let repository = InMemoryAiJobRepository::new();
        let stored = job("explain-1", "hash-1", at() + chrono::Duration::hours(1));
        repository.create(stored.clone()).await.unwrap();
        let claimed = repository
            .claim_next("ai-worker", at(), Duration::from_secs(300))
            .await
            .unwrap()
            .unwrap();

        assert!(matches!(
            repository
                .complete(
                    &claimed.id(),
                    "other-worker",
                    claimed.fencing_token(),
                    "aibr1_0123456789abcdef",
                    at()
                )
                .await,
            Err(RepositoryError::AiJob(_))
        ));

        let completed = repository
            .complete(
                &claimed.id(),
                "ai-worker",
                claimed.fencing_token(),
                "aibr1_0123456789abcdef",
                at() + chrono::Duration::seconds(30),
            )
            .await
            .unwrap();
        assert_eq!(completed.status(), AiJobStatus::Succeeded);
        assert_eq!(completed.briefing_id(), Some("aibr1_0123456789abcdef"));

        let missing = repository
            .fail(
                &AiJobId::new(),
                "ai-worker",
                1,
                "provider_unavailable".to_owned(),
                None,
                at(),
            )
            .await;
        assert!(matches!(missing, Err(RepositoryError::AiJobNotFound)));
    }

    #[tokio::test]
    async fn expiry_closes_queued_and_lapsed_jobs_differently() {
        let repository = InMemoryAiJobRepository::new();
        repository
            .create(job(
                "explain-1",
                "hash-1",
                at() + chrono::Duration::minutes(10),
            ))
            .await
            .unwrap();

        let claimed = repository
            .claim_next("ai-worker", at(), Duration::from_secs(60))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claimed.attempt(), 1);

        let still_queued = job("explain-2", "hash-2", at() + chrono::Duration::minutes(20));
        repository.create(still_queued.clone()).await.unwrap();

        let closed = repository
            .expire_due(at() + chrono::Duration::minutes(15))
            .await
            .unwrap();
        assert_eq!(closed, 1);
        let closed_claim = repository.get(&claimed.id()).await.unwrap().unwrap();
        assert_eq!(closed_claim.status(), AiJobStatus::OutcomeUnknown);
        assert_eq!(closed_claim.error_code(), Some("lease_lapsed"));
        assert_eq!(
            repository
                .get(&still_queued.id())
                .await
                .unwrap()
                .unwrap()
                .status(),
            AiJobStatus::Queued
        );

        let later = repository
            .expire_due(at() + chrono::Duration::minutes(25))
            .await
            .unwrap();
        assert_eq!(later, 1);
        let expired = repository.get(&still_queued.id()).await.unwrap().unwrap();
        assert_eq!(expired.status(), AiJobStatus::Expired);
    }
}
