use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use tm_trading_domain::{
    NotificationChannel, NotificationLogItem, NotificationOutboxItem, NotificationPriority,
    NotificationState,
};
use tm_trading_storage::{
    EnqueueNotificationOutcome, MongoConnection, MongoNotificationLogRepository,
    MongoNotificationOutboxRepository, NotificationLogRepository, NotificationOutboxRepository,
    RecordNotificationLogOutcome,
};
use uuid::Uuid;

fn uri() -> Option<String> {
    std::env::var("TM_TRADING_TEST_MONGODB_URI")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

async fn harness() -> Option<(
    MongoConnection,
    MongoNotificationOutboxRepository,
    MongoNotificationLogRepository,
)> {
    let uri = uri()?;
    let database_name = format!("tm-trading-test-{}", Uuid::new_v4().simple());
    let connection = MongoConnection::connect(&uri, &database_name)
        .await
        .expect("connect to the test MongoDB");
    let outbox_repo = MongoNotificationOutboxRepository::new(connection.database())
        .await
        .expect("create outbox repository");
    let log_repo = MongoNotificationLogRepository::new(connection.database())
        .await
        .expect("create log repository");
    Some((connection, outbox_repo, log_repo))
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
    Utc.with_ymd_and_hms(2026, 11, 23, 10, 0, 0).unwrap()
}

#[tokio::test]
async fn outbox_enqueue_idempotency_and_claim_transitions() {
    let (_connection, outbox_repo, _) = harness_or_skip!();
    let now = at();

    let item = NotificationOutboxItem::new(
        "evt_test_100".to_owned(),
        NotificationChannel::Telegram,
        "signal".to_owned(),
        NotificationPriority::Critical,
        now,
        now,
    )
    .unwrap();

    let outcome = outbox_repo.enqueue(item.clone()).await.unwrap();
    assert_eq!(outcome, EnqueueNotificationOutcome::Enqueued(item.clone()));

    // Duplicate enqueue returns existing
    let outcome2 = outbox_repo.enqueue(item.clone()).await.unwrap();
    assert_eq!(outcome2, EnqueueNotificationOutcome::Existing(item.clone()));

    // Claim
    let claimed = outbox_repo
        .claim_next("worker-test", now, Duration::from_secs(30))
        .await
        .unwrap();
    assert!(claimed.is_some());
    let claimed = claimed.unwrap();
    assert_eq!(claimed.id, item.id);

    // Mark accepted
    let accepted = outbox_repo
        .mark_accepted(&item.id, "telegram_msg_42", now)
        .await
        .unwrap();
    assert_eq!(accepted.state, NotificationState::Accepted);
    assert_eq!(accepted.message_id.as_deref(), Some("telegram_msg_42"));
}

#[tokio::test]
async fn logs_record_first_write_wins() {
    let (_connection, _, log_repo) = harness_or_skip!();
    let now = at();

    let id = NotificationLogItem::build_id("evt_test_200", now);
    let log_item = NotificationLogItem {
        id: id.clone(),
        notify_id: "evt_test_200".to_owned(),
        owner_id: Some("owner_test".to_owned()),
        dedupe_key: None,
        kind: "market".to_owned(),
        priority: NotificationPriority::Important,
        preset: None,
        name: None,
        symbol: Some("ETHUSDT".to_owned()),
        title: Some("Breakout".to_owned()),
        body: Some("Closed above resistance".to_owned()),
        at: now,
        sent_at: now,
        attempts: vec![],
        delivered: vec![NotificationChannel::Telegram],
        deduped: false,
        router_version: "notify-router.v1".to_owned(),
    };

    let outcome = log_repo.record(log_item.clone()).await.unwrap();
    assert_eq!(
        outcome,
        RecordNotificationLogOutcome::Recorded(log_item.clone())
    );

    // First write wins
    let outcome2 = log_repo.record(log_item.clone()).await.unwrap();
    assert_eq!(outcome2, RecordNotificationLogOutcome::Existing(log_item));

    let found = log_repo.get(&id).await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().symbol.as_deref(), Some("ETHUSDT"));
}
