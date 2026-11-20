use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use tm_trading_contracts::{
    ResearchEventDto, ResearchEventListDto, ResearchEventQueryDto, ResearchFreshnessDto,
    ResearchQualityDto,
};
use tm_trading_domain::{ResearchEvent, ResearchQuality};
use tm_trading_market_providers::timeframe_millis;
use tm_trading_storage::ResearchEventQuery;

use crate::auth::CAPABILITY_RESEARCH_READ;
use crate::error::ApiError;
use crate::state::AppState;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

/// Lists the authenticated owner's research observations, newest first.
pub async fn list_research_events(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Query(query): Query<ResearchEventQueryDto>,
) -> Result<Json<ResearchEventListDto>, ApiError> {
    auth.require_capability(CAPABILITY_RESEARCH_READ)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > MAX_LIMIT {
        return Err(ApiError::UnprocessableEntity(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }

    let events = state
        .research_events
        .list(&ResearchEventQuery {
            owner_scope: Some(auth.subject().to_owned()),
            symbol: clean_filter(query.symbol),
            timeframe: clean_filter(query.timeframe),
            event_type: clean_filter(query.event_type),
            since: query.since,
            limit,
        })
        .await
        .map_err(ApiError::from)?;
    let now = Utc::now();
    let events = events
        .iter()
        .map(|event| to_dto(event, now))
        .collect::<Vec<_>>();
    Ok(Json(ResearchEventListDto {
        generated_at: now,
        count: events.len(),
        events,
    }))
}

/// Returns one research observation owned by the authenticated subject.
pub async fn get_research_event(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Path(event_id): Path<String>,
) -> Result<Json<ResearchEventDto>, ApiError> {
    auth.require_capability(CAPABILITY_RESEARCH_READ)?;
    let event = state
        .research_events
        .get(&event_id)
        .await
        .map_err(ApiError::from)?
        .filter(|event| event.owner_scope() == auth.subject())
        .ok_or_else(|| ApiError::NotFound("research event was not found".to_owned()))?;
    Ok(Json(to_dto(&event, Utc::now())))
}

fn to_dto(event: &ResearchEvent, now: DateTime<Utc>) -> ResearchEventDto {
    let age_seconds = (now - event.detected_at()).num_seconds().max(0);
    let freshness = if is_fresh(event, age_seconds) {
        ResearchFreshnessDto::Fresh
    } else {
        ResearchFreshnessDto::Stale
    };
    ResearchEventDto {
        id: event.id().to_owned(),
        owner_scope: event.owner_scope().to_owned(),
        symbol: event.symbol().to_owned(),
        timeframe: event.timeframe().to_owned(),
        event_type: event.event_type().to_owned(),
        method: event.method().to_owned(),
        method_version: event.method_version().to_owned(),
        direction: event.direction(),
        score: event.score(),
        observed_at: event.observed_at(),
        detected_at: event.detected_at(),
        ingested_at: event.ingested_at(),
        source_id: event.source_id().to_owned(),
        provider_method: event.provider_method().to_owned(),
        input_snapshot_id: event.input_snapshot_id().to_owned(),
        producer_version: event.producer_version().to_owned(),
        schema_version: event.schema_version().to_owned(),
        quality_state: match event.quality_state() {
            ResearchQuality::Complete => ResearchQualityDto::Complete,
            ResearchQuality::Partial => ResearchQualityDto::Partial,
            ResearchQuality::Unknown => ResearchQualityDto::Unknown,
        },
        quality_reasons: event.quality_reasons().to_vec(),
        freshness,
        age_seconds,
        evidence: event.evidence().clone(),
        context: event.context().clone(),
    }
}

/// Half a timeframe, with a one-minute floor, separates fresh from stale.
///
/// An unknown timeframe fails closed as stale because its expected update
/// cadence cannot be established.
fn is_fresh(event: &ResearchEvent, age_seconds: i64) -> bool {
    timeframe_millis(event.timeframe()).is_ok_and(|millis| age_seconds <= (millis / 2_000).max(60))
}

fn clean_filter(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tm_trading_domain::NewResearchEvent;

    fn event(timeframe: &str, detected_at: DateTime<Utc>) -> ResearchEvent {
        ResearchEvent::new(NewResearchEvent {
            owner_scope: "test-operator".to_owned(),
            symbol: "BTCUSDT".to_owned(),
            timeframe: timeframe.to_owned(),
            method: "vsa".to_owned(),
            method_version: "1".to_owned(),
            event_type: "ST LONG".to_owned(),
            direction: 1,
            score: 1.0,
            observed_at: detected_at,
            detected_at,
            ingested_at: detected_at,
            source_id: "binance".to_owned(),
            provider_method: "binance.klines".to_owned(),
            input_snapshot_id: "rsnap1_1".to_owned(),
            quality_state: ResearchQuality::Complete,
            quality_reasons: Vec::new(),
            evidence: BTreeMap::new(),
            context: BTreeMap::new(),
        })
        .expect("valid research event")
    }

    #[test]
    fn freshness_uses_half_the_timeframe_with_a_one_minute_floor() {
        let now = Utc::now();
        assert!(is_fresh(&event("1m", now), 60));
        assert!(!is_fresh(&event("1m", now), 61));
        assert!(is_fresh(&event("15m", now), 450));
        assert!(!is_fresh(&event("15m", now), 451));
    }

    #[test]
    fn an_unknown_timeframe_fails_closed_as_stale() {
        let now = Utc::now();
        assert!(!is_fresh(&event("unknown", now), 0));
    }

    #[test]
    fn a_blank_filter_is_ignored() {
        assert_eq!(clean_filter(Some("  ".to_owned())), None);
        assert_eq!(
            clean_filter(Some(" BTCUSDT ".to_owned())),
            Some("BTCUSDT".to_owned())
        );
    }
}
