//! MCP client error types.

use thiserror::Error;

/// Result type for MCP client operations.
pub type Result<T> = std::result::Result<T, McpClientError>;

/// Error type for MCP client operations.
#[derive(Debug, Error)]
pub enum McpClientError {
    /// Connection failed.
    #[error("Failed to connect to MCP server '{server}': {reason}")]
    ConnectionFailed { server: String, reason: String },

    /// Protocol version mismatch.
    #[error("Protocol version mismatch with '{server}': {reason}")]
    ProtocolMismatch { server: String, reason: String },

    /// Tool not found on server.
    #[error("Tool '{tool}' not found on server '{server}'")]
    ToolNotFound { server: String, tool: String },

    /// Tool call failed.
    #[error("Tool call failed on '{server}/{tool}': {reason}")]
    ToolCallFailed {
        server: String,
        tool: String,
        reason: String,
    },

    /// Transport error.
    #[error("Transport error for '{server}': {reason}")]
    TransportError { server: String, reason: String },

    /// Server timeout.
    #[error("Server '{server}' timed out after {timeout_secs}s")]
    Timeout { server: String, timeout_secs: u64 },

    /// Invalid server configuration.
    #[error("Invalid configuration for server '{server}': {reason}")]
    InvalidConfig { server: String, reason: String },

    /// Encryption/decryption error.
    #[error("Secret error for server '{server}': {reason}")]
    SecretError { server: String, reason: String },
}

impl From<McpClientError> for argus_protocol::ToolError {
    fn from(err: McpClientError) -> Self {
        match err {
            McpClientError::ConnectionFailed { server, reason } => {
                argus_protocol::ToolError::McpToolError {
                    server,
                    tool: String::new(),
                    context: format!("connection failed: {reason}"),
                    source: None,
                }
            }
            McpClientError::ProtocolMismatch { server, reason } => {
                argus_protocol::ToolError::McpToolError {
                    server,
                    tool: String::new(),
                    context: format!("protocol mismatch: {reason}"),
                    source: None,
                }
            }
            McpClientError::ToolNotFound { server, tool } => argus_protocol::ToolError::NotFound {
                id: format!("{server}/{tool}"),
            },
            McpClientError::ToolCallFailed {
                server,
                tool,
                reason,
            } => argus_protocol::ToolError::McpToolError {
                server,
                tool,
                context: reason,
                source: None,
            },
            McpClientError::TransportError { server, reason } => {
                argus_protocol::ToolError::McpToolError {
                    server,
                    tool: String::new(),
                    context: format!("transport error: {reason}"),
                    source: None,
                }
            }
            McpClientError::Timeout {
                server,
                timeout_secs,
            } => argus_protocol::ToolError::McpToolError {
                server,
                tool: String::new(),
                context: format!("timeout after {}s", timeout_secs),
                source: None,
            },
            McpClientError::InvalidConfig { server, reason } => {
                argus_protocol::ToolError::McpToolError {
                    server,
                    tool: String::new(),
                    context: format!("invalid config: {reason}"),
                    source: None,
                }
            }
            McpClientError::SecretError { server, reason } => {
                argus_protocol::ToolError::McpToolError {
                    server,
                    tool: String::new(),
                    context: format!("secret error: {reason}"),
                    source: None,
                }
            }
        }
    }
}
