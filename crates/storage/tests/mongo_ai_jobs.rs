//! Live MongoDB tests for the durable AI job queue and atomic briefing publication.

use chrono::{Duration as ChronoDuration, Utc};
use mongodb::bson::{DateTime as BsonDateTime, Document, doc};
use tm_trading_domain::{
    AiBriefing, AiBriefingReference, AiBriefingReferenceKind, AiJobStatus, AiTaskKind,
    NewAiBriefing, NewAiJob,
};
use tm_trading_storage::{
    AiBriefingRepository, AiJobRepository, MongoAiBriefingRepository, MongoAiJobRepository,
    MongoConnection, RepositoryError,
};
use uuid::Uuid;

fn uri() -> Option<String> {
    std::env::var("TM_TRADING_TEST_MONGODB_URI")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

async fn harness() -> (
    MongoConnection,
    MongoAiJobRepository,
    MongoAiBriefingRepository,
) {
    let uri = uri().expect("TM_TRADING_TEST_MONGODB_URI must be set for live tests");
    let database_name = format!("tm-trading-test-{}", Uuid::new_v4().simple());
    let connection = MongoConnection::connect(&uri, &database_name)
        .await
        .expect("connect to the test MongoDB");
    let jobs = MongoAiJobRepository::new(connection.database())
        .await
        .expect("create the AI job repository");
    let briefings = MongoAiBriefingRepository::new(connection.database())
        .await
        .expect("create the AI briefing repository");
    (connection, jobs, briefings)
}

fn new_job(idempotency_key: &str) -> tm_trading_domain::AiJob {
    tm_trading_domain::AiJob::new(NewAiJob {
        owner_scope: "local-operator".to_owned(),
        idempotency_key: idempotency_key.to_owned(),
        request_hash: format!("hash-{idempotency_key}"),
        correlation_id: Uuid::new_v4(),
        task: AiTaskKind::ResearchExplanation,
        research_event_ids: vec!["rev1_0123456789abcdef".to_owned()],
        news_item_ids: Vec::new(),
        requested_at: Utc::now(),
        expires_at: Utc::now() + ChronoDuration::hours(1),
    })
    .unwrap()
}

fn new_briefing() -> AiBriefing {
    AiBriefing::new(NewAiBriefing {
        owner_scope: "local-operator".to_owned(),
        task: AiTaskKind::ResearchExplanation,
        provider_id: "primary".to_owned(),
        model_registry_id: "primary-large".to_owned(),
        model_id: "model-a".to_owned(),
        prompt_version: "research-explanation.v1".to_owned(),
        facts: vec!["Research fact".to_owned()],
        interpretations: vec!["Momentum may be strengthening.".to_owned()],
        uncertainties: Vec::new(),
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
        generated_at: Utc::now(),
    })
    .unwrap()
}

#[tokio::test]
#[ignore = "requires TM_TRADING_TEST_MONGODB_URI"]
async fn a_successful_completion_publishes_the_briefing_and_job_together() {
    let (connection, jobs, briefings) = harness().await;
    jobs.create(new_job("success")).await.unwrap();
    let claimed = jobs
        .claim_next("ai-worker", Utc::now(), std::time::Duration::from_secs(300))
        .await
        .unwrap()
        .unwrap();
    let briefing = new_briefing();

    let completed = jobs
        .complete_with_briefing(
            &claimed.id(),
            "ai-worker",
            claimed.fencing_token(),
            &briefing,
            Utc::now(),
        )
        .await
        .unwrap();
    let published = briefings
        .get("local-operator", briefing.id())
        .await
        .unwrap();
    connection.database().drop().await.unwrap();

    assert_eq!(completed.status(), AiJobStatus::Succeeded);
    assert_eq!(completed.briefing_id(), Some(briefing.id()));
    assert!(completed.finished_at().is_some());
    assert_eq!(
        published.as_ref().map(|stored| stored.id()),
        Some(briefing.id())
    );
}

#[tokio::test]
#[ignore = "requires TM_TRADING_TEST_MONGODB_URI"]
async fn a_briefing_conflict_rolls_back_the_job_completion() {
    let (connection, jobs, _briefings) = harness().await;
    jobs.create(new_job("conflict")).await.unwrap();
    let claimed = jobs
        .claim_next("ai-worker", Utc::now(), std::time::Duration::from_secs(300))
        .await
        .unwrap()
        .unwrap();
    let briefing = new_briefing();
    connection
        .database()
        .collection::<Document>("ai_briefings")
        .insert_one(doc! { "_id": briefing.id(), "conflicting": true })
        .await
        .unwrap();

    let result = jobs
        .complete_with_briefing(
            &claimed.id(),
            "ai-worker",
            claimed.fencing_token(),
            &briefing,
            Utc::now(),
        )
        .await;
    let stored = jobs.get(&claimed.id()).await.unwrap().unwrap();
    connection.database().drop().await.unwrap();

    assert!(matches!(result, Err(RepositoryError::AiBriefingConflict)));
    assert_eq!(stored.status(), AiJobStatus::Claimed);
    assert!(stored.briefing_id().is_none());
}

#[tokio::test]
#[ignore = "requires TM_TRADING_TEST_MONGODB_URI"]
async fn the_database_clock_decides_a_lapsed_lease() {
    let (connection, jobs, briefings) = harness().await;
    jobs.create(new_job("lapsed")).await.unwrap();
    let claimed = jobs
        .claim_next("ai-worker", Utc::now(), std::time::Duration::from_secs(300))
        .await
        .unwrap()
        .unwrap();
    let past =
        BsonDateTime::from_millis((Utc::now() - ChronoDuration::minutes(1)).timestamp_millis());
    connection
        .database()
        .collection::<Document>("ai_jobs")
        .update_one(
            doc! { "_id": claimed.id().to_string() },
            doc! { "$set": { "leaseExpiresAt": past } },
        )
        .await
        .unwrap();
    let briefing = new_briefing();

    let completed = jobs
        .complete_with_briefing(
            &claimed.id(),
            "ai-worker",
            claimed.fencing_token(),
            &briefing,
            Utc::now(),
        )
        .await
        .unwrap();
    let published = briefings
        .get("local-operator", briefing.id())
        .await
        .unwrap();
    connection.database().drop().await.unwrap();

    assert_eq!(completed.status(), AiJobStatus::OutcomeUnknown);
    assert_eq!(completed.error_code(), Some("lease_lapsed"));
    assert!(published.is_none());
}

#[tokio::test]
#[ignore = "requires TM_TRADING_TEST_MONGODB_URI"]
async fn the_database_clock_expires_a_queued_job() {
    let (connection, jobs, _briefings) = harness().await;
    let job = new_job("expired");
    jobs.create(job.clone()).await.unwrap();
    let past =
        BsonDateTime::from_millis((Utc::now() - ChronoDuration::minutes(1)).timestamp_millis());
    connection
        .database()
        .collection::<Document>("ai_jobs")
        .update_one(
            doc! { "_id": job.id().to_string() },
            doc! { "$set": { "expiresAt": past } },
        )
        .await
        .unwrap();

    let closed = jobs
        .expire_due(Utc::now() - ChronoDuration::hours(1))
        .await
        .unwrap();
    let stored = jobs.get(&job.id()).await.unwrap().unwrap();
    connection.database().drop().await.unwrap();

    assert_eq!(closed, 1);
    assert_eq!(stored.status(), AiJobStatus::Expired);
}

#[tokio::test]
#[ignore = "requires TM_TRADING_TEST_MONGODB_URI"]
async fn a_stale_worker_clock_cannot_claim_an_expired_job() {
    let (connection, jobs, _briefings) = harness().await;
    let job = new_job("stale-clock");
    jobs.create(job.clone()).await.unwrap();
    connection
        .database()
        .collection::<Document>("ai_jobs")
        .update_one(
            doc! { "_id": job.id().to_string() },
            doc! { "$set": { "expiresAt": BsonDateTime::from_millis(0) } },
        )
        .await
        .unwrap();

    let claimed = jobs
        .claim_next(
            "ai-worker",
            Utc::now() - ChronoDuration::hours(2),
            std::time::Duration::from_secs(300),
        )
        .await
        .unwrap();
    let stored = jobs.get(&job.id()).await.unwrap().unwrap();
    connection.database().drop().await.unwrap();

    assert!(claimed.is_none());
    assert_eq!(stored.status(), AiJobStatus::Expired);
}
