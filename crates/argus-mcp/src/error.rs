use argus_protocol::ArgusError;
use argus_repository::DbError;

#[derive(Debug, thiserror::Error)]
pub enum McpRuntimeError {
    #[error("mcp repository operation failed: {reason}")]
    Repository { reason: String },
    #[error("mcp server {server_id} not found")]
    ServerNotFound { server_id: i64 },
    #[error("mcp server {server_id} is not ready")]
    ServerNotReady { server_id: i64 },
    #[error("mcp server {server_id} connection failed: {reason}")]
    ConnectFailed { server_id: i64, reason: String },
    #[error("mcp server {server_id} tool '{tool_name}' failed: {reason}")]
    ToolCallFailed {
        server_id: i64,
        tool_name: String,
        reason: String,
    },
    #[error("mcp server '{display_name}' is misconfigured: {reason}")]
    InvalidConfiguration {
        display_name: String,
        reason: String,
    },
    #[error("mcp serialization failed: {reason}")]
    Serialization { reason: String },
}

impl From<DbError> for McpRuntimeError {
    fn from(error: DbError) -> Self {
        Self::Repository {
            reason: error.to_string(),
        }
    }
}

impl From<McpRuntimeError> for ArgusError {
    fn from(error: McpRuntimeError) -> Self {
        match error {
            McpRuntimeError::Repository { reason } => Self::DatabaseError { reason },
            McpRuntimeError::Serialization { reason } => Self::SerdeError { reason },
            other => Self::LlmError {
                reason: other.to_string(),
            },
        }
    }
}
