//! LLM provider error types.
//!
//! Derived from:
//! - Repository: https://github.com/nearai/ironclaw
//! - Upstream path: src/llm/error.rs
//! - Upstream commit: bcef04b82108222c9041e733de459130badd4cd7
//! - License: MIT OR Apache-2.0
//!
//! Local modifications:
//! - Reduced to provider-agnostic core error variants.
//! - Excludes transport-specific `From` conversions from concrete provider crates.

use std::time::Duration;

use thiserror::Error;

/// Unified error type for provider-agnostic LLM operations.
#[derive(Debug, Error)]
pub enum LlmError {
    /// Provider returned an error for a request.
    #[error("Provider {provider} request failed: {reason}")]
    RequestFailed { provider: String, reason: String },

    /// Provider is rate limited and may indicate a retry window.
    #[error("Provider {provider} rate limited, retry after {retry_after:?}")]
    RateLimited {
        provider: String,
        retry_after: Option<Duration>,
    },

    /// Provider response was malformed or unexpected.
    #[error("Invalid response from {provider}: {reason}")]
    InvalidResponse { provider: String, reason: String },

    /// Request exceeded the model context window.
    #[error("Context length exceeded: {used} tokens used, {limit} allowed")]
    ContextLengthExceeded { used: usize, limit: usize },

    /// Requested model is not available on the provider.
    #[error("Model {model} not available on provider {provider}")]
    ModelNotAvailable { provider: String, model: String },

    /// Authentication failed for the provider.
    #[error("Authentication failed for provider {provider}")]
    AuthFailed { provider: String },

    /// Provider session expired and requires renewal.
    #[error("Session expired for provider {provider}")]
    SessionExpired { provider: String },

    /// Session renewal failed.
    #[error("Session renewal failed for provider {provider}: {reason}")]
    SessionRenewalFailed { provider: String, reason: String },
}
