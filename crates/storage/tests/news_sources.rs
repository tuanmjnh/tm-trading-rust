use chrono::{DateTime, TimeZone, Utc};
use tm_trading_domain::{
    NewNewsSource, NewsHealthState, NewsLicenseStatus, NewsRights, NewsTransport, NewsTrustTier,
};
use tm_trading_storage::{InMemoryNewsSourceRepository, NewsSourceRepository, RepositoryError};

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
async fn seeding_preserves_policy_and_operator_edits() {
    let repository = InMemoryNewsSourceRepository::new();
    let first = repository
        .upsert_seed(input("CoinDesk"))
        .await
        .expect("seed source");
    assert_eq!(first.name(), "CoinDesk");

    let disabled = repository
        .set_enabled("coindesk", false, at() + chrono::Duration::minutes(1))
        .await
        .expect("disable source");
    assert!(!disabled.enabled());

    let mut changed_seed = input("Changed by seed");
    changed_seed.created_at = at() + chrono::Duration::minutes(2);
    let seeded_again = repository
        .upsert_seed(changed_seed)
        .await
        .expect("seed source again");

    assert_eq!(seeded_again.name(), "CoinDesk");
    assert!(!seeded_again.enabled());
    assert_eq!(
        seeded_again.updated_at(),
        at() + chrono::Duration::minutes(2)
    );
}

#[tokio::test]
async fn seeding_preserves_all_legacy_policy_fields() {
    let repository = InMemoryNewsSourceRepository::new();
    let mut original = input("Original");
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
    repository
        .upsert_seed(original)
        .await
        .expect("seed original policy");

    let mut replacement = input("Replacement");
    replacement.provider = Some("Replacement Publisher".to_owned());
    replacement.feed_url = Some("https://example.com/replacement.xml".to_owned());
    replacement.trust_tier = NewsTrustTier::T3UnverifiedSocial;
    replacement.license_status = NewsLicenseStatus::Ok;
    replacement.rights.allowed_store_metadata = true;
    replacement.rights.allowed_display = true;
    replacement.rights.allowed_summarize = false;
    replacement.rights.allowed_training = false;
    replacement.poll_interval_sec = 60;
    replacement.retention_days = 1;
    let source = repository
        .upsert_seed(replacement)
        .await
        .expect("reseed source");

    assert_eq!(source.name(), "Original");
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
}

#[tokio::test]
async fn reseeding_does_not_rewind_the_update_timestamp() {
    let repository = InMemoryNewsSourceRepository::new();
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
async fn health_updates_do_not_change_source_policy() {
    let repository = InMemoryNewsSourceRepository::new();
    let source = repository
        .upsert_seed(input("CoinDesk"))
        .await
        .expect("seed source");

    repository
        .record_health("coindesk", at(), 0, None)
        .await
        .expect("record initial empty success");

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
    assert_eq!(failed.health().last_ok_at, Some(at()));
    assert_eq!(failed.health().last_item_at, None);
    assert_eq!(failed.name(), source.name());
    assert_eq!(failed.feed_url(), source.feed_url());

    let degraded = repository
        .record_health("coindesk", at() + chrono::Duration::minutes(2), 0, None)
        .await
        .expect("record empty success");
    assert_eq!(degraded.health().state, NewsHealthState::Degraded);
    assert_eq!(degraded.health().consecutive_errors, 0);

    let healthy = repository
        .record_health("coindesk", at() + chrono::Duration::minutes(3), 2, None)
        .await
        .expect("record success");
    assert_eq!(healthy.health().state, NewsHealthState::Ok);
    assert_eq!(healthy.health().consecutive_errors, 0);
    assert_eq!(healthy.health().last_error, None);
    assert_eq!(
        healthy.health().last_item_at,
        Some(at() + chrono::Duration::minutes(3))
    );
    assert_eq!(healthy.name(), source.name());
}

#[tokio::test]
async fn a_successful_fetch_clears_a_manual_retry_request() {
    let repository = InMemoryNewsSourceRepository::new();
    repository
        .upsert_seed(input("CoinDesk"))
        .await
        .expect("seed source");

    let retrying = repository
        .request_retry("coindesk", at() + chrono::Duration::minutes(1))
        .await
        .expect("request retry");
    assert_eq!(
        retrying.retry_requested_at(),
        Some(at() + chrono::Duration::minutes(1))
    );

    let healthy = repository
        .record_health("coindesk", at() + chrono::Duration::minutes(2), 1, None)
        .await
        .expect("record success");
    assert_eq!(healthy.retry_requested_at(), None);
}

#[tokio::test]
async fn health_errors_do_not_persist_secret_text() {
    let repository = InMemoryNewsSourceRepository::new();
    repository
        .upsert_seed(input("CoinDesk"))
        .await
        .expect("seed source");

    let failed = repository
        .record_health(
            "coindesk",
            at(),
            0,
            Some("request failed: Bearer header.payload.signature"),
        )
        .await
        .expect("record failure");
    assert_eq!(failed.health().last_error.as_deref(), Some("fetch failed"));
}

#[tokio::test]
async fn updates_to_a_missing_source_do_not_create_it() {
    let repository = InMemoryNewsSourceRepository::new();
    assert!(matches!(
        repository
            .set_enabled("missing", true, at())
            .await
            .expect_err("missing source must fail"),
        RepositoryError::NewsSourceNotFound
    ));
    assert!(matches!(
        repository
            .record_health("missing", at(), 0, Some("timeout"))
            .await
            .expect_err("missing source must fail"),
        RepositoryError::NewsSourceNotFound
    ));
    assert!(matches!(
        repository
            .request_retry("missing", at())
            .await
            .expect_err("missing source must fail"),
        RepositoryError::NewsSourceNotFound
    ));
    assert!(repository.list().await.expect("list sources").is_empty());
}
