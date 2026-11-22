use chrono::{DateTime, TimeZone, Utc};
use tm_trading_domain::{
    AiBriefing, AiBriefingError, AiBriefingReference, AiBriefingReferenceKind, AiTaskKind,
    MAX_AI_BRIEFING_LIST_ITEMS, MAX_AI_BRIEFING_TEXT_CHARS, NewAiBriefing, is_valid_ai_briefing_id,
};

const RESEARCH_EVENT_ID: &str = "rev1_0123456789abcdef";
const RESEARCH_SNAPSHOT_ID: &str = "rsnap1_0123456789abcdef";
const NEWS_ITEM_ID: &str = "news_0123456789abcdef0123456789abcdef01234567";

fn at() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 21, 8, 0, 0).unwrap()
}

fn input() -> NewAiBriefing {
    NewAiBriefing {
        owner_scope: "local-operator".to_owned(),
        task: AiTaskKind::MarketBriefing,
        provider_id: "primary".to_owned(),
        model_registry_id: "primary-large".to_owned(),
        model_id: "model-a".to_owned(),
        prompt_version: "market-briefing.v1".to_owned(),
        facts: vec!["BTCUSDT shows a higher high on rising volume.".to_owned()],
        interpretations: vec!["Momentum may be strengthening.".to_owned()],
        uncertainties: vec!["The move may reverse without confirmation.".to_owned()],
        input_snapshot_ids: vec![RESEARCH_SNAPSHOT_ID.to_owned()],
        references: vec![
            AiBriefingReference::new(AiBriefingReferenceKind::ResearchEvent, RESEARCH_EVENT_ID)
                .unwrap(),
        ],
        missing_inputs: Vec::new(),
        stale_inputs: Vec::new(),
        generated_at: at(),
    }
}

#[test]
fn a_briefing_keeps_facts_and_interpretations_separate() {
    let briefing = AiBriefing::new(input()).unwrap();

    assert_eq!(briefing.facts(), input().facts);
    assert_eq!(briefing.interpretations(), input().interpretations);
    assert_eq!(briefing.uncertainties(), input().uncertainties);
    assert_eq!(briefing.input_snapshot_ids(), input().input_snapshot_ids);
    assert_eq!(briefing.references(), input().references);
}

#[test]
fn the_identifier_is_stable_for_the_same_briefing() {
    let first = AiBriefing::new(input()).unwrap();
    let second = AiBriefing::new(input()).unwrap();

    assert_eq!(first.id(), second.id());
    assert!(first.id().starts_with("aibr1_"));
}

#[test]
fn a_later_generation_is_a_different_briefing() {
    let first = AiBriefing::new(input()).unwrap();
    let mut later = input();
    later.generated_at = at() + chrono::Duration::minutes(5);

    assert_ne!(first.id(), AiBriefing::new(later).unwrap().id());
}

#[test]
fn different_content_in_the_same_millisecond_has_a_different_identifier() {
    let first = AiBriefing::new(input()).unwrap();
    let mut changed = input();
    changed.facts = vec!["A different reported fact.".to_owned()];

    assert_ne!(first.id(), AiBriefing::new(changed).unwrap().id());
}

#[test]
fn a_briefing_identifier_has_the_exact_public_shape() {
    let briefing = AiBriefing::new(input()).unwrap();

    assert!(is_valid_ai_briefing_id(briefing.id()));
    assert!(!is_valid_ai_briefing_id("aibr1_not-hex"));
    assert!(!is_valid_ai_briefing_id("rev1_0123456789abcdef"));
}

#[test]
fn a_briefing_without_evidence_is_rejected() {
    let mut without = input();
    without.input_snapshot_ids.clear();
    without.references.clear();

    assert_eq!(
        AiBriefing::new(without).unwrap_err(),
        AiBriefingError::NoEvidence
    );
}

#[test]
fn a_briefing_without_content_is_rejected() {
    let mut without = input();
    without.facts.clear();
    without.interpretations.clear();
    without.uncertainties.clear();

    assert_eq!(
        AiBriefing::new(without).unwrap_err(),
        AiBriefingError::NoContent
    );
}

#[test]
fn a_stored_briefing_keeps_its_producer_and_schema_stamps() {
    let briefing = AiBriefing::from_stored(
        input(),
        "producer/previous".to_owned(),
        "ai-briefing.previous".to_owned(),
    )
    .unwrap();

    assert_eq!(briefing.producer_version(), "producer/previous");
    assert_eq!(briefing.schema_version(), "ai-briefing.previous");
}

#[test]
fn producer_and_schema_stamps_are_part_of_the_identifier() {
    let current = AiBriefing::new(input()).unwrap();
    let previous = AiBriefing::from_stored(
        input(),
        "producer/previous".to_owned(),
        "ai-briefing.previous".to_owned(),
    )
    .unwrap();

    assert_ne!(current.id(), previous.id());
}

