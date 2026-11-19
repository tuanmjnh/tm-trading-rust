//! Live MongoDB tests for the durable provider-instance store.
//!
//! These tests run only when `TM_TRADING_TEST_MONGODB_URI` is set; see
//! `mongo_repository.rs` for the requirement. A throwaway database is created and
//! dropped per test.

use chrono::{DateTime, TimeZone, Utc};
use tm_trading_storage::{
    MongoConnection, MongoProviderInstanceRepository, ProviderInstanceRepository, RepositoryError,
};
use uuid::Uuid;

fn uri() -> Option<String> {
    std::env::var("TM_TRADING_TEST_MONGODB_URI")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

async fn harness() -> Option<(MongoConnection, MongoProviderInstanceRepository)> {
    let uri = uri()?;
    let database_name = format!("tm-trading-test-{}", Uuid::new_v4().simple());
    let connection = MongoConnection::connect(&uri, &database_name)
        .await
        .expect("connect to the test MongoDB");
    let repository = MongoProviderInstanceRepository::new(connection.database())
        .await
        .expect("create the provider repository");
    Some((connection, repository))
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

fn at() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 19, 0, 0, 0).unwrap()
}

#[tokio::test]
async fn creates_reads_updates_and_deletes_an_instance() {
    let (_connection, repository) = harness_or_skip!();

    let created = repository
        .create(
            "binance",
            "Binance Public",
            "https://api.binance.com",
            5_000,
            at(),
        )
        .await
        .expect("create");
    assert_eq!(created.health_state().as_str(), "UNKNOWN");

    assert_eq!(
        repository.get(created.id()).await.expect("get"),
        Some(created.clone())
    );
    assert_eq!(
        repository.list().await.expect("list"),
        vec![created.clone()]
    );

    let edited = created
        .clone()
        .with_settings(
            "Renamed",
            "https://api.binance.com",
            9_000,
            false,
            at() + chrono::Duration::seconds(5),
        )
        .expect("edit");
    let updated = repository.update(edited).await.expect("update");
    assert_eq!(updated.display_name(), "Renamed");
    assert!(!updated.enabled());

    repository.delete(created.id()).await.expect("delete");
    assert!(repository.list().await.expect("list").is_empty());
}

#[tokio::test]
async fn recording_health_writes_only_the_health_fields() {
    let (_connection, repository) = harness_or_skip!();
    let created = repository
        .create("okx", "OKX Public", "https://www.okx.com", 4_000, at())
        .await
        .expect("create");

    let healthy = repository
        .record_health(created.id(), at(), true, Some(37), None)
        .await
        .expect("record a success");
    assert_eq!(healthy.health_state().as_str(), "HEALTHY");
    assert_eq!(healthy.last_latency_ms(), Some(37));
    assert_eq!(healthy.last_success_at(), Some(at()));
    assert_eq!(healthy.display_name(), created.display_name());
    assert_eq!(healthy.updated_at(), created.updated_at());

    let failed = repository
        .record_health(
            created.id(),
            at() + chrono::Duration::seconds(30),
            false,
            None,
            Some("timeout"),
        )
        .await
        .expect("record a failure");
    assert_eq!(failed.health_state().as_str(), "UNHEALTHY");
    assert_eq!(failed.last_error_code(), Some("timeout"));
    assert_eq!(failed.last_latency_ms(), None);
    assert_eq!(failed.last_success_at(), Some(at()));
}

#[tokio::test]
async fn persists_across_a_reconnect() {
    let (connection, repository) = harness_or_skip!();

    let created = repository
        .create(
            "binance",
            "Binance Public",
            "https://api.binance.com",
            5_000,
            at(),
        )
        .await
        .expect("create");

    let reopened = MongoProviderInstanceRepository::new(connection.database())
        .await
        .expect("reopen the repository");
    assert_eq!(
        reopened.get(created.id()).await.expect("get"),
        Some(created.clone())
    );
}

#[tokio::test]
async fn unknown_ids_are_reported_as_not_found() {
    let (_connection, repository) = harness_or_skip!();

    assert!(matches!(
        repository.delete("missing").await,
        Err(RepositoryError::ProviderInstanceNotFound)
    ));
    assert!(matches!(
        repository
            .record_health("missing", at(), true, Some(1), None)
            .await,
        Err(RepositoryError::ProviderInstanceNotFound)
    ));
}

#[tokio::test]
async fn invalid_configuration_is_rejected_before_it_is_stored() {
    let (_connection, repository) = harness_or_skip!();

    let result = repository
        .create(
            "binance",
            "Binance Public",
            "http://api.binance.com",
            5_000,
            at(),
        )
        .await;

    assert!(matches!(result, Err(RepositoryError::ProviderInstance(_))));
    assert!(repository.list().await.expect("list").is_empty());
}
