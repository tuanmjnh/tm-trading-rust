use chrono::{TimeZone, Utc};
use tm_trading_domain::{
    DEFAULT_NEWS_POLL_INTERVAL_SEC, DEFAULT_NEWS_RETENTION_DAYS, NewNewsSource, NewsHealthState,
    NewsLicenseGate, NewsLicenseStatus, NewsRights, NewsSource, NewsSourceHealth, NewsTransport,
    NewsTrustTier, StoredNewsSource,
};

fn at() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 20, 0, 0, 0).unwrap()
}

fn input() -> NewNewsSource {
    NewNewsSource {
        source_id: "coindesk".to_owned(),
        name: "CoinDesk".to_owned(),
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

fn source() -> NewsSource {
    NewsSource::new(input()).expect("valid news source")
}

#[test]
fn validates_news_source_policy_and_feed_url() {
    assert_eq!(source().source_id(), "coindesk");
    assert_eq!(source().transport(), NewsTransport::Rss);
    assert_eq!(source().trust_tier(), NewsTrustTier::T2Secondary);
    assert_eq!(source().license_status(), NewsLicenseStatus::Ok);
    assert!(source().rights().allowed_store_metadata);

    let mut insecure = input();
    insecure.feed_url = Some("http://example.com/feed".to_owned());
    assert!(NewsSource::new(insecure).is_err());
}

#[test]
fn rejects_secret_bearing_or_restricted_feed_urls() {
    for feed_url in [
        "https://user:secret@example.com/feed",
        "https://example.com/feed?api_key=secret",
        "https://example.com/feed?access_token=secret",
        "https://example.com/feed?access[token]=secret",
        "https://example.com/feed?jwt=secret",
        "https://example.com/feed?X-Amz-Credential=secret",
        "https://example.com/feed?AWSAccessKeyId=secret",
        "https://10.0.0.1/feed",
        "https://169.254.169.254/feed",
        "https://[fc00::1]/feed",
    ] {
        let mut candidate = input();
        candidate.feed_url = Some(feed_url.to_owned());
        assert!(
            NewsSource::new(candidate).is_err(),
            "feed URL should be rejected: {feed_url}"
        );
    }
}

#[test]
fn blocks_storage_when_the_source_policy_does_not_allow_it() {
    let source = source();
    assert_eq!(source.license_gate(), NewsLicenseGate::Allowed);

    let disabled = source.clone().with_enabled(false, at());
    assert_eq!(disabled.license_gate(), NewsLicenseGate::SourceDisabled);

    let mut unverified = input();
    unverified.license_status = NewsLicenseStatus::Unknown;
    let unverified = NewsSource::new(unverified).expect("valid unverified source");
    assert_eq!(
        unverified.license_gate(),
        NewsLicenseGate::LicenseNotVerified
    );

    let mut revoked = input();
    revoked.license_status = NewsLicenseStatus::Revoked;
    let revoked = NewsSource::new(revoked).expect("valid revoked source");
    assert_eq!(revoked.license_gate(), NewsLicenseGate::LicenseRevoked);

    let mut no_storage = input();
    no_storage.rights.allowed_store_metadata = false;
    let no_storage = NewsSource::new(no_storage).expect("valid restricted source");
    assert_eq!(
        no_storage.license_gate(),
        NewsLicenseGate::StorageNotPermitted
    );
}

#[test]
fn records_fetch_health_without_changing_source_policy() {
    let source = source();
    let degraded = source.clone().with_fetch_result(at(), 0, None);
    assert_eq!(degraded.health().state, NewsHealthState::Degraded);
    assert_eq!(degraded.health().consecutive_errors, 0);
    assert_eq!(degraded.health().last_fetch_at, Some(at()));

    let failed = degraded.clone().with_fetch_result(at(), 0, Some("timeout"));
    assert_eq!(failed.health().state, NewsHealthState::Down);
    assert_eq!(failed.health().consecutive_errors, 1);
    assert_eq!(failed.health().last_error.as_deref(), Some("timeout"));

    let empty_error = degraded.clone().with_fetch_result(at(), 0, Some(""));
    assert_eq!(
        empty_error.health().last_error.as_deref(),
        Some("fetch failed")
    );

    let secret_error = degraded.with_fetch_result(at(), 0, Some("Bearer header.payload.signature"));
    assert_eq!(
        secret_error.health().last_error.as_deref(),
        Some("fetch failed")
    );

    let healthy = failed.with_fetch_result(at(), 2, None);
    assert_eq!(healthy.health().state, NewsHealthState::Ok);
    assert_eq!(healthy.health().consecutive_errors, 0);
    assert_eq!(healthy.health().last_error, None);
    assert_eq!(healthy.health().last_item_at, Some(at()));
    assert_eq!(healthy.license_status(), NewsLicenseStatus::Ok);
}

#[test]
fn reads_legacy_edge_values_without_hiding_the_registry() {
    let source = NewsSource::from_stored(StoredNewsSource {
        source_id: "legacy".to_owned(),
        name: "Legacy Feed".to_owned(),
        provider: Some(String::new()),
        transport: NewsTransport::Rss,
        feed_url: Some(String::new()),
        trust_tier: NewsTrustTier::T2Secondary,
        market_groups: Vec::new(),
        languages: Vec::new(),
        license_status: NewsLicenseStatus::Unknown,
        rights: NewsRights {
            allowed_store_metadata: false,
            allowed_display: false,
            allowed_summarize: false,
            allowed_training: false,
        },
        enabled: true,
        poll_interval_sec: 0,
        retention_days: 0,
        retry_requested_at: None,
        health: NewsSourceHealth::default(),
        created_at: at(),
        updated_at: at(),
    })
    .expect("legacy source remains readable");

    assert_eq!(source.provider(), None);
    assert_eq!(source.feed_url(), None);
    assert_eq!(source.market_groups(), ["crypto"]);
    assert_eq!(source.languages(), ["en"]);
    assert_eq!(source.poll_interval_sec(), DEFAULT_NEWS_POLL_INTERVAL_SEC);
    assert_eq!(source.retention_days(), DEFAULT_NEWS_RETENTION_DAYS);
}

#[test]
fn disables_a_legacy_source_with_an_unsafe_feed_url() {
    let source = NewsSource::from_stored(StoredNewsSource {
        source_id: "legacy".to_owned(),
        name: "Legacy Feed".to_owned(),
        provider: None,
        transport: NewsTransport::Rss,
        feed_url: Some("https://example.com/feed?jwt=secret".to_owned()),
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
        retry_requested_at: None,
        health: NewsSourceHealth::default(),
        created_at: at(),
        updated_at: at(),
    })
    .expect("legacy source remains visible");

    assert_eq!(source.feed_url(), None);
    assert!(!source.enabled());
}
