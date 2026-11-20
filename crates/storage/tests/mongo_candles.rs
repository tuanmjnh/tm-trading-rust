//! Live MongoDB tests for the durable closed-candle store.
//!
//! These tests run only when `TM_TRADING_TEST_MONGODB_URI` is set; see
//! `mongo_repository.rs` for the requirement. A throwaway database is created and
//! dropped per test.

use mongodb::bson::doc;
use tm_trading_domain::{Candle, NewCandle};
use tm_trading_storage::{
    CandleRepository, MongoCandleRepository, MongoConnection, RepositoryError,
};
use uuid::Uuid;

fn uri() -> Option<String> {
    std::env::var("TM_TRADING_TEST_MONGODB_URI")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

async fn harness() -> Option<(MongoConnection, MongoCandleRepository)> {
    let uri = uri()?;
    let database_name = format!("tm-trading-test-{}", Uuid::new_v4().simple());
    let connection = MongoConnection::connect(&uri, &database_name)
        .await
        .expect("connect to the test MongoDB");
    let repository = MongoCandleRepository::new(connection.database())
        .await
        .expect("create the candle repository");
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

fn candle(open_time: i64, close: f64) -> Candle {
    Candle::new(NewCandle {
        symbol: "BTCUSDT".to_owned(),
        timeframe: "1m".to_owned(),
        market: Some("spot".to_owned()),
        source: "binance".to_owned(),
        open: 100.0,
        high: 110.0,
        low: 90.0,
        close,
        volume: 12.5,
        open_time,
        close_time: open_time + 59_999,
        event_time: None,
        ingest_time: None,
    })
    .expect("build a valid candle")
}

#[tokio::test]
async fn re_recording_the_same_close_is_an_upsert() {
    let (connection, repository) = harness_or_skip!();
    let bar = candle(1_700_000_000_000, 105.0);
    let first = repository
        .upsert_closed(std::slice::from_ref(&bar))
        .await
        .unwrap();
    let second = repository
        .upsert_closed(std::slice::from_ref(&bar))
        .await
        .unwrap();
    let stored = repository.recent("BTCUSDT", "1m", 10).await.unwrap();
    connection.database().drop().await.unwrap();

    assert_eq!((first.inserted, first.existing), (1, 0));
    assert_eq!((second.inserted, second.existing), (0, 1));
    assert_eq!(stored, vec![bar]);
}

#[tokio::test]
async fn rows_survive_a_new_repository_instance() {
    let (connection, repository) = harness_or_skip!();
    let bar = candle(1_700_000_000_000, 105.0);
    repository
        .upsert_closed(std::slice::from_ref(&bar))
        .await
        .unwrap();

    let second_connection = MongoConnection::connect(&uri().unwrap(), connection.database().name())
        .await
        .expect("reconnect to the test MongoDB");
    let second_repository = MongoCandleRepository::new(second_connection.database())
        .await
        .expect("create a second repository");
    let stored = second_repository.get(bar.id()).await.unwrap();
    connection.database().drop().await.unwrap();

    assert_eq!(stored, Some(bar));
}

#[tokio::test]
async fn recent_is_newest_first_and_bounded() {
    let (connection, repository) = harness_or_skip!();
    let older = candle(1_700_000_000_000, 105.0);
    let newer = candle(1_700_000_060_000, 106.0);
    repository
        .upsert_closed(&[older.clone(), newer.clone()])
        .await
        .unwrap();
    let stored = repository.recent("BTCUSDT", "1m", 1).await.unwrap();
    connection.database().drop().await.unwrap();

    assert_eq!(stored, vec![newer]);
}

#[tokio::test]
async fn a_failed_batch_rolls_back_every_candle() {
    let (connection, repository) = harness_or_skip!();
    let valid = candle(1_700_000_000_000, 105.0);
    let invalid = Candle::new(NewCandle {
        symbol: "ETHUSDT".to_owned(),
        timeframe: "1m".to_owned(),
        market: Some("spot".to_owned()),
        source: "binance".to_owned(),
        open: 100.0,
        high: 110.0,
        low: 90.0,
        close: 105.0,
        volume: 12.5,
        open_time: 1_700_000_060_000,
        close_time: 1_700_000_119_999,
        event_time: None,
        ingest_time: Some(i64::MAX),
    })
    .expect("build a candle with an invalid ingestion timestamp");

    let result = repository.upsert_closed(&[valid.clone(), invalid]).await;
    let stored = repository.recent("BTCUSDT", "1m", 10).await.unwrap();
    connection.database().drop().await.unwrap();

    assert!(matches!(
        result,
        Err(RepositoryError::CorruptRecord("ingestTime"))
    ));
    assert!(stored.is_empty());
}

#[tokio::test]
async fn a_legacy_row_without_a_schema_stamp_is_readable() {
    let (connection, repository) = harness_or_skip!();
    let collection = connection
        .database()
        .collection::<mongodb::bson::Document>("candles");
    collection
        .insert_one(doc! {
            "_id": "ETHUSDT:5m:1700000000000",
            "symbol": "ETHUSDT",
            "timeframe": "5m",
            "source": "okx",
            "state": "closed",
            "open": 2000.5,
            "high": 2010,
            "low": 1990,
            "close": 2005,
            "openTime": 1_700_000_000_000_i64,
            "closeTime": 1_700_000_299_999_i64,
        })
        .await
        .unwrap();

    let stored = repository.get("ETHUSDT:5m:1700000000000").await.unwrap();
    connection.database().drop().await.unwrap();

    let stored = stored.expect("legacy row is readable");
    assert_eq!(stored.symbol(), "ETHUSDT");
    assert_eq!(stored.market(), None);
    assert_eq!(stored.volume(), 0.0);
    assert_eq!(stored.event_time(), 1_700_000_299_999);
}

#[tokio::test]
async fn a_row_whose_id_disagrees_with_its_fields_is_rejected() {
    let (connection, repository) = harness_or_skip!();
    let collection = connection
        .database()
        .collection::<mongodb::bson::Document>("candles");
    collection
        .insert_one(doc! {
            "_id": "BTCUSDT:1m:999",
            "symbol": "BTCUSDT",
            "timeframe": "1m",
            "source": "binance",
            "state": "closed",
            "open": 100.0,
            "high": 110.0,
            "low": 90.0,
            "close": 105.0,
            "volume": 1.0,
            "openTime": 1_700_000_000_000_i64,
            "closeTime": 1_700_000_059_999_i64,
        })
        .await
        .unwrap();

    let result = repository.get("BTCUSDT:1m:999").await;
    connection.database().drop().await.unwrap();

    assert!(matches!(result, Err(RepositoryError::CorruptRecord("_id"))));
}
