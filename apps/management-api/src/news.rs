use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use tm_trading_contracts::{
    NewsDirectionDto, NewsFreshnessDto, NewsHealthStateDto, NewsItemDto, NewsLevelDto, NewsListDto,
    NewsQueryDto, NewsSourceDto, NewsSourceListDto,
};
use tm_trading_domain::{NewsDirection, NewsHealthState, NewsItem, NewsLevel, NewsSource};
use tm_trading_storage::NewsItemQuery;

use crate::auth::CAPABILITY_NEWS_READ;
use crate::error::ApiError;
use crate::state::AppState;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
/// A story ingested longer ago than this is reported as stale.
const FRESH_WINDOW_SECONDS: i64 = 3_600;

/// Lists the rights-safe normalized stories, newest ingestion first.
pub async fn list_news(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Query(query): Query<NewsQueryDto>,
) -> Result<Json<NewsListDto>, ApiError> {
    auth.require_capability(CAPABILITY_NEWS_READ)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > MAX_LIMIT {
        return Err(ApiError::UnprocessableEntity(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )));
    }
    let level = query.level.map(level_from_dto);
    let items = state
        .news_items
        .list(&NewsItemQuery {
            source_id: clean_filter(query.source_id),
            market_group: clean_filter(query.market_group),
            level,
            since: query.since,
            limit,
        })
        .await
        .map_err(ApiError::from)?;
    let now = Utc::now();
    let items = items
        .iter()
        .filter(|item| item.display_allowed())
        .map(|item| to_dto(item, now))
        .collect::<Vec<_>>();
    Ok(Json(NewsListDto {
        generated_at: now,
        count: items.len(),
        stale_after_seconds: FRESH_WINDOW_SECONDS,
        items,
    }))
}

/// Returns one story that its rights snapshot allows displaying.
pub async fn get_news_item(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
    Path(news_id): Path<String>,
) -> Result<Json<NewsItemDto>, ApiError> {
    auth.require_capability(CAPABILITY_NEWS_READ)?;
    let item = state
        .news_items
        .get(&news_id)
        .await
        .map_err(ApiError::from)?
        .filter(NewsItem::display_allowed)
        .ok_or_else(|| ApiError::NotFound("news item was not found".to_owned()))?;
    Ok(Json(to_dto(&item, Utc::now())))
}

/// Lists the configured news sources with the health each one reported.
pub async fn list_news_sources(
    State(state): State<AppState>,
    auth: crate::auth::AuthContext,
) -> Result<Json<NewsSourceListDto>, ApiError> {
    auth.require_capability(CAPABILITY_NEWS_READ)?;
    let sources = state.news_sources.list().await.map_err(ApiError::from)?;
    let now = Utc::now();
    let sources: Vec<NewsSourceDto> = sources.iter().map(source_to_dto).collect();
    Ok(Json(NewsSourceListDto {
        generated_at: now,
        count: sources.len(),
        sources,
    }))
}

fn to_dto(item: &NewsItem, now: DateTime<Utc>) -> NewsItemDto {
    let age_seconds = (now - item.ingested_at()).num_seconds().max(0);
    NewsItemDto {
        id: item.item_id().to_owned(),
        source_id: item.source_id().to_owned(),
        url: item.origin_url().to_owned(),
        title: item.title().to_owned(),
        excerpt: item.excerpt().to_owned(),
        language: item.language().to_owned(),
        published_at: item.published_at(),
        first_seen_at: item.first_seen_at(),
        ingested_at: item.ingested_at(),
        market_group: item.market_group().to_owned(),
        event_type: item.event_type().map(str::to_owned),
        tags: item.tags().to_vec(),
        level: match item.level() {
            NewsLevel::High => NewsLevelDto::High,
            NewsLevel::Med => NewsLevelDto::Med,
            NewsLevel::Low => NewsLevelDto::Low,
        },
        direction: match item.direction() {
            NewsDirection::Pos => NewsDirectionDto::Pos,
            NewsDirection::Neg => NewsDirectionDto::Neg,
            NewsDirection::Flat => NewsDirectionDto::Flat,
        },
        score: item.score(),
        freshness: if age_seconds <= FRESH_WINDOW_SECONDS {
            NewsFreshnessDto::Fresh
        } else {
            NewsFreshnessDto::Stale
        },
        age_seconds,
    }
}

