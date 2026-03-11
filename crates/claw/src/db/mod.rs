pub mod llm;
pub mod sqlite;
#[cfg(feature = "dev")]
pub mod approval;
#[cfg(feature = "dev")]
pub mod workflow;

use thiserror::Error;

#[cfg(feature = "dev")]
pub use approval::ApprovalRepository;

#[cfg(feature = "dev")]
pub use workflow::SqliteWorkflowRepository;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database connection failed: {reason}")]
    ConnectionFailed { reason: String },

    #[error("database migration failed: {reason}")]
    MigrationFailed { reason: String },

    #[error("database query failed: {reason}")]
    QueryFailed { reason: String },

    #[error("invalid llm provider kind `{kind}`")]
    InvalidProviderKind { kind: String },

    #[error("host key material is unavailable: {reason}")]
    HostKeyUnavailable { reason: String },

    #[error("failed to encrypt secret: {reason}")]
    SecretEncryptionFailed { reason: String },

    #[error("failed to decrypt secret: {reason}")]
    SecretDecryptionFailed { reason: String },
}
