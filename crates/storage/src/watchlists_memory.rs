use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use tm_trading_domain::Watchlist;

use crate::repository::memory_id;
use crate::{RepositoryError, WatchlistRepository};

#[derive(Default)]
pub struct InMemoryWatchlistRepository {
    watchlists: Mutex<HashMap<String, Watchlist>>,
}

impl InMemoryWatchlistRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl WatchlistRepository for InMemoryWatchlistRepository {
    async fn list(&self, owner_id: &str) -> Result<Vec<Watchlist>, RepositoryError> {
        let stored = self.lock()?;
        let mut lists: Vec<Watchlist> = stored
            .values()
            .filter(|watchlist| watchlist.owner_id() == owner_id)
            .cloned()
            .collect();
        sort_lists(&mut lists);
        Ok(lists)
    }

    async fn create(
        &self,
        owner_id: &str,
        name: &str,
        symbols: &[String],
        now: DateTime<Utc>,
    ) -> Result<Watchlist, RepositoryError> {
        let mut stored = self.lock()?;
        let name = tm_trading_domain::normalize_name(name)?;
        if stored.values().any(|watchlist| {
            watchlist.owner_id() == owner_id && watchlist.name().eq_ignore_ascii_case(&name)
        }) {
            return Err(RepositoryError::DuplicateWatchlistName);
        }
        let is_default = !stored
            .values()
            .any(|watchlist| watchlist.owner_id() == owner_id);
        let watchlist = Watchlist::new(
            memory_id(),
            owner_id.to_owned(),
            name,
            symbols.to_vec(),
            is_default,
            now,
            now,
        )?;
        stored.insert(watchlist.id().to_owned(), watchlist.clone());
        Ok(watchlist)
    }

    async fn replace_symbols(
        &self,
        owner_id: &str,
        watchlist_id: &str,
        symbols: &[String],
        now: DateTime<Utc>,
    ) -> Result<Watchlist, RepositoryError> {
        let mut stored = self.lock()?;
        let watchlist = stored
            .get(watchlist_id)
            .filter(|watchlist| watchlist.owner_id() == owner_id)
            .cloned()
            .ok_or(RepositoryError::WatchlistNotFound)?;
        let updated = watchlist.with_symbols(symbols.to_vec(), now)?;
        stored.insert(updated.id().to_owned(), updated.clone());
        Ok(updated)
    }

    async fn delete(&self, owner_id: &str, watchlist_id: &str) -> Result<(), RepositoryError> {
        let mut stored = self.lock()?;
        match stored.get(watchlist_id) {
            Some(watchlist) if watchlist.owner_id() == owner_id => {
                stored.remove(watchlist_id);
                Ok(())
            }
            _ => Err(RepositoryError::WatchlistNotFound),
        }
    }
}

impl InMemoryWatchlistRepository {
    fn lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, HashMap<String, Watchlist>>, RepositoryError> {
        self.watchlists
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)
    }
}

pub(crate) fn sort_lists(lists: &mut [Watchlist]) {
    lists.sort_by(|left, right| {
        right
            .is_default()
            .cmp(&left.is_default())
            .then_with(|| right.updated_at().cmp(&left.updated_at()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 19, 0, 0, 0).unwrap()
    }

    fn symbols(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[tokio::test]
    async fn the_first_list_becomes_the_owner_default() {
        let repository = InMemoryWatchlistRepository::new();

        let first = repository
            .create("owner-a", "First", &symbols(&["BTCUSDT"]), at())
            .await
            .unwrap();
        let second = repository
            .create("owner-a", "Second", &symbols(&["ETHUSDT"]), at())
            .await
            .unwrap();

        assert!(first.is_default());
        assert!(!second.is_default());
    }

    #[tokio::test]
    async fn another_owners_first_list_is_also_default() {
        let repository = InMemoryWatchlistRepository::new();
        repository
            .create("owner-a", "First", &symbols(&["BTCUSDT"]), at())
            .await
            .unwrap();

        let other = repository
            .create("owner-b", "First", &symbols(&["ETHUSDT"]), at())
            .await
            .unwrap();

        assert!(other.is_default());
    }

    #[tokio::test]
    async fn duplicate_names_are_rejected_case_insensitively() {
        let repository = InMemoryWatchlistRepository::new();
        repository
            .create("owner-a", "My List", &[], at())
            .await
            .unwrap();

        let result = repository.create("owner-a", "my list", &[], at()).await;

        assert!(matches!(
            result,
            Err(RepositoryError::DuplicateWatchlistName)
        ));
    }

    #[tokio::test]
    async fn one_owner_cannot_touch_another_owners_list() {
        let repository = InMemoryWatchlistRepository::new();
        let list = repository
            .create("owner-a", "Mine", &symbols(&["BTCUSDT"]), at())
            .await
            .unwrap();

        let replace = repository
            .replace_symbols("owner-b", list.id(), &symbols(&["ETHUSDT"]), at())
            .await;
        let delete = repository.delete("owner-b", list.id()).await;
        let stored = repository.list("owner-a").await.unwrap();

        assert!(matches!(replace, Err(RepositoryError::WatchlistNotFound)));
        assert!(matches!(delete, Err(RepositoryError::WatchlistNotFound)));
        assert_eq!(stored, vec![list]);
    }

    #[tokio::test]
    async fn replacing_symbols_is_normalized_and_lists_newest_default_first() {
        let repository = InMemoryWatchlistRepository::new();
        let list = repository
            .create("owner-a", "Mine", &symbols(&["BTCUSDT"]), at())
            .await
            .unwrap();
        let later = at() + chrono::Duration::seconds(5);

        let updated = repository
            .replace_symbols(
                "owner-a",
                list.id(),
                &symbols(&["ethusdt", "ETHUSDT"]),
                later,
            )
            .await
            .unwrap();
        let stored = repository.list("owner-a").await.unwrap();

        assert_eq!(updated.symbols(), ["ETHUSDT"]);
        assert_eq!(stored, vec![updated]);
    }

    #[tokio::test]
    async fn deleting_a_list_removes_it() {
        let repository = InMemoryWatchlistRepository::new();
        let list = repository
            .create("owner-a", "Mine", &[], at())
            .await
            .unwrap();

        repository.delete("owner-a", list.id()).await.unwrap();

        assert!(repository.list("owner-a").await.unwrap().is_empty());
    }
}
