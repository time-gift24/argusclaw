//! Credential storage for external system credentials.

use std::sync::Arc;

use sqlx::SqlitePool;

use argus_crypto::Cipher;

use super::error::AuthError;

/// (id, name, username_ct, password_ct, nonce)
type CredentialRow = (i64, String, Vec<u8>, Vec<u8>, Vec<u8>);
/// (id, username_ct, password_ct, nonce)
type CredentialByNameRow = (i64, Vec<u8>, Vec<u8>, Vec<u8>);

#[derive(Debug, Clone)]
pub struct CredentialRecord {
    pub id: i64,
    pub name: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct CredentialSummary {
    pub id: i64,
    pub name: String,
}

pub struct CredentialStore {
    pool: Arc<SqlitePool>,
    cipher: Arc<Cipher>,
}

impl CredentialStore {
    pub fn new(pool: Arc<SqlitePool>, cipher: Arc<Cipher>) -> Self {
        Self { pool, cipher }
    }

    pub async fn list(&self) -> Result<Vec<CredentialSummary>, AuthError> {
        let rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM credentials ORDER BY name")
            .fetch_all(self.pool.as_ref())
            .await?;

        Ok(rows
            .into_iter()
            .map(|(id, name)| CredentialSummary { id, name })
            .collect())
    }

    pub async fn get(&self, id: i64) -> Result<Option<CredentialRecord>, AuthError> {
        let row: Option<CredentialRow> = sqlx::query_as(
            "SELECT id, name, username, password, nonce FROM credentials WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(self.pool.as_ref())
        .await?;

        match row {
            Some((id, name, username_ct, password_ct, nonce)) => {
                let username = self
                    .cipher
                    .decrypt(&nonce, &username_ct)
                    .map_err(|e| AuthError::DecryptionFailed {
                        reason: e.to_string(),
                    })?;
                let password = self
                    .cipher
                    .decrypt(&nonce, &password_ct)
                    .map_err(|e| AuthError::DecryptionFailed {
                        reason: e.to_string(),
                    })?;

                Ok(Some(CredentialRecord {
                    id,
                    name,
                    username: username.expose_secret().to_string(),
                    password: password.expose_secret().to_string(),
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn get_by_name(&self, name: &str) -> Result<Option<CredentialRecord>, AuthError> {
        let row: Option<CredentialByNameRow> = sqlx::query_as(
            "SELECT id, username, password, nonce FROM credentials WHERE name = ?1",
        )
        .bind(name)
        .fetch_optional(self.pool.as_ref())
        .await?;

        match row {
            Some((id, username_ct, password_ct, nonce)) => {
                let username = self
                    .cipher
                    .decrypt(&nonce, &username_ct)
                    .map_err(|e| AuthError::DecryptionFailed {
                        reason: e.to_string(),
                    })?;
                let password = self
                    .cipher
                    .decrypt(&nonce, &password_ct)
                    .map_err(|e| AuthError::DecryptionFailed {
                        reason: e.to_string(),
                    })?;

                Ok(Some(CredentialRecord {
                    id,
                    name: name.to_string(),
                    username: username.expose_secret().to_string(),
                    password: password.expose_secret().to_string(),
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn add(
        &self,
        name: &str,
        username: &str,
        password: &str,
    ) -> Result<i64, AuthError> {
        let encrypted_username = self.cipher.encrypt(username).map_err(|e| AuthError::EncryptionFailed {
            reason: e.to_string(),
        })?;
        let encrypted_password = self.cipher.encrypt(password).map_err(|e| AuthError::EncryptionFailed {
            reason: e.to_string(),
        })?;

        let result = sqlx::query(
            r#"
            INSERT INTO credentials (name, username, password, nonce, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, datetime('now'), datetime('now'))
            "#,
        )
        .bind(name)
        .bind(&encrypted_username.ciphertext)
        .bind(&encrypted_password.ciphertext)
        .bind(&encrypted_username.nonce)
        .execute(self.pool.as_ref())
        .await?;

        Ok(result.last_insert_rowid())
    }

    pub async fn update(
        &self,
        id: i64,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<(), AuthError> {
        if let Some(username) = username {
            let encrypted = self.cipher.encrypt(username).map_err(|e| AuthError::EncryptionFailed {
                reason: e.to_string(),
            })?;
            sqlx::query("UPDATE credentials SET username = ?1, updated_at = datetime('now') WHERE id = ?2")
                .bind(&encrypted.ciphertext)
                .bind(id)
                .execute(self.pool.as_ref())
                .await?;
        }

        if let Some(password) = password {
            let encrypted = self.cipher.encrypt(password).map_err(|e| AuthError::EncryptionFailed {
                reason: e.to_string(),
            })?;
            sqlx::query("UPDATE credentials SET password = ?1, updated_at = datetime('now') WHERE id = ?2")
                .bind(&encrypted.ciphertext)
                .bind(id)
                .execute(self.pool.as_ref())
                .await?;
        }

        Ok(())
    }

    pub async fn delete(&self, id: i64) -> Result<bool, AuthError> {
        let result = sqlx::query("DELETE FROM credentials WHERE id = ?1")
            .bind(id)
            .execute(self.pool.as_ref())
            .await?;

        Ok(result.rows_affected() > 0)
    }
}
