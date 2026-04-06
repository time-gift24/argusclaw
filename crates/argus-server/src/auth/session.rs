//! Cookie-backed authentication session management.
//!
//! Uses HMAC-SHA256 signed cookies to persist the authenticated user ID.
//! The cookie value is `base64(user_id):base64(hmac_signature)`.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ring::hmac;

/// Key used for the session cookie.
pub const SESSION_COOKIE_NAME: &str = "argus_session";

/// Authentication session manager.
///
/// Holds an HMAC signing key for cookie-based sessions.
pub struct AuthSession {
    key: hmac::Key,
}

impl AuthSession {
    pub fn new(secret: &str) -> Self {
        let key = hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
        Self { key }
    }

    /// Create a signed session cookie value for the given user ID.
    pub fn create_session(&self, user_id: i64) -> String {
        let payload = user_id.to_string();
        let tag = hmac::sign(&self.key, payload.as_bytes());
        let sig = URL_SAFE_NO_PAD.encode(tag.as_ref());
        format!("{payload}:{sig}")
    }

    /// Verify a session cookie value and extract the user ID.
    /// Returns `None` if the cookie is malformed or the signature is invalid.
    pub fn verify_session(&self, cookie_value: &str) -> Option<i64> {
        let (payload, sig_b64) = cookie_value.split_once(':')?;
        let sig_bytes = URL_SAFE_NO_PAD.decode(sig_b64).ok()?;
        let user_id: i64 = payload.parse().ok()?;
        hmac::verify(&self.key, payload.as_bytes(), &sig_bytes).ok()?;
        Some(user_id)
    }
}
