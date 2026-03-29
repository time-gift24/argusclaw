use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ChromeToolError {
    #[error("invalid arguments: {reason}")]
    InvalidArguments { reason: String },

    #[error("missing required field '{field}' for action '{action}'")]
    MissingRequiredField { action: String, field: &'static str },

    #[error("action '{action}' is not allowed")]
    ActionNotAllowed { action: String },

    #[error("session not found: {session_id}")]
    SessionNotFound { session_id: String },

    #[error("failed to create directory '{path:?}': {reason}")]
    DirectoryCreateFailed { path: PathBuf, reason: String },
}
