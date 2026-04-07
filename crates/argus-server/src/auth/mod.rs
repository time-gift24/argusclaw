//! Authentication module for the server product.
//!
//! Provides OAuth2 abstractions, dev OAuth2 flow, session management,
//! and HTTP routes for login/callback/logout.

pub mod dev_oauth;
pub mod provider;
pub mod routes;
pub mod session;

use thiserror::Error;

/// Authentication error type.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("invalid or expired authorization code")]
    InvalidCode,

    #[error("csrf state mismatch")]
    StateMismatch,

    #[error("session error: {reason}")]
    Session { reason: String },

    #[error("provider error: {reason}")]
    Provider { reason: String },
}