fn source_to_dto(source: &NewsSource) -> NewsSourceDto {
    let health = source.health();
    NewsSourceDto {
        id: source.source_id().to_owned(),
        name: source.name().to_owned(),
        provider: source.provider().map(str::to_owned),
        transport: source.transport().as_str().to_owned(),
        trust_tier: source.trust_tier().as_str().to_owned(),
        enabled: source.enabled(),
        feed_url: source.feed_url().map(str::to_owned),
        market_groups: source.market_groups().to_vec(),
        languages: source.languages().to_vec(),
        poll_interval_sec: source.poll_interval_sec(),
        retention_days: source.retention_days(),
        health_state: match health.state {
            NewsHealthState::Ok => NewsHealthStateDto::Ok,
            NewsHealthState::Degraded => NewsHealthStateDto::Degraded,
            NewsHealthState::Down => NewsHealthStateDto::Down,
            NewsHealthState::Unknown => NewsHealthStateDto::Unknown,
        },
        last_fetch_at: health.last_fetch_at,
        last_ok_at: health.last_ok_at,
        last_item_at: health.last_item_at,
        last_error: health.last_error.clone(),
        consecutive_errors: health.consecutive_errors,
    }
}

fn level_from_dto(level: NewsLevelDto) -> NewsLevel {
    match level {
        NewsLevelDto::High => NewsLevel::High,
        NewsLevelDto::Med => NewsLevel::Med,
        NewsLevelDto::Low => NewsLevel::Low,
    }
}

fn clean_filter(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tm_trading_domain::{
        NewNewsItem, NewNewsSource, NewsItemRights, NewsLicenseStatus, NewsRights, NewsTransport,
        NewsTrustTier,
    };

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, 8, 0, 0).unwrap()
    }

    fn item(ingested_at: DateTime<Utc>) -> NewsItem {
        NewsItem::new(NewNewsItem {
            source_id: "coindesk".to_owned(),
            url: "https://example.com/story".to_owned(),
            origin_url: "https://example.com/story?utm_source=rss".to_owned(),
            title: "Bitcoin holds support".to_owned(),
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
            score: 0.0,
            direction: NewsDirection::Flat,
            rights: NewsItemRights::default(),
        })
        .unwrap()
    }

    fn source() -> NewsSource {
        NewsSource::new(NewNewsSource {
            source_id: "coindesk".to_owned(),
            name: "CoinDesk".to_owned(),
            provider: Some("CoinDesk".to_owned()),
            transport: NewsTransport::Rss,
            feed_url: Some("https://example.com/feed".to_owned()),
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
        })
        .unwrap()
    }

    #[test]
    fn freshness_uses_the_ingestion_age() {
        let recent = item(at());
        assert_eq!(to_dto(&recent, at()).freshness, NewsFreshnessDto::Fresh);
        let stale = item(at() - chrono::Duration::seconds(FRESH_WINDOW_SECONDS + 1));
        assert_eq!(to_dto(&stale, at()).freshness, NewsFreshnessDto::Stale);
    }

    #[test]
    fn a_story_keeps_its_citation_link_and_missing_publication_time() {
        let dto = to_dto(&item(at()), at());
        assert_eq!(dto.url, "https://example.com/story?utm_source=rss");
        assert_eq!(dto.published_at, None);
        assert_eq!(dto.market_group, "crypto");
        assert_eq!(dto.tags, ["bitcoin"]);
    }

    #[test]
    fn a_source_dto_never_carries_a_secret() {
        let dto = source_to_dto(&source());
        assert_eq!(dto.feed_url.as_deref(), Some("https://example.com/feed"));
        assert_eq!(dto.transport, "rss");
        assert_eq!(dto.health_state, NewsHealthStateDto::Unknown);
        assert_eq!(dto.last_error, None);
    }
}
