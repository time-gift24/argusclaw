//! Error types for Turn and Thread execution.

use std::path::PathBuf;

use thiserror::Error;

use argus_protocol::llm::LlmError;

// ---------------------------------------------------------------------------
// TurnError
// ---------------------------------------------------------------------------

/// Errors that can occur during Turn execution.
#[derive(Debug, Error)]
pub enum TurnError {
    /// Turn was cancelled.
    #[error("Turn cancelled")]
    Cancelled,

    /// LLM call failed.
    #[error("LLM call failed: {0}")]
    LlmFailed(#[from] LlmError),

    /// LLM call blocked by hook.
    #[error("LLM call blocked by hook: {reason}")]
    LlmCallBlocked { reason: String },

    /// Tool not found in registry.
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    /// Tool execution failed.
    #[error("Tool '{name}' execution failed: {reason}")]
    ToolExecutionFailed { name: String, reason: String },

    /// Tool call blocked by hook.
    #[error("Tool call blocked by hook: {reason}")]
    ToolCallBlocked { reason: String },

    /// Maximum iterations reached.
    #[error("Maximum iterations ({0}) reached")]
    MaxIterationsReached(u32),

    /// Context length exceeded.
    #[error("Context length exceeded: {0} tokens")]
    ContextLengthExceeded(usize),

    /// Turn timeout exceeded.
    #[error("Turn timeout exceeded")]
    TimeoutExceeded,

    /// LLM provider not configured.
    #[error("LLM provider not configured for Turn")]
    ProviderNotConfigured,

    /// Turn builder failed (missing required field).
    #[error("Turn build failed: {0}")]
    BuildFailed(String),
}

// Implement From<UninitializedFieldError> for derive_builder compatibility
impl From<derive_builder::UninitializedFieldError> for TurnError {
    fn from(err: derive_builder::UninitializedFieldError) -> Self {
        TurnError::BuildFailed(err.to_string())
    }
}

/// Errors for turn log recovery operations.
#[derive(Debug, Error)]
pub enum TurnLogError {
    #[error("turn file not found: {0}")]
    TurnNotFound(PathBuf),

    #[error("thread metadata not found: {0}")]
    ThreadMetadataNotFound(PathBuf),

    #[error("thread metadata I/O failed at {path}: {reason}")]
    ThreadMetadataIo { path: PathBuf, reason: String },

    #[error("thread metadata malformed at {path}: {reason}")]
    ThreadMetadataMalformed { path: PathBuf, reason: String },

    #[error("malformed JSON event at line {line}: {reason}")]
    MalformedEvent { line: usize, reason: String },

    #[error("unknown event type: {0}")]
    UnknownEventType(String),

    #[error("truncated event at line {line}: {reason}")]
    TruncatedEvent { line: usize, reason: String },

    #[error("out-of-order seq at line {line}: expected {expected} but found {found}")]
    OutOfOrderSeq {
        line: usize,
        expected: u64,
        found: u64,
    },

    #[error(
        "non-monotonic user turn numbers at line {line}: expected turn {expected} but found {found}"
    )]
    NonMonotonicTurnNumber {
        line: usize,
        expected: u32,
        found: u32,
    },

    #[error(
        "checkpoint through_turn {through_turn} exceeds history turn count {turn_count} at line {line}"
    )]
    CheckpointBeyondHistory {
        line: usize,
        through_turn: u32,
        turn_count: u32,
    },
}

// ---------------------------------------------------------------------------
// ThreadError & CompactError
// ---------------------------------------------------------------------------

/// Compact operation error.
#[derive(Debug, Error)]
pub enum CompactError {
    /// Compact failed with a reason.
    #[error("Compact failed: {reason}")]
    Failed {
        /// Reason for the failure.
        reason: String,
    },

    /// Summarize strategy not implemented.
    #[error("Summarize strategy not implemented")]
    SummarizeNotImplemented,
}

