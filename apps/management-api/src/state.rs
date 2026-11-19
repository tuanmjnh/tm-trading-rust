use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tm_trading_contracts::ServiceSnapshot;
use tm_trading_storage::{InMemoryOperationRepository, OperationRepository};

/// Credentials and capabilities used in local deployment mode.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub user_token: String,
    pub agent_token: String,
    pub user_subject: String,
    pub agent_id: String,
}

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub operations: Arc<dyn OperationRepository>,
    pub auth: AuthConfig,
    services: Arc<RwLock<HashMap<String, ServiceSnapshot>>>,
}

impl AppState {
    #[must_use]
    pub fn new(auth: AuthConfig) -> Self {
        Self::with_repository(auth, Arc::new(InMemoryOperationRepository::new()))
    }

    #[must_use]
    pub fn with_repository(auth: AuthConfig, operations: Arc<dyn OperationRepository>) -> Self {
        Self {
            operations,
            auth,
            services: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Replaces the reported state of every service carried by an Agent heartbeat.
    ///
    /// The heartbeat is authoritative for that Agent, so instances the Agent no longer
    /// reports are removed instead of lingering as stale read-model entries.
    pub fn replace_agent_snapshots(
        &self,
        agent_id: &str,
        snapshots: Vec<ServiceSnapshot>,
    ) -> Result<(), StateError> {
        let mut services = self
            .services
            .write()
            .map_err(|_| StateError::LockPoisoned)?;
        services.retain(|_, snapshot| snapshot.agent_id != agent_id);
        for snapshot in snapshots {
            services.insert(snapshot.service_instance_id.clone(), snapshot);
        }
        Ok(())
    }

    pub fn list_snapshots(&self) -> Result<Vec<ServiceSnapshot>, StateError> {
        let services = self.services.read().map_err(|_| StateError::LockPoisoned)?;
        let mut snapshots: Vec<ServiceSnapshot> = services.values().cloned().collect();
        snapshots.sort_by(|left, right| left.service_instance_id.cmp(&right.service_instance_id));
        Ok(snapshots)
    }

    pub fn snapshot(
        &self,
        service_instance_id: &str,
    ) -> Result<Option<ServiceSnapshot>, StateError> {
        let services = self.services.read().map_err(|_| StateError::LockPoisoned)?;
        Ok(services.get(service_instance_id).cloned())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("service read model lock was poisoned")]
    LockPoisoned,
}
