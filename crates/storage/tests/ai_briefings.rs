use chrono::{DateTime, TimeZone, Utc};
use tm_trading_domain::{
    AiBriefing, AiBriefingReference, AiBriefingReferenceKind, AiTaskKind, NewAiBriefing,
};
use tm_trading_storage::{AiBriefingQuery, AiBriefingRepository, InMemoryAiBriefingRepository};

fn at(minute: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 21, 8, minute as u32, 0)
        .unwrap()
}

fn briefing(owner: &str, task: AiTaskKind, minute: i64, fact: &str) -> AiBriefing {
    AiBriefing::new(NewAiBriefing {
        owner_scope: owner.to_owned(),
        task,
        provider_id: "primary".to_owned(),
        model_registry_id: "primary-large".to_owned(),
        model_id: "model-a".to_owned(),
        prompt_version: "market-briefing.v1".to_owned(),
        facts: vec![fact.to_owned()],
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
        generated_at: at(minute),
    })
    .unwrap()
}

#[tokio::test]
async fn briefings_are_owner_scoped_and_task_filtered() {
    let repository = InMemoryAiBriefingRepository::new();
    repository
        .upsert(&briefing(
            "owner-a",
            AiTaskKind::MarketBriefing,
            0,
            "Market fact",
        ))
        .await
        .unwrap();
    repository
        .upsert(&briefing(
            "owner-a",
            AiTaskKind::NewsSummary,
            15,
            "News fact",
        ))
        .await
        .unwrap();
    repository
        .upsert(&briefing(
            "owner-b",
            AiTaskKind::MarketBriefing,
            30,
            "Foreign fact",
        ))
        .await
        .unwrap();

    let list = repository
        .list(&AiBriefingQuery {
            owner_scope: "owner-a".to_owned(),
            task: Some(AiTaskKind::MarketBriefing),
            limit: 10,
        })
        .await
        .unwrap();

    assert_eq!(list.len(), 1);
    assert_eq!(list[0].facts(), ["Market fact"]);
}

#[tokio::test]
async fn a_foreign_briefing_is_not_readable_by_identifier() {
    let repository = InMemoryAiBriefingRepository::new();
    let briefing = briefing("owner-a", AiTaskKind::MarketBriefing, 0, "Market fact");
    repository.upsert(&briefing).await.unwrap();

    assert!(
        repository
            .get("owner-b", briefing.id())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn replaying_the_same_briefing_keeps_the_published_content() {
    let repository = InMemoryAiBriefingRepository::new();
    let first = briefing("owner-a", AiTaskKind::MarketBriefing, 0, "Original fact");
    let replay = briefing("owner-a", AiTaskKind::MarketBriefing, 0, "Original fact");
    assert_eq!(first.id(), replay.id());

    repository.upsert(&first).await.unwrap();
    repository.upsert(&replay).await.unwrap();

    let stored = repository
        .get("owner-a", first.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.facts(), ["Original fact"]);
}

#[tokio::test]
async fn different_content_in_the_same_millisecond_is_stored_separately() {
    let repository = InMemoryAiBriefingRepository::new();
    let first = briefing("owner-a", AiTaskKind::MarketBriefing, 0, "First fact");
    let second = briefing("owner-a", AiTaskKind::MarketBriefing, 0, "Second fact");
    assert_ne!(first.id(), second.id());

    repository.upsert(&first).await.unwrap();
    repository.upsert(&second).await.unwrap();

    let stored = repository
        .list(&AiBriefingQuery {
            owner_scope: "owner-a".to_owned(),
            task: None,
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(stored.len(), 2);
}
