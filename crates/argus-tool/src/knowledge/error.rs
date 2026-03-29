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

    #[error("GitHub resource not found: {0}")]
    NotFound(String),

    #[error("GitHub API rate limited: {0}")]
    RateLimited(String),

    #[error("GitHub request failed: {0}")]
    RequestFailed(String),

    #[error("unexpected GitHub response: {0}")]
    UnexpectedResponse(String),
}

impl KnowledgeToolError {
    pub fn invalid_arguments(err: impl Into<String>) -> Self {
        Self::InvalidArguments(err.into())
    }

    pub fn unexpected_response(err: impl Into<String>) -> Self {
        Self::UnexpectedResponse(err.into())
    }
}
