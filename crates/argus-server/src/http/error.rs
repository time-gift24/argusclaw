//! HTTP error mapping for the chat API.
//!
//! Converts service-layer errors into consistent HTTP responses.

use axum::response::IntoResponse;

/// API error type that maps to HTTP status codes.
#[derive(Debug)]
pub enum ApiError {
    /// The request lacks valid authentication.
    Unauthorized,
    /// The requested resource was not found or access denied.
    NotFound(String),
    /// The request was malformed.
    BadRequest(String),
    /// An internal server error occurred.
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            ApiError::Unauthorized => {
                (axum::http::StatusCode::UNAUTHORIZED, "not authenticated").into_response()
            }
            ApiError::NotFound(msg) => {
                (axum::http::StatusCode::NOT_FOUND, msg).into_response()
            }
            ApiError::BadRequest(msg) => {
                (axum::http::StatusCode::BAD_REQUEST, msg).into_response()
            }
            ApiError::Internal(msg) => {
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
            }
        }
    }
}

impl From<argus_protocol::ArgusError> for ApiError {
    fn from(err: argus_protocol::ArgusError) -> Self {
        match &err {
            argus_protocol::ArgusError::SessionNotFound(_)
            | argus_protocol::ArgusError::ThreadNotFound(_)
            | argus_protocol::ArgusError::TemplateNotFound(_) => {
                ApiError::NotFound(err.to_string())
            }
            _ => ApiError::Internal(err.to_string()),
        }
    }
}
