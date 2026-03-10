use std::sync::Arc;

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::db::DbError;
use crate::db::llm::{LlmProviderId, LlmProviderRecord, LlmProviderRepository};
use crate::llm::secret::{
    ApiKeyCipher, HostMacAddressKeyMaterialSource, KeyMaterialSource, StaticKeyMaterialSource,
};

pub struct SqliteLlmProviderRepository {
    pool: SqlitePool,
    cipher: ApiKeyCipher,
}

impl SqliteLlmProviderRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self::with_key_source(pool, Arc::new(HostMacAddressKeyMaterialSource))
    }

    #[must_use]
    pub fn new_with_key_material(pool: SqlitePool, key_material: Vec<u8>) -> Self {
        Self::with_key_source(pool, Arc::new(StaticKeyMaterialSource::new(key_material)))
    }

    #[must_use]
    pub fn with_key_source(pool: SqlitePool, key_source: Arc<dyn KeyMaterialSource>) -> Self {
        Self {
            pool,
            cipher: ApiKeyCipher::new_arc(key_source),
        }
    }

    fn map_record(
        row: sqlx::sqlite::SqliteRow,
        cipher: &ApiKeyCipher,
    ) -> Result<LlmProviderRecord, DbError> {
        let nonce: Vec<u8> = row
            .try_get("api_key_nonce")
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        let ciphertext: Vec<u8> =
            row.try_get("encrypted_api_key")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;

        Ok(LlmProviderRecord {
            id: LlmProviderId::new(row.try_get::<String, _>("id").map_err(|e| {
                DbError::QueryFailed {
                    reason: e.to_string(),
                }
            })?),
            kind: row
                .try_get::<String, _>("kind")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?
                .parse()?,
            display_name: row
                .try_get("display_name")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?,
            base_url: row.try_get("base_url").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            api_key: cipher.decrypt(&nonce, &ciphertext)?,
            model: row.try_get("model").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            is_default: row
                .try_get::<i64, _>("is_default")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?
                != 0,
        })
    }
}

#[async_trait]
impl LlmProviderRepository for SqliteLlmProviderRepository {
    async fn upsert_provider(&self, record: &LlmProviderRecord) -> Result<(), DbError> {
        let encrypted_secret = self.cipher.encrypt(record.api_key.expose_secret())?;
        let mut transaction = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        if record.is_default {
            sqlx::query("update llm_providers set is_default = 0, updated_at = CURRENT_TIMESTAMP where id != ?1 and is_default = 1")
                .bind(record.id.as_ref())
                .execute(&mut *transaction)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;
        }

        sqlx::query(
            "insert into llm_providers (id, kind, display_name, base_url, model, encrypted_api_key, api_key_nonce, is_default)
             values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             on conflict(id) do update set
                 kind = excluded.kind,
                 display_name = excluded.display_name,
                 base_url = excluded.base_url,
                 model = excluded.model,
                 encrypted_api_key = excluded.encrypted_api_key,
                 api_key_nonce = excluded.api_key_nonce,
                 is_default = excluded.is_default,
                 updated_at = CURRENT_TIMESTAMP",
        )
        .bind(record.id.as_ref())
        .bind(record.kind.as_str())
        .bind(&record.display_name)
        .bind(&record.base_url)
        .bind(&record.model)
        .bind(encrypted_secret.ciphertext)
        .bind(encrypted_secret.nonce)
        .bind(i64::from(record.is_default))
        .execute(&mut *transaction)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        transaction
            .commit()
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(())
    }

    async fn get_provider(&self, id: &LlmProviderId) -> Result<Option<LlmProviderRecord>, DbError> {
        let row = sqlx::query(
            "select id, kind, display_name, base_url, model, encrypted_api_key, api_key_nonce, is_default
             from llm_providers
             where id = ?1",
        )
        .bind(id.as_ref())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| Self::map_record(row, &self.cipher))
            .transpose()
    }

    async fn list_providers(&self) -> Result<Vec<LlmProviderRecord>, DbError> {
        let rows = sqlx::query(
            "select id, kind, display_name, base_url, model, encrypted_api_key, api_key_nonce, is_default
             from llm_providers
             order by display_name asc",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter()
            .map(|row| Self::map_record(row, &self.cipher))
            .collect()
    }

    async fn get_default_provider(&self) -> Result<Option<LlmProviderRecord>, DbError> {
        let row = sqlx::query(
            "select id, kind, display_name, base_url, model, encrypted_api_key, api_key_nonce, is_default
             from llm_providers
             where is_default = 1
             limit 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| Self::map_record(row, &self.cipher))
            .transpose()
    }
}
