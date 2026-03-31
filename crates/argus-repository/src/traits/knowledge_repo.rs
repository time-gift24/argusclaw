//! Knowledge repo repository trait.

use async_trait::async_trait;

use crate::error::DbError;
use crate::types::KnowledgeRepoRecord;

/// Repository trait for knowledge repo persistence.
#[async_trait]
pub trait KnowledgeRepoRepository: Send + Sync {
    /// Insert or update a knowledge repo, returning the row ID.
    async fn upsert(&self, repo: &str, workspace: &str) -> Result<i64, DbError>;

    /// Get a repo by database ID.
    async fn get(&self, id: i64) -> Result<Option<KnowledgeRepoRecord>, DbError>;

    /// Get a repo by its repo string.
    async fn find_by_repo(&self, repo: &str) -> Result<Option<KnowledgeRepoRecord>, DbError>;

    /// List all knowledge repos.
    async fn list(&self) -> Result<Vec<KnowledgeRepoRecord>, DbError>;

    /// Delete a repo by database ID.
    async fn delete(&self, id: i64) -> Result<bool, DbError>;

    /// List repos visible to a specific agent (via workspace binding).
    async fn list_repos_for_agent(&self, agent_id: i64) -> Result<Vec<KnowledgeRepoRecord>, DbError>;

    /// Set the workspace bindings for an agent (replaces all existing).
    async fn set_agent_workspaces(
        &self,
        agent_id: i64,
        workspaces: &[String],
    ) -> Result<(), DbError>;

    /// List workspace names bound to an agent.
    async fn list_agent_workspaces(&self, agent_id: i64) -> Result<Vec<String>, DbError>;
}
