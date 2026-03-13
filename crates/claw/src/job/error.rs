use thiserror::Error;

use crate::db::DbError;

/// Errors that can occur in the job module.
#[derive(Debug, Error)]
pub enum JobError {
    #[error(transparent)]
    Database(#[from] DbError),

    #[error("job `{id}` not found")]
    NotFound { id: String },

    #[error("invalid job type: {value}")]
    InvalidJobType { value: String },

    #[error("agent `{id}` not found")]
    AgentNotFound { id: String },

    #[error("agent creation failed")]
    AgentCreationFailed,

    #[error("thread not found")]
    ThreadNotFound,

    #[error("job execution failed: {reason}")]
    ExecutionFailed { reason: String },

    #[error("job timed out")]
    Timeout,

    #[error("job was cancelled")]
    Cancelled,

    #[error("channel closed unexpectedly")]
    ChannelClosed,

    #[error("too many concurrent jobs (max: {0})")]
    ConcurrencyLimit(usize),

    #[error("job result already consumed")]
    AlreadyConsumed,
}

#[cfg(test)]
mod extended_error_tests {
    use super::*;

    #[test]
    fn job_error_display() {
        let err = JobError::NotFound {
            id: "job-123".to_string(),
        };
        assert!(err.to_string().contains("job-123"));

        let err = JobError::AgentNotFound {
            id: "agent-456".to_string(),
        };
        assert!(err.to_string().contains("agent-456"));

        let err = JobError::AgentCreationFailed;
        assert!(err.to_string().contains("agent creation failed"));

        let err = JobError::ThreadNotFound;
        assert!(err.to_string().contains("thread not found"));

        let err = JobError::ExecutionFailed {
            reason: "bad state".to_string(),
        };
        assert!(err.to_string().contains("bad state"));

        let err = JobError::Timeout;
        assert!(err.to_string().contains("timed out"));

        let err = JobError::Cancelled;
        assert!(err.to_string().contains("cancelled"));

        let err = JobError::ChannelClosed;
        assert!(err.to_string().contains("channel closed"));

        let err = JobError::ConcurrencyLimit(10);
        assert!(err.to_string().contains("10"));

        let err = JobError::AlreadyConsumed;
        assert!(err.to_string().contains("already consumed"));
    }
}
