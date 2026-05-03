use axum::extract::FromRequestParts;
use axum::http::header::HeaderName;
use axum::http::request::Parts;

use crate::auth::AuthenticatedUser;
use crate::error::ApiError;

pub(crate) const USER_ID_HEADER: HeaderName = HeaderName::from_static("x-argus-user-id");
pub(crate) const USER_NAME_HEADER: HeaderName = HeaderName::from_static("x-argus-user-name");
const MAX_EXTERNAL_USER_FIELD_LEN: usize = 256;

/// Server-side chat user context extracted from trusted reverse-proxy headers.
///
/// `external_id` is the stable header value that the repository maps to an
/// internal `users.id` before chat data is read or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestUser {
    external_id: String,
    display_name: Option<String>,
}

impl RequestUser {
    pub(crate) fn new(external_id: String, display_name: Option<String>) -> Self {
        Self {
            external_id,
            display_name,
        }
    }

    pub(crate) fn external_id(&self) -> &str {
        &self.external_id
    }

    pub(crate) fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }
}

impl<S> FromRequestParts<S> for RequestUser
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(user) = parts.extensions.get::<AuthenticatedUser>() {
            return Ok(Self::new(
                user.external_id().to_string(),
                user.display_name().map(str::to_string),
            ));
        }

        let external_id = required_header(parts, &USER_ID_HEADER)?;
        let display_name = optional_header(parts, &USER_NAME_HEADER)?;
        Ok(Self::new(external_id, display_name))
    }
}

fn required_header(parts: &Parts, name: &HeaderName) -> Result<String, ApiError> {
    let value = parts
        .headers
        .get(name)
        .ok_or_else(|| ApiError::unauthorized(format!("missing required header {name}")))?;
    normalize_header(
        name,
        value
            .to_str()
            .map_err(|_| ApiError::unauthorized(format!("header {name} must be visible ASCII")))?,
    )?
    .ok_or_else(|| ApiError::unauthorized(format!("header {name} must not be empty")))
}

fn optional_header(parts: &Parts, name: &HeaderName) -> Result<Option<String>, ApiError> {
    parts
        .headers
        .get(name)
        .map(|value| {
            normalize_header(
                name,
                value.to_str().map_err(|_| {
                    ApiError::unauthorized(format!("header {name} must be visible ASCII"))
                })?,
            )
        })
        .transpose()
        .map(|value| value.flatten())
}

fn normalize_header(name: &HeaderName, value: &str) -> Result<Option<String>, ApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() > MAX_EXTERNAL_USER_FIELD_LEN {
        return Err(ApiError::unauthorized(format!(
            "header {name} exceeds {MAX_EXTERNAL_USER_FIELD_LEN} bytes"
        )));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(ApiError::unauthorized(format!(
            "header {name} contains control characters"
        )));
    }
    Ok(Some(trimmed.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    #[tokio::test]
    async fn request_user_requires_external_id_header() {
        let (mut parts, ()) = Request::builder().body(()).unwrap().into_parts();
        let error = RequestUser::from_request_parts(&mut parts, &())
            .await
            .expect_err("missing user header should fail closed");
        assert!(matches!(error, ApiError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn request_user_extracts_external_id_and_display_name() {
        let request = Request::builder()
            .header(USER_ID_HEADER, " external-123 ")
            .header(USER_NAME_HEADER, " Ada ")
            .body(())
            .unwrap();
        let (mut parts, ()) = request.into_parts();
        let user = RequestUser::from_request_parts(&mut parts, &())
            .await
            .expect("valid headers should extract");
        assert_eq!(user.external_id(), "external-123");
        assert_eq!(user.display_name(), Some("Ada"));
    }
}
