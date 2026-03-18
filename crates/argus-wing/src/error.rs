//! Error types for the argus-wing crate.

use thiserror::Error;

/// Errors that can occur in the ArgusWing.
#[derive(Debug, Error)]
pub enum WingError {
    /// Provider not found.
    #[error("Provider not found: {0}")]
    ProviderNotFound(i64),

    /// Template not found.
    #[error("Template not found: {0}")]
    TemplateNotFound(i64),

    /// Session not found.
    #[error("Session not found: {0}")]
    SessionNotFound(i64),

    /// Thread not found.
    #[error("Thread not found: {0}")]
    ThreadNotFound(String),

    /// Database error.
    #[error("Database error: {reason}")]
    DatabaseError {
        /// The reason for the error.
        reason: String,
    },

    /// Provider error.
    #[error("Provider error: {reason}")]
    ProviderError {
        /// The reason for the error.
        reason: String,
    },

    /// Approval error.
    #[error("Approval error: {reason}")]
    ApprovalError {
        /// The reason for the error.
        reason: String,
    },
}
