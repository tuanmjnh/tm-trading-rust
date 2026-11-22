//! Live MongoDB tests for the durable AI briefing store.
//!
//! These tests run only when `TM_TRADING_TEST_MONGODB_URI` is set. A throwaway
//! database is created and dropped per test.

use chrono::{DateTime, TimeZone, Utc};
use mongodb::bson::doc;
use tm_trading_domain::{
    AiBriefing, AiBriefingReference, AiBriefingReferenceKind, AiTaskKind, NewAiBriefing,
};
use tm_trading_storage::{
    AiBriefingQuery, AiBriefingRepository, MongoAiBriefingRepository, MongoConnection,
};
use uuid::Uuid;

fn uri() -> Option<String> {
    std::env::var("TM_TRADING_TEST_MONGODB_URI")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

async fn harness() -> Option<(MongoConnection, MongoAiBriefingRepository)> {
    let uri = uri()?;
    let database_name = format!("tm-trading-test-{}", Uuid::new_v4().simple());
    let connection = MongoConnection::connect(&uri, &database_name)
        .await
        .expect("connect to the test MongoDB");
    let repository = MongoAiBriefingRepository::new(connection.database())
        .await
        .expect("create the AI briefing repository");
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
    Utc.with_ymd_and_hms(2026, 11, 21, 8, 0, 0).unwrap()
}

fn briefing(owner_scope: &str, fact: &str) -> AiBriefing {
    AiBriefing::new(NewAiBriefing {
        owner_scope: owner_scope.to_owned(),
        task: AiTaskKind::MarketBriefing,
        provider_id: "primary".to_owned(),
        model_registry_id: "primary-large".to_owned(),
        model_id: "model-a".to_owned(),
        prompt_version: "market-briefing.v1".to_owned(),
        facts: vec![fact.to_owned()],
        interpretations: vec!["Momentum may be strengthening.".to_owned()],
        uncertainties: vec!["Confirmation is missing.".to_owned()],
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
async fn a_briefing_round_trips_with_its_evidence_and_provenance() {
    let (connection, repository) = harness_or_skip!();
    let briefing = briefing("local-operator", "BTCUSDT shows a higher high.");

    repository.upsert(&briefing).await.unwrap();
    let stored = repository
        .get("local-operator", briefing.id())
        .await
        .unwrap()
        .expect("stored briefing");

    assert_eq!(stored, briefing);
    assert_eq!(stored.facts(), ["BTCUSDT shows a higher high."]);
    assert_eq!(
        stored.references()[0].kind(),
        AiBriefingReferenceKind::ResearchEvent
    );
    assert_eq!(stored.producer_version(), "tm-trading-ai-worker/0.1.0");
    assert_eq!(stored.schema_version(), "ai-briefing.v1");
    connection.database().drop().await.unwrap();
}

#[tokio::test]
async fn replaying_the_same_briefing_cannot_rewrite_published_content() {
    let (connection, repository) = harness_or_skip!();
    let first = briefing("local-operator", "Original fact");
    let replay = briefing("local-operator", "Original fact");
    assert_eq!(first.id(), replay.id());

    repository.upsert(&first).await.unwrap();
    repository.upsert(&replay).await.unwrap();

    let stored = repository
        .get("local-operator", first.id())
        .await
        .unwrap()
        .expect("stored briefing");
    assert_eq!(stored.facts(), ["Original fact"]);
    connection.database().drop().await.unwrap();
}

#[tokio::test]
async fn different_content_in_the_same_millisecond_is_stored_separately() {
    let (connection, repository) = harness_or_skip!();
    let first = briefing("local-operator", "First fact");
    let second = briefing("local-operator", "Second fact");
    assert_ne!(first.id(), second.id());

    repository.upsert(&first).await.unwrap();
    repository.upsert(&second).await.unwrap();

    let stored = repository
        .list(&AiBriefingQuery {
            owner_scope: "local-operator".to_owned(),
            task: None,
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(stored.len(), 2);
    connection.database().drop().await.unwrap();
}

#[tokio::test]
async fn briefing_reads_are_owner_scoped() {
    let (connection, repository) = harness_or_skip!();
    let briefing = briefing("local-operator", "Market fact");
    repository.upsert(&briefing).await.unwrap();

    assert!(
        repository
            .get("someone-else", briefing.id())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        repository
            .list(&AiBriefingQuery {
                owner_scope: "someone-else".to_owned(),
                task: None,
                limit: 10,
            })
            .await
            .unwrap()
            .is_empty()
    );
    connection.database().drop().await.unwrap();
}

#[tokio::test]
async fn stored_producer_and_schema_stamps_are_preserved() {
    let (connection, repository) = harness_or_skip!();
    let briefing = briefing("local-operator", "Market fact");
    repository.upsert(&briefing).await.unwrap();

    connection
        .database()
        .collection::<mongodb::bson::Document>("ai_briefings")
        .update_one(
            doc! { "_id": briefing.id() },
            doc! { "$set": {
                "producerVersion": "producer/previous",
                "schemaVersion": "ai-briefing.previous",
            } },
        )
        .await
        .unwrap();

    let stored = repository
        .get("local-operator", briefing.id())
        .await
        .unwrap()
        .expect("stored briefing");
    assert_eq!(stored.producer_version(), "producer/previous");
    assert_eq!(stored.schema_version(), "ai-briefing.previous");
    connection.database().drop().await.unwrap();
}
