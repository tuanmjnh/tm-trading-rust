use async_trait::async_trait;

use tm_trading_domain::AiTaskRoute;

use crate::RepositoryError;

#[async_trait]
pub trait AiTaskRouteRepository: Send + Sync {
    async fn list(&self) -> Result<Vec<AiTaskRoute>, RepositoryError>;
    async fn get(&self, task: &str) -> Result<Option<AiTaskRoute>, RepositoryError>;
    async fn upsert(&self, route: &AiTaskRoute) -> Result<(), RepositoryError>;
    async fn delete(&self, task: &str) -> Result<(), RepositoryError>;
}
