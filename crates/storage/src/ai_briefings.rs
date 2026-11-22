use async_trait::async_trait;

use tm_trading_domain::{AiBriefing, AiTaskKind};

use crate::RepositoryError;

/// Default number of briefings returned by an owner-scoped read.
pub const DEFAULT_AI_BRIEFING_LIMIT: usize = 50;

/// Filter of an owner-scoped briefing read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiBriefingQuery {
    pub owner_scope: String,
    pub task: Option<AiTaskKind>,
    pub limit: usize,
}

impl Default for AiBriefingQuery {
    fn default() -> Self {
        Self {
            owner_scope: String::new(),
            task: None,
            limit: DEFAULT_AI_BRIEFING_LIMIT,
        }
    }
}

/// Durable store for evidence-bound AI briefings.
///
/// Briefings are append-only publications. Replaying the same identifier keeps
/// the first published content instead of rewriting it.
#[async_trait]
pub trait AiBriefingRepository: Send + Sync {
    async fn upsert(&self, briefing: &AiBriefing) -> Result<(), RepositoryError>;

    /// Newest briefings first.
    async fn list(&self, query: &AiBriefingQuery) -> Result<Vec<AiBriefing>, RepositoryError>;

    /// Returns a briefing only when it belongs to the requested owner.
    async fn get(
        &self,
        owner_scope: &str,
        briefing_id: &str,
    ) -> Result<Option<AiBriefing>, RepositoryError>;
}
