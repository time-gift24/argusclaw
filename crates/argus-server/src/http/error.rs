//! HTTP error mapping for the server API.

use axum::response::IntoResponse;

#[derive(Debug)]
pub enum ApiError {
    Unauthorized,
    NotFound,
    BadRequest(String),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            ApiError::Unauthorized => {
                (axum::http::StatusCode::UNAUTHORIZED, "not authenticated").into_response()
            }
            ApiError::NotFound => {
                (axum::http::StatusCode::NOT_FOUND, "resource not found").into_response()
            }
            ApiError::BadRequest(msg) => (axum::http::StatusCode::BAD_REQUEST, msg).into_response(),
            ApiError::Internal(msg) => {
                (axum::http::StatusCode::INTERNAL_SERVER_ERROR, msg).into_response()
            }
        }
    }
}

impl From<argus_session::UserChatError> for ApiError {
    fn from(error: argus_session::UserChatError) -> Self {
        match error {
            argus_session::UserChatError::NotFound
            | argus_session::UserChatError::AgentNotEnabled => Self::NotFound,
            argus_session::UserChatError::Internal { reason } => Self::Internal(reason),
        }
    }
}
