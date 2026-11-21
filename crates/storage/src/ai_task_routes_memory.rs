use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use tm_trading_domain::AiTaskRoute;

use crate::{AiTaskRouteRepository, RepositoryError};

#[derive(Default)]
pub struct InMemoryAiTaskRouteRepository {
    routes: Mutex<HashMap<String, AiTaskRoute>>,
}

impl InMemoryAiTaskRouteRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AiTaskRouteRepository for InMemoryAiTaskRouteRepository {
    async fn list(&self) -> Result<Vec<AiTaskRoute>, RepositoryError> {
        let stored = self
            .routes
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        let mut routes: Vec<AiTaskRoute> = stored.values().cloned().collect();
        routes.sort_by_key(|left| std::cmp::Reverse(left.updated_at()));
        Ok(routes)
    }

    async fn get(&self, task: &str) -> Result<Option<AiTaskRoute>, RepositoryError> {
        let stored = self
            .routes
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        Ok(stored.get(task).cloned())
    }

    async fn upsert(&self, route: &AiTaskRoute) -> Result<(), RepositoryError> {
        let mut stored = self
            .routes
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        stored.insert(route.task().as_str().to_owned(), route.clone());
        Ok(())
    }

    async fn delete(&self, task: &str) -> Result<(), RepositoryError> {
        let mut stored = self
            .routes
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        if stored.remove(task).is_none() {
            return Err(RepositoryError::AiTaskRouteNotFound);
        }
        Ok(())
    }
}
