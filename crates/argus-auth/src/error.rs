//! Error types for auth operations.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("account already exists")]
    AccountAlreadyExists,

    #[error("account not found")]
    AccountNotFound,

    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("encryption failed: {reason}")]
    EncryptionFailed { reason: String },

    #[error("decryption failed: {reason}")]
    DecryptionFailed { reason: String },

    #[error("database error: {reason}")]
    DatabaseError { reason: String },

    #[error("token fetch failed: {reason}")]
    TokenFetchFailed { reason: String },

    #[error("token not available")]
    TokenNotAvailable,

    #[error("HTTP error: {reason}")]
    HttpError { reason: String },
}

impl From<sqlx::Error> for AuthError {
    fn from(e: sqlx::Error) -> Self {
        AuthError::DatabaseError {
            reason: e.to_string(),
        }
    }
}

impl From<argus_crypto::CryptoError> for AuthError {
    fn from(e: argus_crypto::CryptoError) -> Self {
        match e {
            argus_crypto::CryptoError::SecretEncryptionFailed { reason } => {
                AuthError::EncryptionFailed { reason }
            }
            argus_crypto::CryptoError::SecretDecryptionFailed { reason } => {
                AuthError::DecryptionFailed { reason }
            }
            _ => AuthError::DecryptionFailed {
                reason: e.to_string(),
            },
        }
    }
}
