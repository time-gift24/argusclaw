use thiserror::Error;

#[derive(Debug, Error)]
pub enum UserError {
    #[error("User already exists: {username}")]
    UserAlreadyExists { username: String },

    #[error("User not found: {username}")]
    UserNotFound { username: String },

    #[error("Invalid password")]
    InvalidPassword,

    #[error("No user setup")]
    NoUserSetup,

    #[error("Database error: {reason}")]
    DatabaseError { reason: String },

    #[error("Password hash error: {reason}")]
    HashError { reason: String },
}

pub type Result<T> = std::result::Result<T, UserError>;
