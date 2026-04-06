//! ProviderTokenCredentialRepository implementation for PostgreSQL.

use crate::error::DbError;
use crate::traits::ProviderTokenCredentialRepository;
use crate::types::ProviderTokenCredential;
use argus_protocol::ProviderId;
use async_trait::async_trait;

use super::user::get_column;
use super::{ArgusPostgres, DbResult};

#[async_trait]
impl ProviderTokenCredentialRepository for ArgusPostgres {
    async fn get_credentials_for_provider(
        &self,
        provider_id: &ProviderId,
    ) -> DbResult<Option<ProviderTokenCredential>> {
        let row = sqlx::query(
            "SELECT provider_id, username, ciphertext, nonce \
             FROM provider_token_credentials WHERE provider_id = $1",
        )
        .bind(provider_id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|r| map_credential(&r)).transpose()
    }

    async fn save_credentials(&self, credential: &ProviderTokenCredential) -> DbResult<()> {
        sqlx::query(
            r#"
            INSERT INTO provider_token_credentials (provider_id, username, ciphertext, nonce)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (provider_id) DO UPDATE SET
                username = EXCLUDED.username,
                ciphertext = EXCLUDED.ciphertext,
                nonce = EXCLUDED.nonce,
                updated_at = NOW()
            "#,
        )
        .bind(credential.provider_id.into_inner())
        .bind(&credential.username)
        .bind(&credential.ciphertext)
        .bind(&credential.nonce)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }
}

fn map_credential(row: &sqlx::postgres::PgRow) -> DbResult<ProviderTokenCredential> {
    Ok(ProviderTokenCredential {
        provider_id: ProviderId::new(get_column(&row, "provider_id")?),
        username: get_column(&row, "username")?,
        ciphertext: get_column(&row, "ciphertext")?,
        nonce: get_column(&row, "nonce")?,
    })
}
