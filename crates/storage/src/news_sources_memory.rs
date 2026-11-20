use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use tm_trading_domain::{NewNewsSource, NewsSource};

use crate::{NewsSourceRepository, RepositoryError};

#[derive(Default)]
pub struct InMemoryNewsSourceRepository {
    sources: Mutex<HashMap<String, NewsSource>>,
}

impl InMemoryNewsSourceRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, HashMap<String, NewsSource>>, RepositoryError> {
        self.sources
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)
    }
}

#[async_trait]
impl NewsSourceRepository for InMemoryNewsSourceRepository {
    async fn upsert_seed(&self, source: NewNewsSource) -> Result<NewsSource, RepositoryError> {
        let candidate = NewsSource::new(source)?;
        let mut stored = self.lock()?;
        let saved = match stored.get(candidate.source_id()) {
            Some(existing) => {
                let updated_at = existing.updated_at().max(candidate.created_at());
                existing.clone().with_updated_at(updated_at)
            }
            None => candidate,
        };
        stored.insert(saved.source_id().to_owned(), saved.clone());
        Ok(saved)
    }

    async fn list(&self) -> Result<Vec<NewsSource>, RepositoryError> {
        let stored = self.lock()?;
        let mut sources: Vec<NewsSource> = stored.values().cloned().collect();
        sources.sort_by(|left, right| left.source_id().cmp(right.source_id()));
        Ok(sources)
    }

    async fn get(&self, source_id: &str) -> Result<Option<NewsSource>, RepositoryError> {
        Ok(self.lock()?.get(source_id).cloned())
    }

    async fn set_enabled(
        &self,
        source_id: &str,
        enabled: bool,
        updated_at: DateTime<Utc>,
    ) -> Result<NewsSource, RepositoryError> {
        let mut stored = self.lock()?;
        let updated = stored
            .get(source_id)
            .cloned()
            .ok_or(RepositoryError::NewsSourceNotFound)?
            .with_enabled(enabled, updated_at);
        stored.insert(source_id.to_owned(), updated.clone());
        Ok(updated)
    }

    async fn record_health(
        &self,
        source_id: &str,
        observed_at: DateTime<Utc>,
        item_count: u32,
        error: Option<&str>,
    ) -> Result<NewsSource, RepositoryError> {
        let mut stored = self.lock()?;
        let updated = stored
            .get(source_id)
            .cloned()
            .ok_or(RepositoryError::NewsSourceNotFound)?
            .with_fetch_result(observed_at, item_count, error);
        stored.insert(source_id.to_owned(), updated.clone());
        Ok(updated)
    }

    async fn request_retry(
        &self,
        source_id: &str,
        requested_at: DateTime<Utc>,
    ) -> Result<NewsSource, RepositoryError> {
        let mut stored = self.lock()?;
        let updated = stored
            .get(source_id)
            .cloned()
            .ok_or(RepositoryError::NewsSourceNotFound)?
            .with_retry_requested(requested_at, requested_at);
        stored.insert(source_id.to_owned(), updated.clone());
        Ok(updated)
    }
}
