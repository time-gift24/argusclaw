use thiserror::Error;

use super::models::KnowledgeAction;

#[derive(Debug, Error)]
pub enum KnowledgeToolError {
    #[error("invalid knowledge arguments: {0}")]
    InvalidArguments(String),

    #[error("repo_id is required for action {action}")]
    RepoIdRequired { action: KnowledgeAction },

    #[error("snapshot_id is required for action {action}")]
    SnapshotIdRequired { action: KnowledgeAction },
}

impl KnowledgeToolError {
    pub fn invalid_arguments(err: impl Into<String>) -> Self {
        Self::InvalidArguments(err.into())
    }
}
