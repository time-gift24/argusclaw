//! Account management for single-user local authentication.

use std::sync::Arc;

use sqlx::SqlitePool;
use subtle::ConstantTimeEq;

use argus_crypto::Cipher;
use argus_protocol::SecretString;

use super::error::AuthError;
use super::token::{SimpleTokenSource, TokenSource};

#[derive(Debug, Clone)]
pub struct UserInfo {
    pub username: String,
}

pub struct AccountManager {
    pool: Arc<SqlitePool>,
    cipher: Arc<Cipher>,
}

impl AccountManager {
    pub fn new(pool: Arc<SqlitePool>, cipher: Arc<Cipher>) -> Self {
        Self { pool, cipher }
    }

    pub async fn has_account(&self) -> Result<bool, AuthError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
            .fetch_one(self.pool.as_ref())
            .await?;
        Ok(count > 0)
    }

    pub async fn setup_account(&self, username: &str, password: &str) -> Result<(), AuthError> {
        // Check if account already exists
        if self.has_account().await? {
            return Err(AuthError::AccountAlreadyExists);
        }

        // Encrypt password
        let encrypted = self.cipher.encrypt(password).map_err(|e| AuthError::EncryptionFailed {
            reason: e.to_string(),
        })?;

        // Insert account (id is always 1 for single-user)
        sqlx::query(
            r#"
            INSERT INTO accounts (id, username, password, nonce, created_at, updated_at)
            VALUES (1, ?1, ?2, ?3, datetime('now'), datetime('now'))
            "#,
        )
        .bind(username)
        .bind(&encrypted.ciphertext)
        .bind(&encrypted.nonce)
        .execute(self.pool.as_ref())
        .await?;

        Ok(())
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<bool, AuthError> {
        let row: Option<(String, Vec<u8>, Vec<u8>)> = sqlx::query_as(
            "SELECT username, password, nonce FROM accounts WHERE id = 1",
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        match row {
            Some((stored_username, ciphertext, nonce)) => {
                // Verify username matches using constant-time comparison to prevent timing attacks
                let username_matches = stored_username.as_bytes().ct_eq(username.as_bytes());
                if !bool::from(username_matches) {
                    return Ok(false);
                }
                // Decrypt and verify password using constant-time comparison
                let decrypted = self
                    .cipher
                    .decrypt(&nonce, &ciphertext)
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
        let row: Option<(String,)> = sqlx::query_as("SELECT username FROM accounts WHERE id = 1")
            .fetch_optional(self.pool.as_ref())
            .await?;

        Ok(row.map(|(username,)| UserInfo { username }))
    }

    /// Login with token verification.
    ///
    /// 1. Verify stored password
    /// 2. Fetch token via TokenSource
    ///
    /// Returns true if both password and token fetch succeed.
    pub async fn login_verify(
        &self,
        username: &str,
        password: &str,
        token_source: &SimpleTokenSource,
    ) -> Result<bool, AuthError> {
        // Verify stored password
        let row: Option<(String, Vec<u8>, Vec<u8>)> = sqlx::query_as(
            "SELECT username, password, nonce FROM accounts WHERE id = 1",
        )
        .fetch_optional(self.pool.as_ref())
        .await?;

        let Some((stored_username, ciphertext, nonce)) = row else {
            return Ok(false);
        };

        if stored_username != username {
            return Ok(false);
        }

        let decrypted: SecretString = self
            .cipher
            .decrypt(&nonce, &ciphertext)
            .map_err(|e| AuthError::DecryptionFailed {
                reason: e.to_string(),
            })?;
        if decrypted.expose_secret() != password {
            return Ok(false);
        }

        // Fetch token (verifies credentials against auth server)
        let _token = token_source
            .fetch_token(username, password)
            .map_err(|e: AuthError| AuthError::TokenFetchFailed {
                reason: e.to_string(),
            })?;

        Ok(true)
    }
}
