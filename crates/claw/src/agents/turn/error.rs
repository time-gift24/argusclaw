//! Turn error types.

use thiserror::Error;

/// Errors that can occur during turn execution.
#[derive(Debug, Error)]
pub enum TurnError {
    /// Tool execution failed.
    #[error("Tool execution failed: {0}")]
    ToolError(String),

    /// LLM provider error.
    #[error("LLM provider error: {0}")]
    LlmError(String),

    /// Maximum iterations exceeded.
    #[error("Maximum iterations exceeded: {0}")]
    MaxIterationsExceeded(u32),

    /// Maximum tool calls exceeded.
    #[error("Maximum tool calls exceeded: {0}")]
    MaxToolCallsExceeded(u32),

    /// Tool timeout.
    #[error("Tool execution timed out after {0} seconds")]
    ToolTimeout(u64),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    ConfigError(String),
}
