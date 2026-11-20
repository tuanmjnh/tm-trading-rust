//! Live MongoDB tests for the legacy-compatible news-source registry.

use chrono::{DateTime, TimeZone, Utc};
use mongodb::bson::{Bson, doc};
use tm_trading_domain::{
    NewNewsSource, NewsHealthState, NewsLicenseStatus, NewsRights, NewsTransport, NewsTrustTier,
};
use tm_trading_storage::{
    MongoConnection, MongoNewsSourceRepository, NewsSourceRepository, RepositoryError,
};
use uuid::Uuid;

fn uri() -> Option<String> {
    std::env::var("TM_TRADING_TEST_MONGODB_URI")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

async fn harness() -> Option<(MongoConnection, MongoNewsSourceRepository)> {
    let uri = uri()?;
    let database_name = format!("tm-trading-test-{}", Uuid::new_v4().simple());
    let connection = MongoConnection::connect(&uri, &database_name)
        .await
        .expect("connect to the test MongoDB");
    let repository = MongoNewsSourceRepository::new(connection.database())
        .await
        .expect("create the news-source repository");
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

fn input(name: &str) -> NewNewsSource {
    NewNewsSource {
        source_id: "coindesk".to_owned(),
        name: name.to_owned(),
        provider: Some("CoinDesk".to_owned()),
        transport: NewsTransport::Rss,
        feed_url: Some("https://www.coindesk.com/arc/outboundfeeds/rss/".to_owned()),
        trust_tier: NewsTrustTier::T2Secondary,
        market_groups: vec!["crypto".to_owned()],
        languages: vec!["en".to_owned()],
        license_status: NewsLicenseStatus::Ok,
        rights: NewsRights {
            allowed_store_metadata: true,
            allowed_display: true,
            allowed_summarize: false,
            allowed_training: false,
        },
        enabled: true,
        poll_interval_sec: 600,
        retention_days: 7,
        created_at: at(),
    }
}

#[tokio::test]
async fn stores_the_legacy_news_sources_shape_without_secrets() {
    let (connection, repository) = harness_or_skip!();
    let source = repository
        .upsert_seed(input("CoinDesk"))
        .await
        .expect("seed source");
    assert_eq!(source.source_id(), "coindesk");

    let document = connection
        .database()
        .collection::<mongodb::bson::Document>("news_sources")
        .find_one(doc! { "_id": "coindesk" })
        .await
        .expect("find source")
        .expect("source document");

    assert_eq!(document.get_str("_id").expect("_id"), "coindesk");
    assert_eq!(document.get_str("sourceId").expect("sourceId"), "coindesk");
    assert_eq!(document.get_str("name").expect("name"), "CoinDesk");
    assert_eq!(document.get_str("provider").expect("provider"), "CoinDesk");
    assert_eq!(document.get_str("transport").expect("transport"), "rss");
    assert_eq!(
        document.get_str("feedUrl").expect("feedUrl"),
        "https://www.coindesk.com/arc/outboundfeeds/rss/"
    );
    assert_eq!(
        document.get_str("trustTier").expect("trustTier"),
        "T2_SECONDARY"
    );
    assert_eq!(
        document.get_str("licenseStatus").expect("licenseStatus"),
        "ok"
    );
    assert!(document.get_bool("enabled").expect("enabled"));
    assert_eq!(
        document.get_str("schemaVersion").expect("schemaVersion"),
        "1"
    );
    assert!(matches!(document.get("createdAt"), Some(Bson::DateTime(_))));
    assert!(matches!(document.get("updatedAt"), Some(Bson::DateTime(_))));

    let rights = document.get_document("rights").expect("rights");
    assert!(
        rights
            .get_bool("allowedStoreMetadata")
            .expect("store right")
    );
    assert!(rights.get_bool("allowedDisplay").expect("display right"));
    assert!(!rights.get_bool("allowedSummarize").expect("summary right"));
    assert!(!rights.get_bool("allowedTraining").expect("training right"));

    let health = document.get_document("health").expect("health");
    assert_eq!(health.get_str("state").expect("state"), "unknown");
    assert_eq!(health.get_i64("consecutiveErrors").expect("errors"), 0);

    for key in document.keys() {
        let key = key.to_ascii_lowercase();
        assert!(
            !key.contains("secret")
                && !key.contains("token")
                && !key.contains("password")
                && !key.contains("apikey"),
            "unexpected secret-bearing field: {key}"
        );
    }
}

#[tokio::test]
async fn reads_legacy_edge_values_and_fails_closed_on_malformed_enablement() {
    let (connection, repository) = harness_or_skip!();
    connection
        .database()
        .collection::<mongodb::bson::Document>("news_sources")
        .insert_one(doc! {
            "_id": "legacy",
            "sourceId": "legacy",
            "name": "Legacy Feed",
            "provider": "",
            "transport": "rss",
            "feedUrl": "",
            "trustTier": "T2_SECONDARY",
            "marketGroups": [],
            "languages": [],
            "licenseStatus": "unknown",
            "rights": {},
            "enabled": "false",
            "pollIntervalSec": 0_i64,
            "retentionDays": 0_i64,
            "retryRequestedAt": Bson::Null,
            "health": {},
            "schemaVersion": "1",
            "createdAt": mongodb::bson::DateTime::from_millis(at().timestamp_millis()),
            "updatedAt": mongodb::bson::DateTime::from_millis(at().timestamp_millis()),
        })
        .await
        .expect("insert legacy source");
    connection
        .database()
        .collection::<mongodb::bson::Document>("news_sources")
        .insert_one(doc! {
            "_id": "legacy-missing",
            "sourceId": "legacy-missing",
            "name": "Legacy Missing Defaults",
            "transport": "rss",
            "trustTier": "T2_SECONDARY",
            "licenseStatus": "unknown",
            "createdAt": mongodb::bson::DateTime::from_millis(at().timestamp_millis()),
            "updatedAt": mongodb::bson::DateTime::from_millis(at().timestamp_millis()),
        })
        .await
        .expect("insert legacy source with missing defaults");

    let sources = repository.list().await.expect("list legacy sources");
    assert_eq!(sources.len(), 2);
    let source = sources
        .iter()
        .find(|source| source.source_id() == "legacy")
        .expect("legacy source");
    let missing_defaults = sources
        .iter()
        .find(|source| source.source_id() == "legacy-missing")
        .expect("legacy source with missing defaults");
    assert_eq!(missing_defaults.market_groups(), ["crypto"]);
    assert_eq!(missing_defaults.languages(), ["en"]);
    assert_eq!(source.provider(), None);
    assert_eq!(source.feed_url(), None);
    assert_eq!(source.market_groups(), ["crypto"]);
    assert_eq!(source.languages(), ["en"]);
    assert_eq!(source.poll_interval_sec(), 600);
    assert_eq!(source.retention_days(), 7);
    assert!(!source.enabled());
}

#[tokio::test]
async fn health_errors_do_not_persist_secret_text() {
    let (connection, repository) = harness_or_skip!();
    repository
        .upsert_seed(input("CoinDesk"))
        .await
        .expect("seed source");
    repository
        .record_health(
            "coindesk",
            at(),
            0,
            Some("request failed: Bearer header.payload.signature"),
        )
        .await
        .expect("record failure");

    let document = connection
        .database()
        .collection::<mongodb::bson::Document>("news_sources")
        .find_one(doc! { "_id": "coindesk" })
        .await
        .expect("find source")
        .expect("source document");
    let health = document.get_document("health").expect("health");
    assert_eq!(
        health.get_str("lastError").expect("last error"),
        "fetch failed"
    );
}

#[tokio::test]
async fn updates_to_a_missing_source_do_not_create_it() {
    let (_connection, repository) = harness_or_skip!();
    assert!(matches!(
        repository
            .set_enabled("missing", true, at())
            .await
            .expect_err("missing source must fail"),
        RepositoryError::NewsSourceNotFound
    ));
    assert!(repository.list().await.expect("list sources").is_empty());
}

#[tokio::test]
async fn seeding_does_not_overwrite_policy_or_operator_settings() {
    let (connection, repository) = harness_or_skip!();
    let mut original = input("CoinDesk");
    original.provider = Some("Original Publisher".to_owned());
    original.feed_url = Some("https://example.com/original.xml".to_owned());
    original.trust_tier = NewsTrustTier::T0Primary;
    original.license_status = NewsLicenseStatus::Revoked;
    original.rights.allowed_store_metadata = false;
    original.rights.allowed_display = false;
    original.rights.allowed_summarize = true;
    original.rights.allowed_training = true;
    original.poll_interval_sec = 900;
    original.retention_days = 14;
    repository.upsert_seed(original).await.expect("seed source");
    repository
        .set_enabled("coindesk", false, at() + chrono::Duration::minutes(1))
        .await
        .expect("disable source");

    let mut changed = input("Changed by seed");
    changed.provider = Some("Replacement Publisher".to_owned());
    changed.feed_url = Some("https://example.com/replacement.xml".to_owned());
    changed.trust_tier = NewsTrustTier::T3UnverifiedSocial;
    changed.license_status = NewsLicenseStatus::Ok;
    changed.rights.allowed_store_metadata = true;
    changed.rights.allowed_display = true;
    changed.rights.allowed_summarize = false;
    changed.rights.allowed_training = false;
    changed.poll_interval_sec = 60;
    changed.retention_days = 1;
    changed.created_at = at() + chrono::Duration::minutes(2);
    let source = repository
        .upsert_seed(changed)
        .await
        .expect("seed source again");

    assert_eq!(source.name(), "CoinDesk");
    assert_eq!(source.provider(), Some("Original Publisher"));
    assert_eq!(source.feed_url(), Some("https://example.com/original.xml"));
    assert_eq!(source.trust_tier(), NewsTrustTier::T0Primary);
    assert_eq!(source.license_status(), NewsLicenseStatus::Revoked);
    assert!(!source.rights().allowed_store_metadata);
    assert!(!source.rights().allowed_display);
    assert!(source.rights().allowed_summarize);
    assert!(source.rights().allowed_training);
    assert_eq!(source.poll_interval_sec(), 900);
    assert_eq!(source.retention_days(), 14);
    assert!(!source.enabled());
    assert_eq!(source.updated_at(), at() + chrono::Duration::minutes(2));

    let document = connection
        .database()
        .collection::<mongodb::bson::Document>("news_sources")
        .find_one(doc! { "_id": "coindesk" })
        .await
        .expect("find source")
        .expect("source document");
    assert_eq!(document.get_str("name").expect("name"), "CoinDesk");
    assert!(!document.get_bool("enabled").expect("enabled"));
}

#[tokio::test]
async fn reseeding_does_not_rewind_the_update_timestamp() {
    let (_connection, repository) = harness_or_skip!();
    repository
        .upsert_seed(input("CoinDesk"))
        .await
        .expect("seed source");
    let updated_at = at() + chrono::Duration::minutes(5);
    repository
        .set_enabled("coindesk", false, updated_at)
        .await
        .expect("disable source");

    let source = repository
        .upsert_seed(input("CoinDesk"))
        .await
        .expect("reseed source");
    assert_eq!(source.updated_at(), updated_at);
}

#[tokio::test]
async fn health_writes_only_observed_fields_and_success_clears_retry() {
    let (connection, repository) = harness_or_skip!();
    repository
        .upsert_seed(input("CoinDesk"))
        .await
        .expect("seed source");

    let failed = repository
        .record_health(
            "coindesk",
            at() + chrono::Duration::minutes(1),
            0,
            Some("timeout"),
        )
        .await
        .expect("record failure");
    assert_eq!(failed.health().state, NewsHealthState::Down);
    assert_eq!(failed.health().consecutive_errors, 1);

    repository
        .request_retry("coindesk", at() + chrono::Duration::minutes(2))
        .await
        .expect("request retry");
    let healthy = repository
        .record_health("coindesk", at() + chrono::Duration::minutes(3), 2, None)
        .await
        .expect("record success");
    assert_eq!(healthy.health().state, NewsHealthState::Ok);
    assert_eq!(healthy.health().consecutive_errors, 0);
    assert_eq!(healthy.retry_requested_at(), None);

    let document = connection
        .database()
        .collection::<mongodb::bson::Document>("news_sources")
        .find_one(doc! { "_id": "coindesk" })
        .await
        .expect("find source")
        .expect("source document");
    assert_eq!(document.get_str("name").expect("name"), "CoinDesk");
    assert_eq!(
        document.get_str("feedUrl").expect("feedUrl"),
        "https://www.coindesk.com/arc/outboundfeeds/rss/"
    );
    let health = document.get_document("health").expect("health");
    assert_eq!(health.get_str("state").expect("state"), "ok");
    assert_eq!(health.get_i64("consecutiveErrors").expect("errors"), 0);
    assert!(matches!(health.get("lastError"), Some(Bson::Null)));
    assert!(matches!(health.get("lastOkAt"), Some(Bson::DateTime(_))));
    assert!(matches!(health.get("lastItemAt"), Some(Bson::DateTime(_))));
    assert!(matches!(document.get("retryRequestedAt"), Some(Bson::Null)));
}
