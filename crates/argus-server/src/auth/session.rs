//! Cookie-backed authentication helpers.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::hmac;

pub const OAUTH_STATE_COOKIE_NAME: &str = "argus_oauth_state";
pub const SESSION_COOKIE_NAME: &str = "argus_session";

/// HMAC signer for browser-bound values stored in cookies.
pub struct AuthSession {
    key: hmac::Key,
}

impl AuthSession {
    pub fn new(secret: &str) -> Self {
        let key = hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
        Self { key }
    }

    pub fn create_session(&self, user_id: i64) -> String {
        self.sign_value(&user_id.to_string())
    }

    pub fn verify_session(&self, cookie_value: &str) -> Option<i64> {
        self.verify_value(cookie_value)?.parse().ok()
    }

    pub fn sign_value(&self, payload: &str) -> String {
        let tag = hmac::sign(&self.key, payload.as_bytes());
        let sig = URL_SAFE_NO_PAD.encode(tag.as_ref());
        format!("{payload}:{sig}")
    }

    pub fn verify_value(&self, signed_value: &str) -> Option<String> {
        let (payload, sig_b64) = signed_value.split_once(':')?;
        let sig_bytes = URL_SAFE_NO_PAD.decode(sig_b64).ok()?;
        hmac::verify(&self.key, payload.as_bytes(), &sig_bytes).ok()?;
        Some(payload.to_string())
    }
}
