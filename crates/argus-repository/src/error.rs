//! Database error types.

use thiserror::Error;

use argus_protocol::llm::LlmProviderKindParseError;

/// Database error type.
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

    #[error("secret key material is unavailable: {reason}")]
    SecretKeyMaterialUnavailable { reason: String },

    #[error("failed to encrypt secret: {reason}")]
    SecretEncryptionFailed { reason: String },

    #[error("failed to decrypt secret: {reason}")]
    SecretDecryptionFailed { reason: String },

    #[error("record not found: {id}")]
    NotFound { id: String },
}

impl From<LlmProviderKindParseError> for DbError {
    fn from(err: LlmProviderKindParseError) -> Self {
        DbError::InvalidProviderKind {
            kind: err.to_string(),
        }
    }
}

impl From<DbError> for argus_protocol::ArgusError {
    fn from(err: DbError) -> Self {
        argus_protocol::ArgusError::DatabaseError {
            reason: err.to_string(),
        }
    }
}
