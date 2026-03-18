//! Agent repository trait.

use async_trait::async_trait;

use crate::error::DbError;
use crate::types::{AgentId, AgentRecord};

/// Repository trait for agent persistence.
#[async_trait]
pub trait AgentRepository: Send + Sync {
    /// Create or update an agent.
    async fn upsert(&self, record: &AgentRecord) -> Result<(), DbError>;

    /// Get an agent by ID.
    async fn get(&self, id: &AgentId) -> Result<Option<AgentRecord>, DbError>;

    /// Find an agent by display name.
    async fn find_by_display_name(
        &self,
        display_name: &str,
    ) -> Result<Option<AgentRecord>, DbError>;

    /// List all agents.
    async fn list(&self) -> Result<Vec<AgentRecord>, DbError>;

    /// Delete an agent.
    async fn delete(&self, id: &AgentId) -> Result<bool, DbError>;
}
