//! Live MongoDB tests for the durable operation store.
//!
//! These tests run only when `TM_TRADING_TEST_MONGODB_URI` is set to a TLS-enabled
//! MongoDB URI whose certificate chain validates against the local trust store or a
//! `tlsCAFile`. They create and drop a throwaway database per test, so they never
//! touch an existing collection.

use std::time::Duration;

use chrono::{Duration as ChronoDuration, TimeZone, Utc};
use tm_trading_domain::{
    NewOperation, Operation, OperationAction, OperationError, OperationStatus,
};
use tm_trading_storage::{
    CreateOperationOutcome, MongoConnection, MongoOperationRepository, OperationRepository,
    RepositoryError,
};
use uuid::Uuid;

fn uri() -> Option<String> {
    std::env::var("TM_TRADING_TEST_MONGODB_URI")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

async fn harness() -> Option<(MongoConnection, MongoOperationRepository, String)> {
    let uri = uri()?;
    let database_name = format!("tm-trading-test-{}", Uuid::new_v4().simple());
    let connection = MongoConnection::connect(&uri, &database_name)
        .await
        .expect("connect to the test MongoDB");
    let repository = MongoOperationRepository::new(connection.database())
        .await
        .expect("create the operation repository");
    Some((connection, repository, database_name))
}

macro_rules! harness_or_skip {
    () => {
        match harness().await {
            Some(value) => value,
            None => {
                eprintln!("skipped: TM_TRADING_TEST_MONGODB_URI is not set");
                return;
            }
        }
    };
}

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

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 17, 0, 0, 0).unwrap()
}

#[tokio::test]
async fn operation_survives_a_new_repository_instance() {
    let (connection, repository, database_name) = harness_or_skip!();
    let created = operation("user-1", "start-1", "hash-1");
    let operation_id = created.id();
    repository.create(created.clone()).await.unwrap();

    let second_connection = MongoConnection::connect(&uri().unwrap(), &database_name)
        .await
        .expect("reconnect to the test MongoDB");
    let second_repository = MongoOperationRepository::new(second_connection.database())
        .await
        .expect("create a second repository");

    let reloaded = second_repository.get(&operation_id).await.unwrap();
    connection.database().drop().await.unwrap();
    assert_eq!(reloaded, Some(created));
}

#[tokio::test]
async fn idempotent_create_returns_the_existing_operation() {
    let (connection, repository, _) = harness_or_skip!();
    let first = operation("user-1", "start-1", "hash-1");
    let operation_id = first.id();
    repository.create(first).await.unwrap();

    let outcome = repository
        .create(operation("user-1", "start-1", "hash-1"))
        .await
        .unwrap();
    connection.database().drop().await.unwrap();

    let CreateOperationOutcome::Existing(existing) = outcome else {
        panic!("expected the existing operation");
    };
    assert_eq!(existing.id(), operation_id);
}

#[tokio::test]
async fn idempotency_key_reuse_with_a_different_request_is_rejected() {
    let (connection, repository, _) = harness_or_skip!();
    repository
        .create(operation("user-1", "start-1", "hash-1"))
        .await
        .unwrap();

    let result = repository
        .create(operation("user-1", "start-1", "hash-2"))
        .await;
    connection.database().drop().await.unwrap();

    assert!(matches!(
        result,
        Err(RepositoryError::IdempotencyConflict { .. })
    ));
}

#[tokio::test]
async fn claim_and_completion_are_fenced() {
    let (connection, repository, _) = harness_or_skip!();
    let created = operation("user-1", "start-1", "hash-1");
    let operation_id = created.id();
    repository.create(created).await.unwrap();

    let claimed = repository
        .claim_next("agent-1", now(), Duration::from_secs(30))
        .await
        .unwrap()
        .expect("the operation is claimed");
    assert_eq!(claimed.status(), OperationStatus::Claimed);
    assert_eq!(claimed.attempt(), 1);
    assert_eq!(claimed.fencing_token(), 1);

    let blocked = repository
        .claim_next("agent-1", now(), Duration::from_secs(30))
        .await
        .unwrap();
    assert!(blocked.is_none(), "an active lease blocks a second claim");

    let reclaimed = repository
        .claim_next(
            "agent-1",
            now() + ChronoDuration::seconds(31),
            Duration::from_secs(30),
        )
        .await
        .unwrap()
        .expect("an expired lease can be reclaimed");
    assert_eq!(reclaimed.attempt(), 2);
    assert_eq!(reclaimed.fencing_token(), 2);

    repository
        .mark_running(
            &operation_id,
            "agent-1",
            2,
            now() + ChronoDuration::seconds(31),
        )
        .await
        .unwrap();
    let stale = repository
        .mark_succeeded(
            &operation_id,
            "agent-1",
            1,
            Some("stale result".to_owned()),
            now() + ChronoDuration::seconds(32),
        )
        .await;

    let succeeded = repository
        .mark_succeeded(
            &operation_id,
            "agent-1",
            2,
            Some("fresh result".to_owned()),
            now() + ChronoDuration::seconds(33),
        )
        .await
        .unwrap();
    connection.database().drop().await.unwrap();

    assert!(matches!(
        stale,
        Err(RepositoryError::Domain(
            OperationError::LeaseOwnershipMismatch
        ))
    ));
    assert_eq!(succeeded.status(), OperationStatus::Succeeded);
    assert_eq!(succeeded.result_summary(), Some("fresh result"));
}

#[tokio::test]
async fn expired_operation_becomes_terminal() {
    let (connection, repository, _) = harness_or_skip!();
    let created = operation("user-1", "start-1", "hash-1");
    let operation_id = created.id();
    repository.create(created).await.unwrap();

    let expired = repository
        .expire_due(now() + ChronoDuration::minutes(5))
        .await
        .unwrap();
    let stored = repository.get(&operation_id).await.unwrap().unwrap();
    connection.database().drop().await.unwrap();

    assert_eq!(expired, 1);
    assert_eq!(stored.status(), OperationStatus::Expired);
    assert!(stored.finished_at().is_some());
}
