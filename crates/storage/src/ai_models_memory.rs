use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use tm_trading_domain::AiModel;

use crate::{AiModelRepository, RepositoryError};

#[derive(Default)]
pub struct InMemoryAiModelRepository {
    models: Mutex<HashMap<String, AiModel>>,
}

impl InMemoryAiModelRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AiModelRepository for InMemoryAiModelRepository {
    async fn list(&self) -> Result<Vec<AiModel>, RepositoryError> {
        let stored = self
            .models
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        let mut models: Vec<AiModel> = stored.values().cloned().collect();
        models.sort_by_key(|left| std::cmp::Reverse(left.updated_at()));
        Ok(models)
    }

    async fn get(&self, id: &str) -> Result<Option<AiModel>, RepositoryError> {
        let stored = self
            .models
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        Ok(stored.get(id).cloned())
    }

    async fn upsert(&self, model: &AiModel) -> Result<(), RepositoryError> {
        let mut stored = self
            .models
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        stored.insert(model.id().to_owned(), model.clone());
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), RepositoryError> {
        let mut stored = self
            .models
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        if stored.remove(id).is_none() {
            return Err(RepositoryError::AiModelNotFound);
        }
        Ok(())
    }

    async fn delete_by_provider(&self, provider_id: &str) -> Result<u64, RepositoryError> {
        let mut stored = self
            .models
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        let before = stored.len();
        stored.retain(|_, model| model.provider_id() != provider_id);
        Ok((before - stored.len()) as u64)
    }
}
