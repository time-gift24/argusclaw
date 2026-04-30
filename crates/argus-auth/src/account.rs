//! Account management for single-user local authentication.

use std::sync::Arc;

use argus_crypto::Cipher;
use subtle::ConstantTimeEq;

use super::error::AuthError;
use argus_repository::traits::AccountRepository;

#[derive(Debug, Clone)]
pub struct UserInfo {
    pub username: String,
}

pub struct AccountManager {
    repo: Arc<dyn AccountRepository>,
    cipher: Arc<Cipher>,
}

impl AccountManager {
    pub fn new(repo: Arc<dyn AccountRepository>, cipher: Arc<Cipher>) -> Self {
        Self { repo, cipher }
    }

    pub async fn has_account(&self) -> Result<bool, AuthError> {
        self.repo
            .has_account()
            .await
            .map_err(|e| AuthError::DatabaseError {
                reason: e.to_string(),
            })
    }

    pub async fn setup_account(&self, username: &str, password: &str) -> Result<(), AuthError> {
        // Check if account already exists
        if self.has_account().await? {
            return Err(AuthError::AccountAlreadyExists);
        }

        // Encrypt password
        let encrypted = self
            .cipher
            .encrypt(password)
            .map_err(|e| AuthError::EncryptionFailed {
                reason: e.to_string(),
            })?;

        self.repo
            .setup_account(username, &encrypted.ciphertext, &encrypted.nonce)
            .await
            .map_err(|e| AuthError::DatabaseError {
                reason: e.to_string(),
            })
    }

    pub async fn configure_account(&self, username: &str, password: &str) -> Result<(), AuthError> {
        let encrypted = self
            .cipher
            .encrypt(password)
            .map_err(|e| AuthError::EncryptionFailed {
                reason: e.to_string(),
            })?;

        self.repo
            .configure_account(username, &encrypted.ciphertext, &encrypted.nonce)
            .await
            .map_err(|e| AuthError::DatabaseError {
                reason: e.to_string(),
            })
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<bool, AuthError> {
        let creds = self
            .repo
            .get_credentials()
            .await
            .map_err(|e| AuthError::DatabaseError {
                reason: e.to_string(),
            })?;

        match creds {
            Some(stored) => {
                // Verify username matches using constant-time comparison to prevent timing attacks
                let username_matches = stored.username.as_bytes().ct_eq(username.as_bytes());
                if !bool::from(username_matches) {
                    return Ok(false);
                }
                // Decrypt and verify password using constant-time comparison
                let decrypted = self
                    .cipher
                    .decrypt(&stored.nonce, &stored.ciphertext)
                    .map_err(|e| AuthError::DecryptionFailed {
                        reason: e.to_string(),
                    })?;
                let password_bytes = password.as_bytes();
                let decrypted_bytes = decrypted.expose_secret().as_bytes();
                let password_matches = decrypted_bytes.ct_eq(password_bytes);
                Ok(bool::from(password_matches))
            }
            None => Ok(false),
        }
    }

    pub async fn logout(&self) -> Result<(), AuthError> {
        // No-op for single-user local app
        Ok(())
    }

    pub async fn get_current_user(&self) -> Result<Option<UserInfo>, AuthError> {
        let username = self
            .repo
            .get_username()
            .await
            .map_err(|e| AuthError::DatabaseError {
                reason: e.to_string(),
            })?;
        Ok(username.map(|username| UserInfo { username }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use argus_crypto::StaticKeySource;
    use argus_repository::{ArgusSqlite, migrate};

    use super::*;

    #[tokio::test]
    async fn configure_account_creates_and_replaces_login_credentials() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite should connect");
        migrate(&pool).await.expect("migrations should run");
        let manager = AccountManager::new(
            Arc::new(ArgusSqlite::new(pool)),
            Arc::new(Cipher::new(StaticKeySource::new(
                b"fixed-test-key".to_vec(),
            ))),
        );

        manager
            .configure_account("alice", "first-secret")
            .await
            .expect("account should configure");
        assert!(
            manager
                .login("alice", "first-secret")
                .await
                .expect("login should verify")
        );

        manager
            .configure_account("bob", "second-secret")
            .await
            .expect("account should update");

        assert!(
            !manager
                .login("alice", "first-secret")
                .await
                .expect("old credentials should verify as invalid")
        );
        assert!(
            manager
                .login("bob", "second-secret")
                .await
                .expect("new credentials should verify")
        );
    }
}
