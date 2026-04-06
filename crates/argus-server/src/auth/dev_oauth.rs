//! Development OAuth2 provider for testing.
//!
//! Generates short-lived authorization codes that can be exchanged for
//! predefined test user identities. No real OAuth2 endpoint is involved.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use argus_protocol::OAuth2Identity;

use super::provider::OAuth2AuthProvider;
use super::AuthError;

/// Short-lived authorization code issued by the dev authorize form.
struct PendingCode {
    identity: OAuth2Identity,
    created_at: std::time::Instant,
}

/// Development OAuth2 provider.
///
/// Stores codes in memory. Codes expire after a short window.
pub struct DevOAuth2Provider {
    codes: Mutex<HashMap<String, PendingCode>>,
    ttl: std::time::Duration,
}

impl DevOAuth2Provider {
    pub fn new() -> Self {
        Self {
            codes: Mutex::new(HashMap::new()),
            ttl: std::time::Duration::from_secs(60),
        }
    }

    /// Issue a new authorization code for the given identity.
    /// Returns the code string.
    pub fn issue_code(&self, identity: OAuth2Identity) -> String {
        let code = uuid::Uuid::now_v7().to_string();
        let pending = PendingCode {
            identity,
            created_at: std::time::Instant::now(),
        };
        if let Ok(mut codes) = self.codes.lock() {
            // Prune expired codes
            codes.retain(|_, v| v.created_at.elapsed() < self.ttl);
            codes.insert(code.clone(), pending);
        }
        code
    }
}


#[async_trait]
impl OAuth2AuthProvider for DevOAuth2Provider {
    async fn authorize_url(&self, state: &str, _redirect_uri: String) -> Result<String, AuthError> {
        Ok(format!("/dev-oauth/authorize?state={state}"))
    }

    async fn exchange_code(
        &self,
        code: &str,
        _redirect_uri: String,
    ) -> Result<OAuth2Identity, AuthError> {
        let mut codes = self.codes.lock().map_err(|e| AuthError::Provider {
            reason: e.to_string(),
        })?;
        let pending = codes.remove(code).ok_or(AuthError::InvalidCode)?;
        if pending.created_at.elapsed() > self.ttl {
            return Err(AuthError::InvalidCode);
        }
        Ok(pending.identity)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
