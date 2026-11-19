use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use tm_trading_domain::{NewProviderInstance, ProviderInstance};

use crate::repository::memory_id;
use crate::{ProviderInstanceRepository, RepositoryError};

#[derive(Default)]
pub struct InMemoryProviderInstanceRepository {
    instances: Mutex<HashMap<String, ProviderInstance>>,
}

impl InMemoryProviderInstanceRepository {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, HashMap<String, ProviderInstance>>, RepositoryError> {
        self.instances
            .lock()
            .map_err(|_| RepositoryError::LockPoisoned)
    }
}

#[async_trait]
impl ProviderInstanceRepository for InMemoryProviderInstanceRepository {
    async fn list(&self) -> Result<Vec<ProviderInstance>, RepositoryError> {
        let stored = self.lock()?;
        let mut instances: Vec<ProviderInstance> = stored.values().cloned().collect();
        instances.sort_by(|left, right| {
            left.created_at()
                .cmp(&right.created_at())
                .then_with(|| left.id().cmp(right.id()))
        });
        Ok(instances)
    }

    async fn get(
        &self,
        provider_instance_id: &str,
    ) -> Result<Option<ProviderInstance>, RepositoryError> {
        Ok(self.lock()?.get(provider_instance_id).cloned())
    }

    async fn create(
        &self,
        provider_definition_id: &str,
        display_name: &str,
        base_url: &str,
        timeout_ms: u64,
        now: DateTime<Utc>,
    ) -> Result<ProviderInstance, RepositoryError> {
        let instance = ProviderInstance::new(NewProviderInstance {
            id: memory_id(),
            provider_definition_id: provider_definition_id.to_owned(),
            display_name: display_name.to_owned(),
            base_url: base_url.to_owned(),
            timeout_ms,
            created_at: now,
        })?;
        self.lock()?
            .insert(instance.id().to_owned(), instance.clone());
        Ok(instance)
    }

    async fn update(
        &self,
        instance: ProviderInstance,
    ) -> Result<ProviderInstance, RepositoryError> {
        let mut stored = self.lock()?;
        if !stored.contains_key(instance.id()) {
            return Err(RepositoryError::ProviderInstanceNotFound);
        }
        stored.insert(instance.id().to_owned(), instance.clone());
        Ok(instance)
    }

    async fn record_health(
        &self,
        provider_instance_id: &str,
        observed_at: DateTime<Utc>,
        healthy: bool,
        latency_ms: Option<u64>,
        error_code: Option<&str>,
    ) -> Result<ProviderInstance, RepositoryError> {
        let mut stored = self.lock()?;
        let updated = stored
            .get(provider_instance_id)
            .cloned()
            .ok_or(RepositoryError::ProviderInstanceNotFound)?
            .with_health(observed_at, healthy, latency_ms, error_code);
        stored.insert(updated.id().to_owned(), updated.clone());
        Ok(updated)
    }

    async fn delete(&self, provider_instance_id: &str) -> Result<(), RepositoryError> {
        match self.lock()?.remove(provider_instance_id) {
            Some(_) => Ok(()),
            None => Err(RepositoryError::ProviderInstanceNotFound),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 19, 0, 0, 0).unwrap()
    }

    fn repository() -> InMemoryProviderInstanceRepository {
        InMemoryProviderInstanceRepository::new()
    }

    #[tokio::test]
    async fn creates_lists_and_deletes_an_instance() {
        let repository = repository();

        let created = repository
            .create(
                "binance",
                "Binance Public",
                "https://api.binance.com",
                5_000,
                at(),
            )
            .await
            .unwrap();
        assert!(created.enabled());
        assert_eq!(repository.list().await.unwrap(), vec![created.clone()]);

        repository.delete(created.id()).await.unwrap();
        assert!(repository.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn recording_health_keeps_the_configuration() {
        let repository = repository();
        let created = repository
            .create("okx", "OKX Public", "https://www.okx.com", 4_000, at())
            .await
            .unwrap();

        let probed = repository
            .record_health(created.id(), at(), true, Some(31), None)
            .await
            .unwrap();

        assert_eq!(probed.health_state().as_str(), "HEALTHY");
        assert_eq!(probed.last_latency_ms(), Some(31));
        assert_eq!(probed.display_name(), created.display_name());
        assert_eq!(probed.updated_at(), created.updated_at());
    }

    #[tokio::test]
    async fn a_failed_probe_records_a_sanitized_code() {
        let repository = repository();
        let created = repository
            .create(
                "binance",
                "Binance Public",
                "https://api.binance.com",
                5_000,
                at(),
            )
            .await
            .unwrap();

        let probed = repository
            .record_health(created.id(), at(), false, None, Some("timeout"))
            .await
            .unwrap();

        assert_eq!(probed.health_state().as_str(), "UNHEALTHY");
        assert_eq!(probed.last_error_code(), Some("timeout"));
        assert_eq!(probed.last_latency_ms(), None);
    }

    #[tokio::test]
    async fn unknown_ids_are_reported_as_not_found() {
        let repository = repository();

        assert!(matches!(
            repository.delete("missing").await,
            Err(RepositoryError::ProviderInstanceNotFound)
        ));
        assert!(matches!(
            repository
                .record_health("missing", at(), true, Some(1), None)
                .await,
            Err(RepositoryError::ProviderInstanceNotFound)
        ));
    }

    #[tokio::test]
    async fn invalid_configuration_is_rejected_before_it_is_stored() {
        let repository = repository();

        let result = repository
            .create(
                "binance",
                "Binance Public",
                "http://api.binance.com",
                5_000,
                at(),
            )
            .await;

        assert!(matches!(result, Err(RepositoryError::ProviderInstance(_))));
        assert!(repository.list().await.unwrap().is_empty());
    }
}
