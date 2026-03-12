pub mod approval;
pub mod llm;
pub mod sqlite;
pub mod thread;
#[cfg(feature = "dev")]
pub mod workflow;

use thiserror::Error;

#[cfg(feature = "dev")]
pub use approval::ApprovalRepository;
pub use sqlite::SqliteJobRepository;
pub use thread::{MessageId, MessageRecord, ThreadRecord, ThreadRepository};

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
