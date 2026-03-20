//! Credential storage for external system credentials.

use std::sync::Arc;

use sqlx::SqlitePool;

use argus_crypto::Cipher;

use super::error::AuthError;

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
        let row: Option<(i64, String, Vec<u8>, Vec<u8>)> = sqlx::query_as(
            "SELECT id, name, username, nonce FROM credentials WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(self.pool.as_ref())
        .await?;

        match row {
            Some((id, name, ciphertext, nonce)) => {
                let decrypted = self
                    .cipher
                    .decrypt(&nonce, &ciphertext)
                    .map_err(|e| AuthError::DecryptionFailed {
                        reason: e.to_string(),
                    })?;
                let payload = decrypted.expose_secret();
                let parts: Vec<&str> = payload.split("\x00").collect();
                if parts.len() != 2 {
                    return Err(AuthError::DecryptionFailed {
                        reason: "invalid credential payload format".to_string(),
                    });
                }

                Ok(Some(CredentialRecord {
                    id,
                    name,
                    username: parts[0].to_string(),
                    password: parts[1].to_string(),
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn get_by_name(&self, name: &str) -> Result<Option<CredentialRecord>, AuthError> {
        let row: Option<(i64, Vec<u8>, Vec<u8>)> = sqlx::query_as(
            "SELECT id, username, nonce FROM credentials WHERE name = ?1",
        )
        .bind(name)
        .fetch_optional(self.pool.as_ref())
        .await?;

        match row {
            Some((id, ciphertext, nonce)) => {
                let decrypted = self
                    .cipher
                    .decrypt(&nonce, &ciphertext)
                    .map_err(|e| AuthError::DecryptionFailed {
                        reason: e.to_string(),
                    })?;
                let payload = decrypted.expose_secret();
                let parts: Vec<&str> = payload.split("\x00").collect();
                if parts.len() != 2 {
                    return Err(AuthError::DecryptionFailed {
                        reason: "invalid credential payload format".to_string(),
                    });
                }

                Ok(Some(CredentialRecord {
                    id,
                    name: name.to_string(),
                    username: parts[0].to_string(),
                    password: parts[1].to_string(),
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
        // Combine username and password into a single payload to encrypt together
        // This allows storing only one nonce for both fields
        let payload = format!("{}\x00{}\x00", username, password);
        let encrypted = self.cipher.encrypt(&payload).map_err(|e| AuthError::EncryptionFailed {
            reason: e.to_string(),
        })?;

        let result = sqlx::query(
            r#"
            INSERT INTO credentials (name, username, password, nonce, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, datetime('now'), datetime('now'))
            "#,
        )
        .bind(name)
        .bind(&encrypted.ciphertext)
        .bind(&encrypted.nonce) // Same nonce used for both username and password
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
        // If either field is being updated, re-encrypt both together
        // This ensures they share the same nonce
        if username.is_some() || password.is_some() {
            let current = self.get(id).await?;
            if let Some(current) = current {
                let new_username = username.unwrap_or(&current.username);
                let new_password = password.unwrap_or(&current.password);

                let payload = format!("{}\x00{}\x00", new_username, new_password);
                let encrypted = self.cipher.encrypt(&payload).map_err(|e| AuthError::EncryptionFailed {
                    reason: e.to_string(),
                })?;

                sqlx::query("UPDATE credentials SET username = ?1, nonce = ?2, updated_at = datetime('now') WHERE id = ?3")
                    .bind(&encrypted.ciphertext)
                    .bind(&encrypted.nonce)
                    .bind(id)
                    .execute(self.pool.as_ref())
                    .await?;
            }
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
