//! Live MongoDB tests for the legacy-compatible news item store.

use chrono::{DateTime, TimeZone, Utc};
use mongodb::bson::{Bson, doc};
use tm_trading_domain::{
    NewNewsItem, NewsDirection, NewsItem, NewsItemRights, NewsLevel, news_item_id_for,
};
use tm_trading_storage::{
    MongoConnection, MongoNewsItemRepository, NewsItemQuery, NewsItemRepository,
    NewsItemUpsertOutcome,
};
use uuid::Uuid;

fn uri() -> Option<String> {
    std::env::var("TM_TRADING_TEST_MONGODB_URI")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

async fn harness() -> Option<(MongoConnection, MongoNewsItemRepository)> {
    let uri = uri()?;
    let database_name = format!("tm-trading-test-{}", Uuid::new_v4().simple());
    let connection = MongoConnection::connect(&uri, &database_name)
        .await
        .expect("connect to the test MongoDB");
    let repository = MongoNewsItemRepository::new(connection.database())
        .await
        .expect("create the news item repository");
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
    Utc.with_ymd_and_hms(2026, 11, 20, 0, 0, 0).unwrap()
}

fn item(url: &str, title: &str, ingested_at: DateTime<Utc>) -> NewsItem {
    NewsItem::new(NewNewsItem {
        source_id: "coindesk".to_owned(),
        url: url.to_owned(),
        origin_url: url.to_owned(),
        title: title.to_owned(),
        excerpt: "A short summary.".to_owned(),
        language: "en".to_owned(),
        published_at: None,
        first_seen_at: at(),
        ingested_at,
        market_group: None,
        source_market_groups: vec!["crypto".to_owned()],
        event_type: None,
        tags: vec!["bitcoin".to_owned()],
        level: NewsLevel::Med,
        score: 0.5,
        direction: NewsDirection::Flat,
        rights: NewsItemRights::default(),
    })
    .unwrap()
}

#[tokio::test]
async fn stores_the_legacy_news_items_shape() {
    let (connection, repository) = harness_or_skip!();
    let story = item(
        "https://www.coindesk.com/story?utm_source=x",
        "Bitcoin holds support",
        at(),
    );
    assert_eq!(
        repository
            .upsert_seen(std::slice::from_ref(&story))
            .await
            .expect("write story"),
        NewsItemUpsertOutcome {
            inserted: 1,
            updated: 0,
            unchanged: 0,
        }
    );

    let document = connection
        .database()
        .collection::<mongodb::bson::Document>("news_items")
        .find_one(doc! { "_id": story.item_id() })
        .await
        .expect("find story")
        .expect("story document");
    assert_eq!(document.get_str("_id").unwrap(), story.item_id());
    assert_eq!(document.get_str("sourceId").unwrap(), "coindesk");
    assert_eq!(
        document.get_str("url").unwrap(),
        "https://www.coindesk.com/story"
    );
    assert_eq!(
        document.get_str("originUrl").unwrap(),
        "https://www.coindesk.com/story?utm_source=x"
    );
    assert_eq!(document.get_str("marketGroup").unwrap(), "crypto");
    assert_eq!(document.get_str("level").unwrap(), "med");
    assert_eq!(document.get_str("dir").unwrap(), "flat");
    assert!(matches!(document.get("publishedAt"), Some(Bson::Null)));
    assert_eq!(document.get_str("schemaVersion").unwrap(), "1");
    let rights = document.get_document("rights").expect("rights snapshot");
    assert!(rights.get_bool("allowedDisplay").unwrap());
    assert!(!rights.get_bool("allowedSummarize").unwrap());
}

#[tokio::test]
async fn replaying_the_same_story_freezes_the_observation_times() {
    let (_connection, repository) = harness_or_skip!();
    let first = item("https://example.com/story", "Title", at());
    repository
        .upsert_seen(std::slice::from_ref(&first))
        .await
        .expect("insert story");

    let replay = item(
        "https://example.com/story",
        "Title",
        at() + chrono::Duration::hours(2),
    );
    assert_eq!(
        repository
            .upsert_seen(std::slice::from_ref(&replay))
            .await
            .expect("replay story"),
        NewsItemUpsertOutcome {
            inserted: 0,
            updated: 0,
            unchanged: 1,
        }
    );
    let stored = repository
        .get(first.item_id())
        .await
        .expect("read story")
        .expect("story");
    assert_eq!(stored.ingested_at(), at());
    assert_eq!(stored.updated_at(), None);

    let corrected = item(
        "https://example.com/story",
        "Corrected title",
        at() + chrono::Duration::hours(2),
    );
    assert_eq!(
        repository
            .upsert_seen(std::slice::from_ref(&corrected))
            .await
            .expect("correct story"),
        NewsItemUpsertOutcome {
            inserted: 0,
            updated: 1,
            unchanged: 0,
        }
    );
    let stored = repository
        .get(first.item_id())
        .await
        .expect("read story")
        .expect("story");
    assert_eq!(stored.title(), "Corrected title");
    assert_eq!(stored.updated_at(), Some(at() + chrono::Duration::hours(2)));
}

#[tokio::test]
async fn reads_legacy_documents_with_missing_optional_fields() {
    let (connection, repository) = harness_or_skip!();
    let story = item("https://example.com/legacy", "Legacy title", at());
    connection
        .database()
        .collection::<mongodb::bson::Document>("news_items")
        .insert_one(doc! {
            "_id": story.item_id(),
            "sourceId": "legacy",
            "url": story.url(),
            "originUrl": story.origin_url(),
            "title": "Legacy title",
            "firstSeenAt": mongodb::bson::DateTime::from_millis(at().timestamp_millis()),
            "ingestedAt": mongodb::bson::DateTime::from_millis(at().timestamp_millis()),
        })
        .await
        .expect("insert legacy document");

    let stored = repository
        .get(story.item_id())
        .await
        .expect("read legacy document")
        .expect("legacy document");
    assert_eq!(stored.excerpt(), "");
    assert_eq!(stored.language(), "en");
    assert_eq!(stored.market_group(), "crypto");
    assert_eq!(stored.level(), NewsLevel::Med);
    assert_eq!(stored.direction(), NewsDirection::Flat);
    assert_eq!(stored.score(), 0.0);
    assert!(stored.tags().is_empty());
    assert_eq!(stored.published_at(), None);
    assert_eq!(stored.updated_at(), None);
    assert!(stored.rights().allowed_display);
}

#[tokio::test]
async fn list_is_newest_first_filtered_and_bounded() {
    let (_connection, repository) = harness_or_skip!();
    let older = item("https://example.com/older", "Older", at());
    let newer = item(
        "https://example.com/newer",
        "Newer",
        at() + chrono::Duration::minutes(5),
    );
    repository
        .upsert_seen(&[older, newer.clone()])
        .await
        .expect("write stories");

    let bounded = NewsItemQuery {
        source_id: Some("coindesk".to_owned()),
        limit: 1,
        ..NewsItemQuery::default()
    };
    assert_eq!(
        repository.list(&bounded).await.expect("list stories"),
        vec![newer.clone()]
    );
    assert_eq!(
        news_item_id_for("https://example.com/newer").as_deref(),
        Some(newer.item_id())
    );
}