/// Errors that can occur during Thread operations.
#[derive(Debug, Error)]
pub enum ThreadError {
    /// Turn execution failed.
    #[error("Turn execution failed: {0}")]
    TurnFailed(#[from] TurnError),

    /// Turn build failed.
    #[error("Turn build failed: {0}")]
    TurnBuildFailed(String),

    /// Compact operation failed.
    #[error("Compact failed: {0}")]
    CompactFailed(#[from] CompactError),

    /// Provider not configured.
    #[error("LLM provider not configured")]
    ProviderNotConfigured,

    /// Compactor not configured.
    #[error("Compactor not configured")]
    CompactorNotConfigured,

    /// Agent record not set.
    #[error("Agent record not set")]
    AgentRecordNotSet,

    /// Session ID not set.
    #[error("Session ID not set")]
    SessionIdNotSet,

    /// Channel send error.
    #[error("Event channel closed")]
    ChannelClosed,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_turn_error_display() {
        let err = TurnError::ToolNotFound("missing_tool".to_string());
        assert!(err.to_string().contains("missing_tool"));
    }

    #[test]
    fn test_turn_error_from_llm_error() {
        use argus_protocol::llm::LlmError;

        let llm_err = LlmError::RequestFailed {
            provider: "test".to_string(),
            reason: "timeout".to_string(),
        };
        let turn_err = TurnError::from(llm_err);
        assert!(matches!(turn_err, TurnError::LlmFailed(_)));
    }

    #[test]
    fn test_tool_execution_failed_display() {
        let err = TurnError::ToolExecutionFailed {
            name: "search".to_string(),
            reason: "network error".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("search"));
        assert!(msg.contains("network error"));
    }

    #[test]
    fn test_tool_call_blocked_display() {
        let err = TurnError::ToolCallBlocked {
            reason: "unsafe operation".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("blocked"));
        assert!(msg.contains("unsafe operation"));
    }

    #[test]
    fn test_llm_call_blocked_display() {
        let err = TurnError::LlmCallBlocked {
            reason: "rate limit exceeded".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("LLM call blocked"));
        assert!(msg.contains("rate limit exceeded"));
    }

    #[test]
    fn test_provider_not_configured_display_mentions_turn() {
        let err = TurnError::ProviderNotConfigured;
        assert_eq!(err.to_string(), "LLM provider not configured for Turn");
    }

    #[test]
    fn test_max_iterations_reached_display() {
        let err = TurnError::MaxIterationsReached(10);
        let msg = err.to_string();
        assert!(msg.contains("10"));
    }

    #[test]
    fn test_context_length_exceeded_display() {
        let err = TurnError::ContextLengthExceeded(4096);
        let msg = err.to_string();
        assert!(msg.contains("4096"));
    }

    #[test]
    fn test_timeout_exceeded_display() {
        let err = TurnError::TimeoutExceeded;
        let msg = err.to_string();
        assert!(msg.contains("timeout"));
    }

    #[test]
    fn compact_error_display_failed() {
        let err = CompactError::Failed {
            reason: "test reason".to_string(),
        };
        assert!(err.to_string().contains("test reason"));
    }

    #[test]
    fn compact_error_display_summarize_not_implemented() {
        let err = CompactError::SummarizeNotImplemented;
        assert!(err.to_string().contains("not implemented"));
    }

    #[test]
    fn thread_error_display_compact_failed() {
        let err = ThreadError::CompactFailed(CompactError::Failed {
            reason: "test reason".to_string(),
        });
        assert!(err.to_string().contains("test reason"));
    }

    #[test]
    fn thread_error_display_provider_not_configured() {
        let err = ThreadError::ProviderNotConfigured;
        assert!(err.to_string().contains("provider"));
    }

    #[test]
    fn thread_error_display_channel_closed() {
        let err = ThreadError::ChannelClosed;
        assert!(err.to_string().contains("channel"));
    }
}
