//! OAuth2 authentication provider abstraction.

use std::any::Any;

use async_trait::async_trait;
use argus_protocol::OAuth2Identity;

use super::AuthError;

/// Trait for server-side OAuth2 authentication providers.
///
/// The production provider will be implemented later.
/// `DevOAuth2Provider` implements this for development testing.
#[async_trait]
pub trait OAuth2AuthProvider: Send + Sync {
    /// Build the authorize URL to redirect the user to.
    async fn authorize_url(&self, state: &str, redirect_uri: String) -> Result<String, AuthError>;

    /// Exchange an authorization code for user identity.
    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: String,
    ) -> Result<OAuth2Identity, AuthError>;

    /// Support downcasting for provider-specific operations.
    fn as_any(&self) -> &dyn Any;
}
