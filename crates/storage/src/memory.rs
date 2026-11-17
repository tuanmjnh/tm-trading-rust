use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use tm_trading_domain::{Operation, OperationId};

use crate::{CreateOperationOutcome, OperationRepository, RepositoryError};

#[derive(Default)]
pub struct InMemoryOperationRepository {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    operations: HashMap<OperationId, Operation>,
    idempotency: HashMap<(String, String), OperationId>,
}

impl InMemoryOperationRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> Result<MutexGuard<'_, State>, RepositoryError> {
        self.state.lock().map_err(|_| RepositoryError::LockPoisoned)
    }
}

#[async_trait]
impl OperationRepository for InMemoryOperationRepository {
    async fn create(
        &self,
        operation: Operation,
    ) -> Result<CreateOperationOutcome, RepositoryError> {
        let mut state = self.state()?;
        let key = (
            operation.requested_by().to_owned(),
            operation.idempotency_key().to_owned(),
        );

        if let Some(existing_id) = state.idempotency.get(&key) {
            let existing = state
                .operations
                .get(existing_id)
                .ok_or(RepositoryError::LockPoisoned)?;

            if existing.request_hash() == operation.request_hash() {
                return Ok(CreateOperationOutcome::Existing(existing.clone()));
            }

            return Err(RepositoryError::IdempotencyConflict {
                requested_by: key.0,
                idempotency_key: key.1,
            });
        }

        let operation_id = operation.id();
        state.idempotency.insert(key, operation_id);
        state.operations.insert(operation_id, operation.clone());
        Ok(CreateOperationOutcome::Created(operation))
    }

    async fn get(&self, operation_id: &OperationId) -> Result<Option<Operation>, RepositoryError> {
        let state = self.state()?;
        Ok(state.operations.get(operation_id).cloned())
    }

