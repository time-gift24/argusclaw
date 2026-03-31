//! Knowledge repository types and provider trait.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::ids::AgentId;

/// A knowledge repository record from the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeRepoRecord {
    /// Database row ID.
    pub id: i64,
    /// Legacy-compatible repository lookup key, typically "owner/name".
    pub repo: String,
    /// Stable repository identifier used by the knowledge runtime.
    pub repo_id: String,
    /// Repository provider, e.g. "github".
    pub provider: String,
    /// Repository owner or organization.
    pub owner: String,
    /// Repository name.
    pub name: String,
    /// Default branch used for snapshot resolution.
    pub default_branch: String,
    /// Optional manifest paths to probe inside the repository.
    pub manifest_paths: Vec<String>,
    /// Workspace / scenario tag.
    pub workspace: String,
}

/// Provider trait for knowledge repo lookup, agent-scoped.
///
/// Implemented by the persistence layer (ArgusSqlite) and injected into
/// `KnowledgeTool` at construction time.
#[async_trait]
pub trait KnowledgeRepoProvider: Send + Sync {
    /// List repos visible to an agent.
    ///
    /// When `agent_id` is `Some`, only repos belonging to workspaces bound to
    /// that agent are returned. When `None`, all repos are returned.
    async fn list_repos(
        &self,
        agent_id: Option<AgentId>,
    ) -> Result<Vec<KnowledgeRepoRecord>, Box<dyn std::error::Error + Send + Sync>>;

    /// Get a specific repo, validating it is accessible by the agent.
    async fn get_repo(
        &self,
        repo: &str,
        agent_id: Option<AgentId>,
    ) -> Result<KnowledgeRepoRecord, Box<dyn std::error::Error + Send + Sync>>;
}
