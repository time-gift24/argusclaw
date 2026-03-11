//! Thread error types.

use thiserror::Error;

use crate::agents::turn::TurnError;

/// Errors that can occur during Thread operations.
#[derive(Debug, Error)]
pub enum ThreadError {
    /// Turn execution failed.
    #[error("Turn execution failed: {0}")]
    TurnFailed(#[from] TurnError),

    /// Compact operation failed.
    #[error("Compact failed: {reason}")]
    CompactFailed {
        /// Reason for the failure.
        reason: String,
    },

    /// Provider not configured.
    #[error("LLM provider not configured")]
    ProviderNotConfigured,

    /// Channel send error.
    #[error("Event channel closed")]
    ChannelClosed,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_error_display_compact_failed() {
        let err = ThreadError::CompactFailed {
            reason: "test reason".to_string(),
        };
        assert!(err.to_string().contains("test reason"));
    }

    #[test]
    fn thread_error_display_provider_not_configured() {
        let err = ThreadError::ProviderNotConfigured;
        assert!(err.to_string().contains("provider"));
    }

    #[test]
    fn thread_error_display_channel_closed() {
        let err = ThreadError::ChannelClosed;
        assert!(err.to_string().contains("channel"));
    }
}
