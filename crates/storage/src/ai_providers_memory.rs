use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use tm_trading_domain::AiProviderProfile;

use crate::{AiProviderProfileRepository, RepositoryError};

#[derive(Default)]
pub struct InMemoryAiProviderProfileRepository {
    profiles: Mutex<HashMap<String, AiProviderProfile>>,
}

impl InMemoryAiProviderProfileRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AiProviderProfileRepository for InMemoryAiProviderProfileRepository {
    async fn list(&self) -> Result<Vec<AiProviderProfile>, RepositoryError> {
        let stored = self
            .profiles
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        let mut profiles: Vec<AiProviderProfile> = stored.values().cloned().collect();
        profiles.sort_by_key(|left| std::cmp::Reverse(left.updated_at()));
        Ok(profiles)
    }

    async fn get(&self, id: &str) -> Result<Option<AiProviderProfile>, RepositoryError> {
        let stored = self
            .profiles
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        Ok(stored.get(id).cloned())
    }

    async fn upsert(&self, profile: &AiProviderProfile) -> Result<(), RepositoryError> {
        let mut stored = self
            .profiles
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        stored.insert(profile.id().to_owned(), profile.clone());
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), RepositoryError> {
        let mut stored = self
            .profiles
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)?;
        if stored.remove(id).is_none() {
            return Err(RepositoryError::AiProviderProfileNotFound);
        }
        Ok(())
    }
}
