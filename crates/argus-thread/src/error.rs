//! Thread error types.

use thiserror::Error;

use argus_turn::TurnError;

/// Compact operation error.
#[derive(Debug, Error)]
pub enum CompactError {
    /// Compact failed with a reason.
    #[error("Compact failed: {reason}")]
    Failed {
        /// Reason for the failure.
        reason: String,
    },

    /// Summarize strategy not implemented.
    #[error("Summarize strategy not implemented")]
    SummarizeNotImplemented,
}

/// Errors that can occur during Thread operations.
#[derive(Debug, Error)]
pub enum ThreadError {
    /// Turn execution failed.
    #[error("Turn execution failed: {0}")]
    TurnFailed(#[from] TurnError),

    /// Compact operation failed.
    #[error("Compact failed: {0}")]
    CompactFailed(#[from] CompactError),

    /// Provider not configured.
    #[error("LLM provider not configured")]
    ProviderNotConfigured,

    /// Compactor not configured.
    #[error("Compactor not configured")]
    CompactorNotConfigured,

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
    fn compact_error_display_failed() {
        let err = CompactError::Failed {
            reason: "test reason".to_string(),
        };
        assert!(err.to_string().contains("test reason"));
    }

    #[test]
    fn compact_error_display_summarize_not_implemented() {
        let err = CompactError::SummarizeNotImplemented;
        assert!(err.to_string().contains("not implemented"));
    }

    #[test]
    fn thread_error_display_compact_failed() {
        let err = ThreadError::CompactFailed(CompactError::Failed {
            reason: "test reason".to_string(),
        });
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
