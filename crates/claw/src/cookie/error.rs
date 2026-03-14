//! Cookie module error types.

use thiserror::Error;

/// Errors from cookie operations.
#[derive(Debug, Error)]
pub enum CookieError {
    #[error("Failed to connect to Chrome: {reason}")]
    ConnectionFailed { reason: String },

    #[error("Chrome not running with remote debugging port")]
    DebuggingPortNotEnabled,

    #[error("CDP error: {0}")]
    #[cfg(feature = "cookie")]
    CdpError(#[from] chromiumoxide::error::CdpError),

    #[error("Invalid cookie format: {raw}")]
    InvalidCookieFormat { raw: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
