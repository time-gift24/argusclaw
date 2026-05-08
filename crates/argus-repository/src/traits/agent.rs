//! Agent repository trait.

use async_trait::async_trait;

use crate::error::DbError;
use crate::types::{AgentDeleteReport, AgentId, AgentRecord};

/// Repository trait for agent persistence.
#[async_trait]
pub trait AgentRepository: Send + Sync {
    /// Create or update an agent, returning the agent's ID.
    async fn upsert(&self, record: &AgentRecord) -> Result<AgentId, DbError>;

    /// Get an agent by ID.
    async fn get(&self, id: &AgentId) -> Result<Option<AgentRecord>, DbError>;

    /// Find an agent by display name.
    async fn find_by_display_name(
        &self,
        display_name: &str,
    ) -> Result<Option<AgentRecord>, DbError>;

    /// Find an agent ID by display name.
    async fn find_id_by_display_name(&self, display_name: &str)
    -> Result<Option<AgentId>, DbError>;

    /// List all agents.
    async fn list(&self) -> Result<Vec<AgentRecord>, DbError>;

    /// Count references to an agent (threads and jobs that reference it).
    async fn count_references(&self, id: &AgentId) -> Result<(i64, i64, i64), DbError>;

    /// Delete an agent.
    async fn delete(&self, id: &AgentId) -> Result<bool, DbError>;

    /// Delete an agent and its explicit runtime associations.
    async fn delete_with_associations(&self, id: &AgentId) -> Result<AgentDeleteReport, DbError>;
}
