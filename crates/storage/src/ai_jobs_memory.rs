use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use tm_trading_domain::{AiBriefing, AiJob, AiJobId, AiJobStatus};

use crate::{AiBriefingQuery, AiJobQuery, AiJobRepository, CreateAiJobOutcome, RepositoryError};

#[derive(Default)]
pub struct InMemoryAiJobRepository {
    jobs: Mutex<HashMap<AiJobId, AiJob>>,
    briefings: Mutex<HashMap<String, AiBriefing>>,
}

impl InMemoryAiJobRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> Result<std::sync::MutexGuard<'_, HashMap<AiJobId, AiJob>>, RepositoryError> {
        self.jobs.lock().map_err(|_| RepositoryError::LockPoisoned)
    }

    pub async fn get_briefing(
        &self,
        owner_scope: &str,
        briefing_id: &str,
    ) -> Result<Option<AiBriefing>, RepositoryError> {
        let stored = self
            .briefings
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        Ok(stored
            .get(briefing_id)
            .filter(|briefing| briefing.owner_scope() == owner_scope)
            .cloned())
    }

    pub async fn list_briefings(
        &self,
        query: &AiBriefingQuery,
    ) -> Result<Vec<AiBriefing>, RepositoryError> {
        let stored = self
            .briefings
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        let mut briefings = stored
            .values()
            .filter(|briefing| {
                briefing.owner_scope() == query.owner_scope
                    && query.task.is_none_or(|task| briefing.task() == task)
            })
            .cloned()
            .collect::<Vec<_>>();
        briefings.sort_by(|left, right| {
            right
                .generated_at()
                .cmp(&left.generated_at())
                .then_with(|| right.id().cmp(left.id()))
        });
        briefings.truncate(query.limit);
        Ok(briefings)
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

    async fn list(&self, query: &AiJobQuery) -> Result<Vec<AiJob>, RepositoryError> {
        let mut jobs = self
            .state()?
            .values()
            .filter(|job| job.owner_scope() == query.owner_scope)
            .filter(|job| query.task.is_none_or(|task| job.task() == task))
            .cloned()
            .collect::<Vec<_>>();
        jobs.sort_by(|left, right| {
            right
                .requested_at()
                .cmp(&left.requested_at())
                .then_with(|| right.id().to_string().cmp(&left.id().to_string()))
        });
        jobs.truncate(query.limit);
        Ok(jobs)
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

    async fn complete_with_briefing(
        &self,
        job_id: &AiJobId,
        worker_id: &str,
        fencing_token: u64,
        briefing: &AiBriefing,
        now: DateTime<Utc>,
    ) -> Result<AiJob, RepositoryError> {
        let mut jobs = self.state()?;
        let mut briefings = self
            .briefings
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        let mut completed = jobs
            .get(job_id)
            .cloned()
            .ok_or(RepositoryError::AiJobNotFound)?;
        if completed.owner_scope() != briefing.owner_scope() || completed.task() != briefing.task()
        {
            return Err(RepositoryError::AiBriefingMismatch);
        }
        match completed.status() {
            AiJobStatus::OutcomeUnknown => return Ok(completed),
            AiJobStatus::Succeeded => {
                return if completed.briefing_id() == Some(briefing.id()) {
                    Ok(completed)
                } else {
                    Err(RepositoryError::AiBriefingConflict)
                };
            }
            AiJobStatus::Claimed => {}
            _ => {
                return Err(RepositoryError::AiJob(
                    tm_trading_domain::AiJobError::InvalidTransition,
                ));
            }
        }
        match completed.complete(worker_id, fencing_token, briefing.id().to_owned(), now) {
            Ok(()) => {}
            Err(tm_trading_domain::AiJobError::LeaseLapsed) => {
                completed.mark_outcome_unknown("lease_lapsed".to_owned(), now)?;
                jobs.insert(completed.id(), completed.clone());
                return Ok(completed);
            }
            Err(error) => return Err(error.into()),
        }
        match briefings.get(briefing.id()) {
            Some(existing) if existing != briefing => {
                return Err(RepositoryError::AiBriefingConflict);
            }
            Some(_) => {}
            None => {
                briefings.insert(briefing.id().to_owned(), briefing.clone());
            }
        }
        jobs.insert(completed.id(), completed.clone());
        Ok(completed)
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
        let mut job = state
            .get(job_id)
            .cloned()
            .ok_or(RepositoryError::AiJobNotFound)?;
        match job.fail(worker_id, fencing_token, error_code, redacted_error, now) {
            Ok(()) => {}
            Err(tm_trading_domain::AiJobError::LeaseLapsed) => {
                job.mark_outcome_unknown("lease_lapsed".to_owned(), now)?;
            }
            Err(error) => return Err(error.into()),
        }
        state.insert(job.id(), job.clone());
        Ok(job)
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
    use tm_trading_domain::{
        AiBriefing, AiBriefingReference, AiBriefingReferenceKind, AiJobStatus, AiTaskKind,
        NewAiBriefing, NewAiJob,
    };

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
            correlation_id: uuid::Uuid::new_v4(),
            task: AiTaskKind::ResearchExplanation,
            research_event_ids: vec!["rev1_0123456789abcdef".to_owned()],
            news_item_ids: Vec::new(),
            requested_at,
            expires_at,
        })
        .unwrap()
    }

    fn briefing() -> AiBriefing {
        briefing_for("local-operator", AiTaskKind::ResearchExplanation)
    }

    fn briefing_for(owner_scope: &str, task: AiTaskKind) -> AiBriefing {
        AiBriefing::new(NewAiBriefing {
            owner_scope: owner_scope.to_owned(),
            task,
            provider_id: "primary".to_owned(),
            model_registry_id: "primary-large".to_owned(),
            model_id: "model-a".to_owned(),
            prompt_version: "research-explanation.v1".to_owned(),
            facts: vec!["Research fact".to_owned()],
            interpretations: vec!["Momentum may be strengthening.".to_owned()],
            uncertainties: Vec::new(),
            input_snapshot_ids: vec!["rsnap1_0123456789abcdef".to_owned()],
            references: vec![
                AiBriefingReference::new(
                    AiBriefingReferenceKind::ResearchEvent,
                    "rev1_0123456789abcdef",
                )
                .unwrap(),
            ],
            missing_inputs: Vec::new(),
            stale_inputs: Vec::new(),
            generated_at: at(),
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
    async fn lists_only_the_owner_and_applies_the_task_and_limit() {
        let repository = InMemoryAiJobRepository::new();
        let first = job_requested_at(
            "explain-1",
            "hash-1",
            at(),
            at() + chrono::Duration::hours(1),
        );
        let second = job_requested_at(
            "explain-2",
            "hash-2",
            at() + chrono::Duration::seconds(1),
            at() + chrono::Duration::hours(1),
        );
        let foreign = AiJob::new(NewAiJob {
            owner_scope: "other-operator".to_owned(),
            idempotency_key: "explain-3".to_owned(),
            request_hash: "hash-3".to_owned(),
            correlation_id: uuid::Uuid::new_v4(),
            task: AiTaskKind::ResearchExplanation,
            research_event_ids: vec!["rev1_0123456789abcdef".to_owned()],
            news_item_ids: Vec::new(),
            requested_at: at(),
            expires_at: at() + chrono::Duration::hours(1),
        })
        .unwrap();
        repository.create(first.clone()).await.unwrap();
        repository.create(second.clone()).await.unwrap();
        repository.create(foreign).await.unwrap();

        let listed = repository
            .list(&AiJobQuery {
                owner_scope: "local-operator".to_owned(),
                task: Some(AiTaskKind::ResearchExplanation),
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(
            listed.iter().map(|job| job.id()).collect::<Vec<_>>(),
            vec![second.id(), first.id()]
        );

        let limited = repository
            .list(&AiJobQuery {
                owner_scope: "local-operator".to_owned(),
                task: None,
                limit: 1,
            })
            .await
            .unwrap();
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].id(), second.id());
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

        let briefing = briefing();
        assert!(matches!(
            repository
                .complete_with_briefing(
                    &claimed.id(),
                    "other-worker",
                    claimed.fencing_token(),
                    &briefing,
                    at()
                )
                .await,
            Err(RepositoryError::AiJob(_))
        ));
        assert!(
            repository
                .get(&claimed.id())
                .await
                .unwrap()
                .unwrap()
                .briefing_id()
                .is_none()
        );

        let completed = repository
            .complete_with_briefing(
                &claimed.id(),
                "ai-worker",
                claimed.fencing_token(),
                &briefing,
                at() + chrono::Duration::seconds(30),
            )
            .await
            .unwrap();
        assert_eq!(completed.status(), AiJobStatus::Succeeded);
        assert_eq!(completed.briefing_id(), Some(briefing.id()));
        assert_eq!(
            repository
                .get_briefing("local-operator", briefing.id())
                .await
                .unwrap(),
            Some(briefing)
        );

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
    async fn a_briefing_that_does_not_match_its_job_cannot_complete_it() {
        let repository = InMemoryAiJobRepository::new();
        repository
            .create(job(
                "explain-1",
                "hash-1",
                at() + chrono::Duration::hours(1),
            ))
            .await
            .unwrap();
        let claimed = repository
            .claim_next("ai-worker", at(), Duration::from_secs(300))
            .await
            .unwrap()
            .unwrap();
        for mismatched in [
            briefing_for("other-operator", AiTaskKind::ResearchExplanation),
            briefing_for("local-operator", AiTaskKind::NewsSummary),
        ] {
            let result = repository
                .complete_with_briefing(
                    &claimed.id(),
                    "ai-worker",
                    claimed.fencing_token(),
                    &mismatched,
                    at(),
                )
                .await;

            assert!(matches!(result, Err(RepositoryError::AiBriefingMismatch)));
            assert!(
                repository
                    .get_briefing(mismatched.owner_scope(), mismatched.id())
                    .await
                    .unwrap()
                    .is_none()
            );
        }

        let stored = repository.get(&claimed.id()).await.unwrap().unwrap();
        assert_eq!(stored.status(), AiJobStatus::Claimed);
        assert!(stored.briefing_id().is_none());
    }

    #[tokio::test]
    async fn replaying_a_completed_publication_is_idempotent() {
        let repository = InMemoryAiJobRepository::new();
        repository
            .create(job(
                "explain-1",
                "hash-1",
                at() + chrono::Duration::hours(1),
            ))
            .await
            .unwrap();
        let claimed = repository
            .claim_next("ai-worker", at(), Duration::from_secs(300))
            .await
            .unwrap()
            .unwrap();
        let briefing = briefing();
        let completed = repository
            .complete_with_briefing(
                &claimed.id(),
                "ai-worker",
                claimed.fencing_token(),
                &briefing,
                at(),
            )
            .await
            .unwrap();

        let replayed = repository
            .complete_with_briefing(
                &claimed.id(),
                "ai-worker",
                claimed.fencing_token(),
                &briefing,
                at() + chrono::Duration::seconds(1),
            )
            .await
            .unwrap();

        assert_eq!(replayed, completed);
        assert_eq!(replayed.status(), AiJobStatus::Succeeded);
        assert_eq!(
            repository
                .get_briefing("local-operator", briefing.id())
                .await
                .unwrap(),
            Some(briefing)
        );
    }

    #[tokio::test]
    async fn an_expired_completion_is_unknown_and_publishes_nothing() {
        let repository = InMemoryAiJobRepository::new();
        repository
            .create(job(
                "explain-1",
                "hash-1",
                at() + chrono::Duration::hours(1),
            ))
            .await
            .unwrap();
        let claimed = repository
            .claim_next("ai-worker", at(), Duration::from_secs(60))
            .await
            .unwrap()
            .unwrap();
        let briefing = briefing();

        let completed = repository
            .complete_with_briefing(
                &claimed.id(),
                "ai-worker",
                claimed.fencing_token(),
                &briefing,
                at() + chrono::Duration::seconds(61),
            )
            .await
            .unwrap();

        assert_eq!(completed.status(), AiJobStatus::OutcomeUnknown);
        assert_eq!(completed.error_code(), Some("lease_lapsed"));
        assert!(completed.briefing_id().is_none());
        assert!(
            repository
                .get_briefing("local-operator", briefing.id())
                .await
                .unwrap()
                .is_none()
        );
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