#[test]
fn required_identity_fields_reject_blank_values() {
    let mut without_provider = input();
    without_provider.provider_id = "  ".to_owned();

    assert_eq!(
        AiBriefing::new(without_provider).unwrap_err(),
        AiBriefingError::EmptyField("providerId")
    );
}

#[test]
fn lists_are_bounded() {
    let mut too_many = input();
    too_many.facts = (0..=MAX_AI_BRIEFING_LIST_ITEMS)
        .map(|index| format!("Fact {index}"))
        .collect();

    assert_eq!(
        AiBriefing::new(too_many).unwrap_err(),
        AiBriefingError::TooManyItems("facts")
    );
}

#[test]
fn text_items_are_bounded() {
    let mut too_long = input();
    too_long.facts = vec!["x".repeat(MAX_AI_BRIEFING_TEXT_CHARS + 1)];

    assert_eq!(
        AiBriefing::new(too_long).unwrap_err(),
        AiBriefingError::TextTooLong("facts")
    );
}

#[test]
fn total_content_is_bounded() {
    let mut too_long = input();
    too_long.facts = vec!["x".repeat(MAX_AI_BRIEFING_TEXT_CHARS)];
    too_long.interpretations = (0..4)
        .map(|_| "y".repeat(MAX_AI_BRIEFING_TEXT_CHARS))
        .collect();
    too_long.uncertainties = vec!["z".repeat(MAX_AI_BRIEFING_TEXT_CHARS)];

    assert_eq!(
        AiBriefing::new(too_long).unwrap_err(),
        AiBriefingError::ContentTooLong
    );
}

#[test]
fn reference_values_must_match_their_kind() {
    assert_eq!(
        AiBriefingReference::new(AiBriefingReferenceKind::ResearchEvent, NEWS_ITEM_ID).unwrap_err(),
        AiBriefingError::InvalidReferenceValue("research_event")
    );
    assert_eq!(
        AiBriefingReference::new(AiBriefingReferenceKind::NewsItem, RESEARCH_EVENT_ID).unwrap_err(),
        AiBriefingError::InvalidReferenceValue("news_item")
    );
    assert_eq!(
        AiBriefingReference::new(AiBriefingReferenceKind::SourceUrl, "not a URL").unwrap_err(),
        AiBriefingError::InvalidReferenceValue("source_url")
    );
    assert_eq!(
        AiBriefingReference::new(
            AiBriefingReferenceKind::SourceUrl,
            "ftp://example.com/article"
        )
        .unwrap_err(),
        AiBriefingError::InvalidReferenceValue("source_url")
    );
    assert_eq!(
        AiBriefingReference::new(
            AiBriefingReferenceKind::SourceUrl,
            "https://example.com/article?token=secret"
        )
        .unwrap_err(),
        AiBriefingError::InvalidReferenceValue("source_url")
    );
    assert_eq!(
        AiBriefingReference::new(
            AiBriefingReferenceKind::SourceUrl,
            "https://example.com/article#access_token=secret"
        )
        .unwrap_err(),
        AiBriefingError::InvalidReferenceValue("source_url")
    );

    assert!(
        AiBriefingReference::new(AiBriefingReferenceKind::ResearchEvent, RESEARCH_EVENT_ID).is_ok()
    );
    assert!(AiBriefingReference::new(AiBriefingReferenceKind::NewsItem, NEWS_ITEM_ID).is_ok());
    assert!(
        AiBriefingReference::new(
            AiBriefingReferenceKind::SourceUrl,
            "https://example.com/article"
        )
        .is_ok()
    );
}

#[test]
fn input_snapshot_ids_must_be_exact_research_snapshot_identifiers() {
    let mut invalid = input();
    invalid.input_snapshot_ids = vec![RESEARCH_EVENT_ID.to_owned()];

    assert_eq!(
        AiBriefing::new(invalid).unwrap_err(),
        AiBriefingError::InvalidInputSnapshotId
    );
}

#[test]
fn references_are_bounded_and_non_empty() {
    let mut too_many = input();
    too_many.references = (0..=MAX_AI_BRIEFING_LIST_ITEMS)
        .map(|index| {
            AiBriefingReference::new(
                AiBriefingReferenceKind::ResearchEvent,
                format!("rev1_{index:016x}"),
            )
            .unwrap()
        })
        .collect();
    assert_eq!(
        AiBriefing::new(too_many).unwrap_err(),
        AiBriefingError::TooManyItems("references")
    );

    assert_eq!(
        AiBriefingReference::new(AiBriefingReferenceKind::NewsItem, " ").unwrap_err(),
        AiBriefingError::EmptyField("reference.value")
    );
}
