use async_trait::async_trait;

use tm_trading_domain::AiModel;

use crate::RepositoryError;

#[async_trait]
pub trait AiModelRepository: Send + Sync {
    async fn list(&self) -> Result<Vec<AiModel>, RepositoryError>;
    async fn get(&self, id: &str) -> Result<Option<AiModel>, RepositoryError>;
    async fn upsert(&self, model: &AiModel) -> Result<(), RepositoryError>;
    async fn delete(&self, id: &str) -> Result<(), RepositoryError>;
    /// Removes every model of a deleted provider so no model outlives its provider.
    async fn delete_by_provider(&self, provider_id: &str) -> Result<u64, RepositoryError>;
}
