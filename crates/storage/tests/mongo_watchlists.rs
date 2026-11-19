//! Live MongoDB tests for the durable watchlist store.
//!
//! These tests run only when `TM_TRADING_TEST_MONGODB_URI` is set; see
//! `mongo_repository.rs` for the requirement. A throwaway database is created and
//! dropped per test.

use chrono::{DateTime, TimeZone, Utc};
use mongodb::bson::{Bson, Document, doc};
use tm_trading_storage::{
    MongoConnection, MongoWatchlistRepository, RepositoryError, WatchlistRepository,
};
use uuid::Uuid;

fn uri() -> Option<String> {
    std::env::var("TM_TRADING_TEST_MONGODB_URI")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

async fn harness() -> Option<(MongoConnection, MongoWatchlistRepository)> {
    let uri = uri()?;
    let database_name = format!("tm-trading-test-{}", Uuid::new_v4().simple());
    let connection = MongoConnection::connect(&uri, &database_name)
        .await
        .expect("connect to the test MongoDB");
    let repository = MongoWatchlistRepository::new(connection.database())
        .await
        .expect("create the watchlist repository");
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

fn symbols(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[tokio::test]
async fn creates_lists_and_deletes_a_watchlist() {
    let (_connection, repository) = harness_or_skip!();

    let created = repository
        .create("owner-a", "Core", &symbols(&["btcusdt", "ETHUSDT"]), at())
        .await
        .expect("create");
    assert!(created.is_default());
    assert_eq!(created.symbols(), ["BTCUSDT", "ETHUSDT"]);

    let listed = repository.list("owner-a").await.expect("list");
    assert_eq!(listed, vec![created.clone()]);

    repository
        .delete("owner-a", created.id())
        .await
        .expect("delete");
    assert!(repository.list("owner-a").await.expect("list").is_empty());
}

#[tokio::test]
async fn persists_across_a_reconnect() {
    let (connection, repository) = harness_or_skip!();

    let created = repository
        .create("owner-a", "Core", &symbols(&["BTCUSDT"]), at())
        .await
        .expect("create");
    drop(repository);

    let reopened = MongoWatchlistRepository::new(connection.database())
        .await
        .expect("reopen");
    assert_eq!(reopened.list("owner-a").await.expect("list"), vec![created]);
}

#[tokio::test]
async fn rejects_a_duplicate_name_case_insensitively() {
    let (_connection, repository) = harness_or_skip!();

    repository
        .create("owner-a", "Core", &symbols(&["BTCUSDT"]), at())
        .await
        .expect("first");
    let duplicate = repository
        .create("owner-a", "core", &symbols(&["ETHUSDT"]), at())
        .await;

    assert!(matches!(
        duplicate,
        Err(RepositoryError::DuplicateWatchlistName)
    ));
}

#[tokio::test]
async fn one_owner_cannot_touch_another_owners_list() {
    let (_connection, repository) = harness_or_skip!();

    let created = repository
        .create("owner-a", "Core", &symbols(&["BTCUSDT"]), at())
        .await
        .expect("create");

    let replace = repository
        .replace_symbols("owner-b", created.id(), &symbols(&["ETHUSDT"]), at())
        .await;
    let delete = repository.delete("owner-b", created.id()).await;

    assert!(matches!(replace, Err(RepositoryError::WatchlistNotFound)));
    assert!(matches!(delete, Err(RepositoryError::WatchlistNotFound)));
}

#[tokio::test]
async fn replaces_symbols_and_keeps_the_legacy_document_shape() {
    let (connection, repository) = harness_or_skip!();

    let created = repository
        .create("owner-a", "Core", &symbols(&["BTCUSDT"]), at())
        .await
        .expect("create");
    let later = at() + chrono::Duration::seconds(30);
    let updated = repository
        .replace_symbols("owner-a", created.id(), &symbols(&["ethusdt"]), later)
        .await
        .expect("replace");

    assert_eq!(updated.symbols(), ["ETHUSDT"]);
    assert_eq!(updated.updated_at(), later);

    let raw = connection
        .database()
        .collection::<Document>("watchlists")
        .find_one(doc! { "ownerId": "owner-a" })
        .await
        .expect("find")
        .expect("row");
    assert!(matches!(raw.get("_id"), Some(Bson::ObjectId(_))));
    assert!(matches!(raw.get("createdAt"), Some(Bson::DateTime(_))));
    assert_eq!(raw.get_array("symbols").expect("symbols").len(), 1);
}
