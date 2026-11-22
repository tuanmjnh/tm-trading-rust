use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use tm_trading_domain::AiBriefing;

use crate::{AiBriefingQuery, AiBriefingRepository, RepositoryError};

#[derive(Default)]
pub struct InMemoryAiBriefingRepository {
    briefings: Mutex<HashMap<String, AiBriefing>>,
}

impl InMemoryAiBriefingRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AiBriefingRepository for InMemoryAiBriefingRepository {
    async fn upsert(&self, briefing: &AiBriefing) -> Result<(), RepositoryError> {
        let mut stored = self
            .briefings
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        match stored.get(briefing.id()) {
            Some(existing) if existing != briefing => Err(RepositoryError::AiBriefingConflict),
            Some(_) => Ok(()),
            None => {
                stored.insert(briefing.id().to_owned(), briefing.clone());
                Ok(())
            }
        }
    }

    async fn list(&self, query: &AiBriefingQuery) -> Result<Vec<AiBriefing>, RepositoryError> {
        let stored = self
            .briefings
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        let mut briefings: Vec<AiBriefing> = stored
            .values()
            .filter(|briefing| {
                briefing.owner_scope() == query.owner_scope
                    && query.task.is_none_or(|task| briefing.task() == task)
            })
            .cloned()
            .collect();
        briefings.sort_by(|left, right| {
            right
                .generated_at()
                .cmp(&left.generated_at())
                .then_with(|| right.id().cmp(left.id()))
        });
        briefings.truncate(query.limit);
        Ok(briefings)
    }

    async fn get(
        &self,
        owner_scope: &str,
        briefing_id: &str,
    ) -> Result<Option<AiBriefing>, RepositoryError> {
        let stored = self
            .briefings
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        Ok(stored
            .get(briefing_id)
            .filter(|briefing| briefing.owner_scope() == owner_scope)
            .cloned())
    }
}
