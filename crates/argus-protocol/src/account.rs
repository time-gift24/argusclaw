//! Account repository trait and credentials for authentication.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Account credentials for authentication.
/// Contains the stored ciphertext and nonce needed for password verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountCredentials {
    pub username: String,
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
}

/// Repository trait for account persistence.
///
/// This trait is implemented by storage layers (e.g., SQLite in repository)
/// and consumed by `AccountManager` in argus-auth.
#[async_trait]
pub trait AccountRepository: Send + Sync {
    /// Check whether any account exists.
    async fn has_account(&self) -> crate::Result<bool>;

    /// Create a new account (id is always 1 for single-user).
    async fn setup_account(
        &self,
        username: &str,
        ciphertext: &[u8],
        nonce: &[u8],
    ) -> crate::Result<()>;

    /// Create or replace the single configured account.
    async fn configure_account(
        &self,
        username: &str,
        ciphertext: &[u8],
        nonce: &[u8],
    ) -> crate::Result<()>;

    /// Get stored credentials for login verification.
    async fn get_credentials(&self) -> crate::Result<Option<AccountCredentials>>;

    /// Get the current username.
    async fn get_username(&self) -> crate::Result<Option<String>>;
}
