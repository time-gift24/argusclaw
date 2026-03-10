use thiserror::Error;

use crate::db::DbError;
use crate::llm::LlmError;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error(transparent)]
    Database(#[from] DbError),

    #[error(transparent)]
    Llm(#[from] LlmError),

    #[error("provider `{id}` was not found")]
    ProviderNotFound { id: String },

    #[error("no default provider is configured")]
    DefaultProviderNotConfigured,

    #[error("provider kind `{kind}` is not supported by this build")]
    UnsupportedProviderKind { kind: String },
}
