//! Knowledge repo persistence types.

use serde::{Deserialize, Serialize};

/// A knowledge repo database record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeRepoRecord {
    /// Database row ID.
    pub id: i64,
    /// Repository URL or "owner/name".
    pub repo: String,
    /// Workspace / scenario tag.
    pub workspace: String,
}
