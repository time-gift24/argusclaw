//! Provider token credential repository trait for server-side credential storage.

use async_trait::async_trait;

use crate::error::DbError;
use crate::types::ProviderTokenCredential;
use argus_protocol::ProviderId;

/// Repository trait for server-managed provider token-exchange credentials.
///
/// Stores encrypted credentials that `TokenLLMProvider` uses to obtain
/// access tokens. These are server-managed secrets, not user login data.
#[async_trait]
pub trait ProviderTokenCredentialRepository: Send + Sync {
    /// Get stored credentials for a given provider.
    ///
    /// Returns `None` if the provider has no stored token-exchange credentials.
    async fn get_credentials_for_provider(
        &self,
        provider_id: &ProviderId,
    ) -> Result<Option<ProviderTokenCredential>, DbError>;

    /// Save or update credentials for a provider.
    async fn save_credentials(
        &self,
        credential: &ProviderTokenCredential,
    ) -> Result<(), DbError>;
}
