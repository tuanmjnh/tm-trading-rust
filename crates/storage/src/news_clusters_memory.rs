use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use tm_trading_domain::NewsCluster;

use crate::{NewsClusterQuery, NewsClusterRepository, RepositoryError};

#[derive(Default)]
pub struct InMemoryNewsClusterRepository {
    clusters: Mutex<HashMap<String, NewsCluster>>,
}

impl InMemoryNewsClusterRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl NewsClusterRepository for InMemoryNewsClusterRepository {
    async fn upsert(&self, cluster: &NewsCluster) -> Result<(), RepositoryError> {
        let mut stored = self
            .clusters
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        stored.insert(cluster.cluster_id().to_owned(), cluster.clone());
        Ok(())
    }

    async fn list(&self, query: &NewsClusterQuery) -> Result<Vec<NewsCluster>, RepositoryError> {
        let stored = self
            .clusters
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        let mut clusters: Vec<NewsCluster> = stored
            .values()
            .filter(|cluster| query.matches(cluster))
            .cloned()
            .collect();
        clusters.sort_by(|left, right| {
            right
                .last_seen_at()
                .cmp(&left.last_seen_at())
                .then_with(|| right.cluster_id().cmp(left.cluster_id()))
        });
        clusters.truncate(query.limit);
        Ok(clusters)
    }

    async fn get(&self, cluster_id: &str) -> Result<Option<NewsCluster>, RepositoryError> {
        let stored = self
            .clusters
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        Ok(stored.get(cluster_id).cloned())
    }
}
