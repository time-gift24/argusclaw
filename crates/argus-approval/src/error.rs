//! Approval error types.

use thiserror::Error;
use uuid::Uuid;

/// Errors that can occur during approval operations.
#[derive(Debug, Error)]
pub enum ApprovalError {
    /// No pending approval request with the given ID.
    #[error("No pending approval request with id {0}")]
    NotFound(Uuid),

    /// Validation failed for a request or policy.
    #[error("Validation failed: {0}")]
    Validation(String),

    /// Too many pending requests for an agent.
    #[error("Too many pending requests for agent {agent_id} (max: {max})")]
    TooManyPending {
        /// The agent ID that has too many pending requests.
        agent_id: String,
        /// The maximum allowed pending requests per agent.
        max: usize,
    },
}
