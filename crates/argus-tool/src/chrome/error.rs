#[derive(Debug, thiserror::Error)]
pub enum ChromeToolError {
    #[error("invalid arguments: {reason}")]
    InvalidArguments { reason: String },

    #[error("missing required field '{field}' for action '{action}'")]
    MissingRequiredField { action: String, field: &'static str },

    #[error("action '{action}' is not allowed")]
    ActionNotAllowed { action: String },
}
