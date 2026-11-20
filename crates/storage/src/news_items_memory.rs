use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use tm_trading_domain::NewsItem;

use crate::{NewsItemQuery, NewsItemRepository, NewsItemUpsertOutcome, RepositoryError};

#[derive(Default)]
pub struct InMemoryNewsItemRepository {
    items: Mutex<HashMap<String, NewsItem>>,
}

impl InMemoryNewsItemRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl NewsItemRepository for InMemoryNewsItemRepository {
    async fn upsert_seen(
        &self,
        items: &[NewsItem],
    ) -> Result<NewsItemUpsertOutcome, RepositoryError> {
        let mut stored = self
            .items
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        let mut outcome = NewsItemUpsertOutcome::default();
        for item in items {
            match stored.get(item.item_id()).cloned() {
                Some(existing) => {
                    let merged = NewsItem::merge_reingest(existing.clone(), item.clone());
                    if merged == existing {
                        outcome.unchanged += 1;
                    } else {
                        stored.insert(merged.item_id().to_owned(), merged);
                        outcome.updated += 1;
                    }
                }
                None => {
                    stored.insert(item.item_id().to_owned(), item.clone());
                    outcome.inserted += 1;
                }
            }
        }
        Ok(outcome)
    }

    async fn list(&self, query: &NewsItemQuery) -> Result<Vec<NewsItem>, RepositoryError> {
        let stored = self
            .items
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        let mut items: Vec<NewsItem> = stored
            .values()
            .filter(|item| query.matches(item))
            .cloned()
            .collect();
        items.sort_by(|left, right| {
            right
                .ingested_at()
                .cmp(&left.ingested_at())
                .then_with(|| right.item_id().cmp(left.item_id()))
        });
        items.truncate(query.limit);
        Ok(items)
    }

    async fn get(&self, item_id: &str) -> Result<Option<NewsItem>, RepositoryError> {
        let stored = self
            .items
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        Ok(stored.get(item_id).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, TimeZone, Utc};
    use tm_trading_domain::{NewNewsItem, NewsDirection, NewsItemRights, NewsLevel};

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, 8, 0, 0).unwrap()
    }

    fn item(url: &str, title: &str, ingested_at: DateTime<Utc>) -> NewsItem {
        NewsItem::new(NewNewsItem {
            source_id: "coindesk".to_owned(),
            url: url.to_owned(),
            origin_url: url.to_owned(),
            title: title.to_owned(),
            excerpt: "Summary.".to_owned(),
            language: "en".to_owned(),
            published_at: None,
            first_seen_at: at(),
            ingested_at,
            market_group: None,
            source_market_groups: vec!["crypto".to_owned()],
            event_type: None,
            tags: Vec::new(),
            level: NewsLevel::Med,
            score: 0.5,
            direction: NewsDirection::Flat,
            rights: NewsItemRights::default(),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn replaying_a_story_keeps_one_frozen_row() {
        let repository = InMemoryNewsItemRepository::new();
        let first = item("https://example.com/story?utm_source=a", "Title", at());
        assert_eq!(
            repository
                .upsert_seen(std::slice::from_ref(&first))
                .await
                .unwrap(),
            NewsItemUpsertOutcome {
                inserted: 1,
                updated: 0,
                unchanged: 0,
            }
        );

        let replay = item(
            "https://example.com/story",
            "Title",
            at() + chrono::Duration::hours(1),
        );
        assert_eq!(replay.item_id(), first.item_id());
        assert_eq!(
            repository
                .upsert_seen(std::slice::from_ref(&replay))
                .await
                .unwrap(),
            NewsItemUpsertOutcome {
                inserted: 0,
                updated: 0,
                unchanged: 1,
            }
        );
        let stored = repository.get(first.item_id()).await.unwrap().unwrap();
        assert_eq!(stored.ingested_at(), at());
    }

    #[tokio::test]
    async fn a_material_change_updates_the_envelope_once() {
        let repository = InMemoryNewsItemRepository::new();
        let first = item("https://example.com/story", "Title", at());
        repository
            .upsert_seen(std::slice::from_ref(&first))
            .await
            .unwrap();

        let corrected = item(
            "https://example.com/story",
            "Corrected title",
            at() + chrono::Duration::hours(1),
        );
        assert_eq!(
            repository
                .upsert_seen(std::slice::from_ref(&corrected))
                .await
                .unwrap(),
            NewsItemUpsertOutcome {
                inserted: 0,
                updated: 1,
                unchanged: 0,
            }
        );
        let stored = repository.get(first.item_id()).await.unwrap().unwrap();
        assert_eq!(stored.title(), "Corrected title");
        assert_eq!(stored.updated_at(), Some(at() + chrono::Duration::hours(1)));
    }

    #[tokio::test]
    async fn list_is_newest_first_filtered_and_bounded() {
        let repository = InMemoryNewsItemRepository::new();
        let older = item("https://example.com/older", "Older", at());
        let newer = item(
            "https://example.com/newer",
            "Newer",
            at() + chrono::Duration::minutes(5),
        );
        repository
            .upsert_seen(&[older, newer.clone()])
            .await
            .unwrap();

        let query = NewsItemQuery {
            limit: 1,
            ..NewsItemQuery::default()
        };
        assert_eq!(repository.list(&query).await.unwrap(), vec![newer]);

        let filtered = NewsItemQuery {
            source_id: Some("other".to_owned()),
            ..NewsItemQuery::default()
        };
        assert!(repository.list(&filtered).await.unwrap().is_empty());
    }
}
