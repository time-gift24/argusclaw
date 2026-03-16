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

    #[error("username is required")]
    UsernameRequired,

    #[error("password is required")]
    PasswordRequired,

    #[error("password must be at least {min_length} characters")]
    PasswordTooShort { min_length: usize },

    #[error("Database error: {reason}")]
    DatabaseError { reason: String },

    #[error("Password hash error: {reason}")]
    HashError { reason: String },
}

pub type Result<T> = std::result::Result<T, UserError>;