    async fn claim_next(
        &self,
        agent_id: &str,
        now: DateTime<Utc>,
        lease_duration: Duration,
    ) -> Result<Option<Operation>, RepositoryError> {
        if agent_id.trim().is_empty() {
            return Err(RepositoryError::Domain(
                tm_trading_domain::OperationError::EmptyField("agent_id"),
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
            .operations
            .iter()
            .filter(|(_, operation)| {
                operation.target_agent_id() == agent_id && operation.is_claimable_at(now)
            })
            .min_by_key(|(_, operation)| operation.requested_at())
            .map(|(operation_id, _)| *operation_id);

        let Some(candidate_id) = candidate_id else {
            return Ok(None);
        };
        let operation = state
            .operations
            .get_mut(&candidate_id)
            .ok_or(RepositoryError::LockPoisoned)?;
        operation.claim(agent_id, now, lease_expires_at)?;
        Ok(Some(operation.clone()))
    }

    async fn mark_running(
        &self,
        operation_id: &OperationId,
        agent_id: &str,
        fencing_token: u64,
        now: DateTime<Utc>,
    ) -> Result<Operation, RepositoryError> {
        let mut state = self.state()?;
        let operation = state
            .operations
            .get_mut(operation_id)
            .ok_or(RepositoryError::OperationNotFound)?;
        operation.mark_running(agent_id, fencing_token, now)?;
        Ok(operation.clone())
    }

    async fn mark_succeeded(
        &self,
        operation_id: &OperationId,
        agent_id: &str,
        fencing_token: u64,
        result_summary: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<Operation, RepositoryError> {
        let mut state = self.state()?;
        let operation = state
            .operations
            .get_mut(operation_id)
            .ok_or(RepositoryError::OperationNotFound)?;
        operation.mark_succeeded(agent_id, fencing_token, result_summary, now)?;
        Ok(operation.clone())
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
        let mut state = self.state()?;
        let operation = state
            .operations
            .get_mut(operation_id)
            .ok_or(RepositoryError::OperationNotFound)?;
        operation.mark_failed(agent_id, fencing_token, error_code, redacted_error, now)?;
        Ok(operation.clone())
    }

    async fn expire_due(&self, now: DateTime<Utc>) -> Result<usize, RepositoryError> {
        let mut state = self.state()?;
        Ok(expire_due_locked(&mut state, now))
    }
}

fn expire_due_locked(state: &mut State, now: DateTime<Utc>) -> usize {
    let mut expired = 0;
    for operation in state.operations.values_mut() {
        if !operation.status().is_terminal()
            && operation.expires_at() <= now
            && operation.mark_expired(now).is_ok()
        {
            expired += 1;
        }
    }
    expired
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::{Duration as ChronoDuration, TimeZone, Utc};
    use uuid::Uuid;

    use tm_trading_domain::{
        NewOperation, Operation, OperationAction, OperationError, OperationStatus,
    };

    use super::*;

    fn operation(requested_by: &str, idempotency_key: &str, request_hash: &str) -> Operation {
        let requested_at = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();
        Operation::new(NewOperation {
            idempotency_key: idempotency_key.to_owned(),
            request_hash: request_hash.to_owned(),
            requested_by: requested_by.to_owned(),
            requested_at,
            target_agent_id: "agent-1".to_owned(),
            service_instance_id: "sample-worker@agent-1".to_owned(),
            service_definition_id: "sample-worker".to_owned(),
            action: OperationAction::Start,
            expires_at: requested_at + ChronoDuration::minutes(5),
            correlation_id: Uuid::new_v4(),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn same_idempotency_key_and_hash_returns_existing_operation() {
        let repository = InMemoryOperationRepository::new();
        let first = operation("user-1", "start-1", "hash-1");
        let second = operation("user-1", "start-1", "hash-1");

        let first_outcome = repository.create(first).await.unwrap();
        let second_outcome = repository.create(second).await.unwrap();

        assert!(matches!(first_outcome, CreateOperationOutcome::Created(_)));
        let CreateOperationOutcome::Existing(existing) = second_outcome else {
            panic!("expected an existing operation");
        };
        assert_eq!(existing.request_hash(), "hash-1");
    }

    #[tokio::test]
    async fn same_idempotency_key_with_different_hash_is_rejected() {
        let repository = InMemoryOperationRepository::new();
        repository
            .create(operation("user-1", "start-1", "hash-1"))
            .await
            .unwrap();

        let result = repository
            .create(operation("user-1", "start-1", "hash-2"))
            .await;

        assert!(matches!(
            result,
            Err(RepositoryError::IdempotencyConflict { .. })
        ));
    }

    #[tokio::test]
    async fn active_lease_prevents_a_second_claim() {
        let repository = InMemoryOperationRepository::new();
        repository
            .create(operation("user-1", "start-1", "hash-1"))
            .await
            .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();

        let claimed = repository
            .claim_next("agent-1", now, Duration::from_secs(30))
            .await
            .unwrap()
            .unwrap();
        let second_claim = repository
            .claim_next(
                "agent-1",
                now + ChronoDuration::seconds(10),
                Duration::from_secs(30),
            )
            .await
            .unwrap();

        assert_eq!(claimed.fencing_token(), 1);
        assert!(second_claim.is_none());
    }

    #[tokio::test]
    async fn expired_lease_can_be_reclaimed_with_a_new_fencing_token() {
        let repository = InMemoryOperationRepository::new();
        repository
            .create(operation("user-1", "start-1", "hash-1"))
            .await
            .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();

        repository
            .claim_next("agent-1", now, Duration::from_secs(30))
            .await
            .unwrap()
            .unwrap();
        let reclaimed = repository
            .claim_next(
                "agent-1",
                now + ChronoDuration::seconds(31),
                Duration::from_secs(30),
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(reclaimed.attempt(), 2);
        assert_eq!(reclaimed.fencing_token(), 2);
    }

    #[tokio::test]
    async fn expired_unclaimed_operation_becomes_terminal() {
        let repository = InMemoryOperationRepository::new();
        let operation = operation("user-1", "start-1", "hash-1");
        let operation_id = operation.id();
        repository.create(operation).await.unwrap();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 5, 0).unwrap();

        let expired = repository.expire_due(now).await.unwrap();
        let stored = repository.get(&operation_id).await.unwrap().unwrap();

        assert_eq!(expired, 1);
        assert_eq!(stored.status(), OperationStatus::Expired);
    }

    #[tokio::test]
    async fn completion_requires_the_current_fencing_token() {
        let repository = InMemoryOperationRepository::new();
        let operation = operation("user-1", "start-1", "hash-1");
        let operation_id = operation.id();
        repository.create(operation).await.unwrap();
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap();

        repository
            .claim_next("agent-1", now, Duration::from_secs(30))
            .await
            .unwrap()
            .unwrap();
        repository
            .mark_running(&operation_id, "agent-1", 1, now)
            .await
            .unwrap();
        repository
            .claim_next(
                "agent-1",
                now + ChronoDuration::seconds(31),
                Duration::from_secs(30),
            )
            .await
            .unwrap()
            .unwrap();
        repository
            .mark_running(
                &operation_id,
                "agent-1",
                2,
                now + ChronoDuration::seconds(31),
            )
            .await
            .unwrap();

        let result = repository
            .mark_succeeded(
                &operation_id,
                "agent-1",
                1,
                Some("stale result".to_owned()),
                now + ChronoDuration::seconds(32),
            )
            .await;

        assert!(matches!(
            result,
            Err(RepositoryError::Domain(
                OperationError::LeaseOwnershipMismatch
            ))
        ));
    }
}
