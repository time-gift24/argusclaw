use thiserror::Error;

use crate::agents::AgentId;
use crate::db::DbError;
use crate::llm::LlmError;
use crate::protocol::ThreadId;
use crate::tool::ToolError;
use crate::user::UserError;

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

    #[error("model `{model}` is not available on provider `{provider}`")]
    ModelNotAvailable { provider: String, model: String },

    #[error("provider validation failed: {reason}")]
    ProviderValidationFailed { reason: String },

    #[error("agent validation failed: {reason}")]
    AgentValidationFailed { reason: String },

    #[error("agent `{id}` was not found")]
    AgentNotFound { id: AgentId },

    #[error("thread `{id}` was not found")]
    ThreadNotFound { id: ThreadId },

    #[error("approval is not configured for this agent")]
    ApprovalNotConfigured,

    #[error("approval resolution failed: {reason}")]
    ApprovalFailed { reason: String },

    #[error("agent build failed: required field `{field}` was not set")]
    AgentBuildFailed { field: &'static str },

    #[error("thread build failed: {reason}")]
    ThreadBuildFailed { reason: String },

    #[error("failed to load built-in agent: {reason}")]
    BuiltinAgentLoadFailed { reason: String },

    #[error("default agent not found")]
    DefaultAgentNotFound,

    #[error(transparent)]
    User(#[from] UserError),
}

#[cfg(test)]
mod tests {
    use super::AgentError;

    #[test]
    fn model_not_available_error_formats_correctly() {
        let error = AgentError::ModelNotAvailable {
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
        };
        let message = error.to_string();
        assert!(message.contains("openai"));
        assert!(message.contains("gpt-5"));
        assert!(message.contains("not available"));
    }

    #[test]
    fn provider_validation_failed_error_formats_correctly() {
        let error = AgentError::ProviderValidationFailed {
            reason: "models list is empty".to_string(),
        };
        let message = error.to_string();
        assert!(message.contains("validation failed"));
        assert!(message.contains("models list is empty"));
    }
}
