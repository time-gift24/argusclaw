//! Error types for crypto operations.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("host key material is unavailable: {reason}")]
    HostKeyUnavailable { reason: String },

    #[error("secret key material is unavailable: {reason}")]
    SecretKeyMaterialUnavailable { reason: String },

    #[error("failed to encrypt secret: {reason}")]
    SecretEncryptionFailed { reason: String },

    #[error("failed to decrypt secret: {reason}")]
    SecretDecryptionFailed { reason: String },
}
