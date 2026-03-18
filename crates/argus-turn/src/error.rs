//! Turn error types.

use thiserror::Error;

use argus_protocol::llm::LlmError;

/// Errors that can occur during Turn execution.
#[derive(Debug, Error)]
pub enum TurnError {
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
}
