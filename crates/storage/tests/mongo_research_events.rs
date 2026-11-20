//! Live MongoDB tests for the durable research event store.
//!
//! These tests run only when `TM_TRADING_TEST_MONGODB_URI` is set; see
//! `mongo_candles.rs` for the requirement. A throwaway database is created and
//! dropped per test.

use chrono::{DateTime, TimeZone, Utc};
use mongodb::bson::doc;
use std::collections::BTreeMap;
use tm_trading_domain::{NewResearchEvent, ResearchEvent, ResearchQuality};
use tm_trading_storage::{
    MongoConnection, MongoResearchEventRepository, ResearchEventQuery, ResearchEventRepository,
};
use uuid::Uuid;

fn uri() -> Option<String> {
    std::env::var("TM_TRADING_TEST_MONGODB_URI")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

async fn harness() -> Option<(MongoConnection, MongoResearchEventRepository)> {
    let uri = uri()?;
    let database_name = format!("tm-trading-test-{}", Uuid::new_v4().simple());
    let connection = MongoConnection::connect(&uri, &database_name)
        .await
        .expect("connect to the test MongoDB");
    let repository = MongoResearchEventRepository::new(connection.database())
        .await
        .expect("create the research event repository");
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

fn at(minute: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 20, 8, minute as u32, 0)
        .unwrap()
}

fn event(symbol: &str, event_type: &str, minute: i64) -> ResearchEvent {
    ResearchEvent::new(NewResearchEvent {
        owner_scope: "local-operator".to_owned(),
        symbol: symbol.to_owned(),
        timeframe: "15m".to_owned(),
        method: "vsa".to_owned(),
        method_version: "1".to_owned(),
        event_type: event_type.to_owned(),
        direction: 1,
        score: 0.5,
        observed_at: at(minute),
        detected_at: at(minute),
        ingested_at: at(minute),
        source_id: "binance".to_owned(),
        provider_method: "binance.klines".to_owned(),
        input_snapshot_id: "rsnap1_0123456789abcdef".to_owned(),
        quality_state: ResearchQuality::Complete,
        quality_reasons: vec!["fixture".to_owned()],
        evidence: BTreeMap::from([("close".to_owned(), 100.5)]),
        context: BTreeMap::from([("bucket".to_owned(), "TIM".to_owned())]),
    })
    .expect("build a valid research event")
}

#[tokio::test]
async fn re_recording_the_same_event_is_an_upsert() {
    let (connection, repository) = harness_or_skip!();
    let event = event("BTCUSDT", "ST LONG", 0);

    let first = repository
        .upsert(std::slice::from_ref(&event))
        .await
        .unwrap();
    let second = repository
        .upsert(std::slice::from_ref(&event))
        .await
        .unwrap();

    assert_eq!(first.inserted, 1);
    assert_eq!(second.existing, 1);

    let stored = repository
        .get(event.id())
        .await
        .unwrap()
        .expect("stored event");
    assert_eq!(stored, event);
    connection.database().drop().await.unwrap();
}

#[tokio::test]
async fn events_are_read_back_with_their_provenance() {
    let (connection, repository) = harness_or_skip!();
    let events = vec![
        event("BTCUSDT", "ST LONG", 0),
        event("BTCUSDT", "BC", 15),
        event("ETHUSDT", "SV", 30),
    ];
    repository.upsert(&events).await.unwrap();

    let all = repository
        .list(&ResearchEventQuery::default())
        .await
        .unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].event_type(), "SV");
    assert_eq!(all[0].evidence()["close"], 100.5);
    assert_eq!(all[0].context()["bucket"], "TIM");
    assert_eq!(all[0].quality_reasons(), ["fixture"]);

    let filtered = repository
        .list(&ResearchEventQuery {
            symbol: Some("BTCUSDT".to_owned()),
            ..ResearchEventQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(filtered.len(), 2);

    let foreign = repository
        .list(&ResearchEventQuery {
            owner_scope: Some("someone-else".to_owned()),
            ..ResearchEventQuery::default()
        })
        .await
        .unwrap();
    assert!(foreign.is_empty());

    connection.database().drop().await.unwrap();
}

#[tokio::test]
async fn stored_producer_and_schema_stamps_are_preserved() {
    let (connection, repository) = harness_or_skip!();
    let event = event("BTCUSDT", "ST LONG", 0);
    repository
        .upsert(std::slice::from_ref(&event))
        .await
        .unwrap();

    connection
        .database()
        .collection::<mongodb::bson::Document>("research_events")
        .update_one(
            doc! { "_id": event.id() },
            doc! { "$set": {
                "producerVersion": "producer/previous",
                "schemaVersion": "research-event.previous",
            } },
        )
        .await
        .unwrap();

    let stored = repository
        .get(event.id())
        .await
        .unwrap()
        .expect("stored event");
    assert_eq!(stored.producer_version(), "producer/previous");
    assert_eq!(stored.schema_version(), "research-event.previous");
    connection.database().drop().await.unwrap();
}

#[tokio::test]
async fn an_unknown_event_is_not_found() {
    let (connection, repository) = harness_or_skip!();
    assert!(repository.get("rev1_missing").await.unwrap().is_none());
    connection.database().drop().await.unwrap();
}
