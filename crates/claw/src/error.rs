use thiserror::Error;

use crate::agents::AgentId;
use crate::db::DbError;
use crate::llm::LlmError;
use crate::protocol::ThreadId;
use crate::tool::ToolError;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error(transparent)]
    Database(#[from] DbError),

    #[error(transparent)]
    Llm(#[from] LlmError),

    #[error(transparent)]
    Tool(#[from] ToolError),

    #[error("failed to resolve home directory for the default database path")]
    HomeDirectoryUnavailable,

    #[error("database path `{path}` has no parent directory")]
    InvalidDatabasePath { path: String },

    #[error("failed to create database directory `{path}`: {reason}")]
    DatabaseDirectoryCreateFailed { path: String, reason: String },

    #[error("provider `{id}` was not found")]
    ProviderNotFound { id: String },

    #[error("no default provider is configured")]
    DefaultProviderNotConfigured,

    #[error("provider kind `{kind}` is not supported by this build")]
    UnsupportedProviderKind { kind: String },

    #[error("agent `{id}` was not found")]
    AgentNotFound { id: AgentId },

    #[error("thread `{id}` was not found")]
    ThreadNotFound { id: ThreadId },

    #[error("approval is not configured for this agent")]
    ApprovalNotConfigured,

    #[error("approval resolution failed: {reason}")]
    ApprovalFailed { reason: String },
}
