use thiserror::Error;

use crate::agents::AgentRuntimeId;
use crate::agents::thread::ThreadId;
use crate::db::DbError;
use crate::llm::LlmError;
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

    #[error(
        "no default provider is configured. Run `argusclaw provider set-default --id <provider-id>` to set one, or create a provider with `--default` flag"
    )]
    DefaultProviderNotConfigured,

    #[error("provider kind `{kind}` is not supported by this build")]
    UnsupportedProviderKind { kind: String },

    #[error("argus agent not found in database")]
    ArgusAgentNotFound,

    #[error("agent not found: {0}")]
    AgentNotFound(AgentRuntimeId),

    #[error("thread not found: {id}")]
    ThreadNotFound { id: ThreadId },

    #[error("failed to parse agent runtime id")]
    AgentRuntimeIdParseError(String),
}
