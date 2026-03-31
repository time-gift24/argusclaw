//! Knowledge repo persistence types.

use serde::{Deserialize, Serialize};

/// A knowledge repo database record.
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
