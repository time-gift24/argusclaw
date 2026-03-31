use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ApiError::NotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            ApiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };
        (status, Json(ErrorBody { error: message })).into_response()
    }
}

impl From<argus_protocol::ArgusError> for ApiError {
    fn from(e: argus_protocol::ArgusError) -> Self {
        use argus_protocol::ArgusError::*;
        match &e {
            SessionNotFound(_)
            | ThreadNotFound(_)
            | ProviderNotFound(_)
            | TemplateNotFound(_) => ApiError::NotFound(e.to_string()),
            SessionAlreadyLoaded(_)
            | SessionNotLoaded(_)
            | DefaultProviderNotConfigured => ApiError::BadRequest(e.to_string()),
            _ => ApiError::Internal(e.to_string()),
        }
    }
}
