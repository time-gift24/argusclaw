use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArgusError {
    #[error("Session not found: {0}")]
    SessionNotFound(i64),

    #[error("Session already loaded: {0}")]
    SessionAlreadyLoaded(i64),

    #[error("Session not loaded: {0}")]
    SessionNotLoaded(i64),

    #[error("Thread not found: {0}")]
    ThreadNotFound(String),

    #[error("Thread build failed: {reason}")]
    ThreadBuildFailed { reason: String },

    #[error("Template not found: {0}")]
    TemplateNotFound(i64),

    #[error("Provider not found: {0}")]
    ProviderNotFound(i64),

    #[error("No default provider configured")]
    DefaultProviderNotConfigured,

    #[error("Turn log error: {reason}")]
    TurnLogError { reason: String },

    #[error("Database error: {reason}")]
    DatabaseError { reason: String },

    #[error("LLM error: {reason}")]
    LlmError { reason: String },

    #[error("Approval error: {reason}")]
    ApprovalError { reason: String },

    #[error("IO error: {reason}")]
    IoError { reason: String },

    #[error("Serialization error: {reason}")]
    SerdeError { reason: String },
}

pub type Result<T> = std::result::Result<T, ArgusError>;
