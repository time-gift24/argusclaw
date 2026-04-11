//! Job-related errors.

use thiserror::Error;

/// Errors that can occur during job operations.
#[derive(Debug, Error)]
pub enum JobError {
    /// Agent not found.
    #[error("agent not found: {0}")]
    AgentNotFound(i64),

    /// Job not found.
    #[error("job not found: {0}")]
    JobNotFound(String),

    /// Job execution failed.
    #[error("job execution failed: {0}")]
    ExecutionFailed(String),

    /// Retry exhausted after max attempts.
    #[error("retry exhausted after max attempts: {0}")]
    RetryExhausted(String),

    /// Internal error.
    #[error("internal error: {0}")]
    Internal(String),

    /// Turn execution failed.
    #[error("turn execution failed: {0}")]
    TurnResult(String),

    /// Job is not in a running state.
    #[error("job is not running: {0}")]
    JobNotRunning(String),
}
