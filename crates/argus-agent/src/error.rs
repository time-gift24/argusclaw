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
    #[error("LLM provider not configured for TurnInput")]
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

    #[error("malformed JSON event at line {line}: {reason}")]
    MalformedEvent { line: usize, reason: String },

    #[error("unknown event type: {0}")]
    UnknownEventType(String),

    #[error("truncated event at line {line}: {reason}")]
    TruncatedEvent { line: usize, reason: String },
}

// ---------------------------------------------------------------------------
// TokenizationError, ThreadError & CompactError
// ---------------------------------------------------------------------------

/// Legacy tokenization errors kept for public API compatibility.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[deprecated(
    note = "Token counts are now approximate between turns and authoritative after LLM responses."
)]
pub enum TokenizationError {
    /// Required tokenizer asset was not found on disk.
    #[error("Tokenizer asset not found: {path}")]
    AssetMissing { path: PathBuf },

    /// Tokenizer asset path could not be represented as UTF-8.
    #[error("Tokenizer asset path is not valid UTF-8: {path:?}")]
    InvalidAssetPath { path: PathBuf },

    /// Tokenizer construction failed.
    #[error("Failed to build tokenizer from {vocab_path} and {merges_path}: {reason}")]
    BuildFailed {
        vocab_path: PathBuf,
        merges_path: PathBuf,
        reason: String,
    },

    /// Tokenizer encoding failed.
    #[error("Failed to encode text with tokenizer: {reason}")]
    EncodeFailed { reason: String },

    /// Token count exceeded the supported `u32` range.
    #[error("Token count exceeds supported range: {count}")]
    CountOverflow { count: usize },
}

/// Compact operation error.
#[derive(Debug, Error)]
#[allow(deprecated)]
pub enum CompactError {
    /// Compact failed with a reason.
    #[error("Compact failed: {reason}")]
    Failed {
        /// Reason for the failure.
        reason: String,
    },

    /// Tokenization failed while recalculating context size.
    #[error("Tokenization failed: {0}")]
    TokenizationFailed(#[from] TokenizationError),

    /// Summarize strategy not implemented.
    #[error("Summarize strategy not implemented")]
    SummarizeNotImplemented,
}

/// Errors that can occur during Thread operations.
#[derive(Debug, Error)]
#[allow(deprecated)]
pub enum ThreadError {
    /// Turn execution failed.
    #[error("Turn execution failed: {0}")]
    TurnFailed(#[from] TurnError),

    /// Turn build failed.
    #[error("Turn build failed: {0}")]
    TurnBuildFailed(String),

    /// MCP tools could not be resolved for the upcoming turn.
    #[error("Failed to resolve MCP tools: {reason}")]
    McpToolResolutionFailed { reason: String },

    /// Compact operation failed.
    #[error("Compact failed: {0}")]
    CompactFailed(#[from] CompactError),

    /// Tokenization failed while updating thread state.
    #[error("Tokenization failed: {0}")]
    TokenizationFailed(#[from] TokenizationError),

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
#[allow(deprecated)]
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
    fn tokenization_error_display_asset_missing() {
        let err = TokenizationError::AssetMissing {
            path: PathBuf::from("/tmp/missing.json"),
        };
        assert!(err.to_string().contains("missing.json"));
    }

    #[test]
    fn compact_error_display_tokenization_failed() {
        let err = CompactError::TokenizationFailed(TokenizationError::EncodeFailed {
            reason: "bad token".to_string(),
        });
        assert!(err.to_string().contains("bad token"));
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
    fn thread_error_display_tokenization_failed() {
        let err = ThreadError::TokenizationFailed(TokenizationError::EncodeFailed {
            reason: "boom".to_string(),
        });
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn thread_error_display_channel_closed() {
        let err = ThreadError::ChannelClosed;
        assert!(err.to_string().contains("channel"));
    }
}
