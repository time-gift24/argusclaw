use thiserror::Error;

/// Errors that can occur in dev tools operations.
#[derive(Error, Debug)]
pub enum DevError {
    #[error("Turn execution failed: {reason}")]
    TurnFailed { reason: String },

    #[error("Database error: {reason}")]
    DatabaseError { reason: String },

    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    #[error("Other error: {reason}")]
    Other { reason: String },
}

/// Result type for dev tools operations.
pub type Result<T> = std::result::Result<T, DevError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = DevError::ProviderNotFound("test-provider".to_string());
        assert!(err.to_string().contains("Provider not found"));
        assert!(err.to_string().contains("test-provider"));
    }

    #[test]
    fn test_error_display_session() {
        let err = DevError::SessionNotFound("test-session".to_string());
        assert!(err.to_string().contains("Session not found"));
        assert!(err.to_string().contains("test-session"));
    }

    #[test]
    fn test_error_display_config() {
        let err = DevError::InvalidConfiguration("invalid timeout".to_string());
        assert!(err.to_string().contains("Invalid configuration"));
        assert!(err.to_string().contains("invalid timeout"));
    }

    #[test]
    fn test_error_named_fields() {
        let err = DevError::DatabaseError {
            reason: "connection failed".to_string(),
        };
        assert!(err.to_string().contains("Database error"));
        assert!(err.to_string().contains("connection failed"));
    }
}
