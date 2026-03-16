use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::db::DbError;
use crate::db::llm::{
    LlmProviderId, LlmProviderRecord, LlmProviderRepository, LlmProviderSummary,
    ProviderSecretStatus, SecretString,
};
use crate::llm::secret::{
    ApiKeyCipher, FileKeyMaterialSource, HostMacAddressKeyMaterialSource, KeyMaterialSource,
    StaticKeyMaterialSource,
};

pub struct SqliteLlmProviderRepository {
    pool: SqlitePool,
    write_cipher: ApiKeyCipher,
    read_ciphers: Vec<ApiKeyCipher>,
}

type SharedProviderFields = (
    LlmProviderId,
    crate::db::llm::LlmProviderKind,
    String,
    String,
    bool,
    HashMap<String, String>,
    Vec<u8>,
    Vec<u8>,
);

impl SqliteLlmProviderRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self::with_key_sources(
            pool,
            Arc::new(FileKeyMaterialSource::from_env_or_default()),
            vec![Arc::new(HostMacAddressKeyMaterialSource)],
        )
    }

    #[must_use]
    pub fn new_with_key_material(pool: SqlitePool, key_material: Vec<u8>) -> Self {
        Self::with_key_sources(
            pool,
            Arc::new(StaticKeyMaterialSource::new(key_material)),
            Vec::new(),
        )
    }

    #[must_use]
    pub fn new_with_key_material_and_fallbacks(
        pool: SqlitePool,
        key_material: Vec<u8>,
        fallback_key_materials: Vec<Vec<u8>>,
    ) -> Self {
        let fallback_sources = fallback_key_materials
            .into_iter()
            .map(|key_material| {
                Arc::new(StaticKeyMaterialSource::new(key_material)) as Arc<dyn KeyMaterialSource>
            })
            .collect();

        Self::with_key_sources(
            pool,
            Arc::new(StaticKeyMaterialSource::new(key_material)),
            fallback_sources,
        )
    }

    #[must_use]
    pub fn with_key_source(pool: SqlitePool, key_source: Arc<dyn KeyMaterialSource>) -> Self {
        Self::with_key_sources(pool, key_source, Vec::new())
    }

    #[must_use]
    pub fn with_key_sources(
        pool: SqlitePool,
        key_source: Arc<dyn KeyMaterialSource>,
        fallback_sources: Vec<Arc<dyn KeyMaterialSource>>,
    ) -> Self {
        let mut read_ciphers = vec![ApiKeyCipher::new_arc(Arc::clone(&key_source))];
        read_ciphers.extend(fallback_sources.into_iter().map(ApiKeyCipher::new_arc));

        Self {
            pool,
            write_cipher: ApiKeyCipher::new_arc(key_source),
            read_ciphers,
        }
    }

    fn parse_shared_fields(row: sqlx::sqlite::SqliteRow) -> Result<SharedProviderFields, DbError> {
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

        let extra_headers_json: String =
            row.try_get("extra_headers")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;
        let extra_headers: HashMap<String, String> = serde_json::from_str(&extra_headers_json)
            .map_err(|e| DbError::QueryFailed {
                reason: format!("failed to parse extra_headers: {e}"),
            })?;

        Ok((
            LlmProviderId::new(row.try_get::<String, _>("id").map_err(|e| {
                DbError::QueryFailed {
                    reason: e.to_string(),
                }
            })?),
            row.try_get::<String, _>("kind")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?
                .parse()?,
            row.try_get("display_name")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?,
            row.try_get("base_url").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            row.try_get::<i64, _>("is_default")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?
                != 0,
            extra_headers,
            nonce,
            ciphertext,
        ))
    }

    fn decrypt_secret(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<SecretString, DbError> {
        let mut last_error = None;

        for cipher in &self.read_ciphers {
            match cipher.decrypt(nonce, ciphertext) {
                Ok(secret) => return Ok(secret),
                Err(error) => last_error = Some(error),
            }
        }

        Err(
            last_error.unwrap_or_else(|| DbError::SecretDecryptionFailed {
                reason: "no key sources are configured".to_string(),
            }),
        )
    }

    fn map_record(&self, row: sqlx::sqlite::SqliteRow) -> Result<LlmProviderRecord, DbError> {
        let (id, kind, display_name, base_url, is_default, extra_headers, nonce, ciphertext) =
            Self::parse_shared_fields(row)?;

        Ok(LlmProviderRecord {
            id,
            kind,
            display_name,
            base_url,
            api_key: self.decrypt_secret(&nonce, &ciphertext)?,
            is_default,
            extra_headers,
            secret_status: ProviderSecretStatus::Ready,
        })
    }

    fn map_summary(&self, row: sqlx::sqlite::SqliteRow) -> Result<LlmProviderSummary, DbError> {
        let (id, kind, display_name, base_url, is_default, extra_headers, nonce, ciphertext) =
            Self::parse_shared_fields(row)?;
        let secret_status = if self.decrypt_secret(&nonce, &ciphertext).is_ok() {
            ProviderSecretStatus::Ready
        } else {
            ProviderSecretStatus::RequiresReentry
        };

        Ok(LlmProviderSummary {
            id,
            kind,
            display_name,
            base_url,
            is_default,
            extra_headers,
            secret_status,
        })
    }
}

#[async_trait]
impl LlmProviderRepository for SqliteLlmProviderRepository {
    async fn upsert_provider(&self, record: &LlmProviderRecord) -> Result<(), DbError> {
        let encrypted_secret = self.write_cipher.encrypt(record.api_key.expose_secret())?;
        let extra_headers_json =
            serde_json::to_string(&record.extra_headers).map_err(|e| DbError::QueryFailed {
                reason: format!("failed to serialize extra_headers: {e}"),
            })?;
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
            "insert into llm_providers (id, kind, display_name, base_url, encrypted_api_key, api_key_nonce, is_default, extra_headers)
             values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             on conflict(id) do update set
                 kind = excluded.kind,
                 display_name = excluded.display_name,
                 base_url = excluded.base_url,
                 encrypted_api_key = excluded.encrypted_api_key,
                 api_key_nonce = excluded.api_key_nonce,
                 is_default = excluded.is_default,
                 extra_headers = excluded.extra_headers,
                 updated_at = CURRENT_TIMESTAMP",
        )
        .bind(record.id.as_ref())
        .bind(record.kind.as_str())
        .bind(&record.display_name)
        .bind(&record.base_url)
        .bind(encrypted_secret.ciphertext)
        .bind(encrypted_secret.nonce)
        .bind(i64::from(record.is_default))
        .bind(&extra_headers_json)
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

    async fn set_default_provider(&self, id: &LlmProviderId) -> Result<(), DbError> {
        let mut transaction = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        let exists =
            sqlx::query_scalar::<_, i64>("select count(1) from llm_providers where id = ?1")
                .bind(id.as_ref())
                .fetch_one(&mut *transaction)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;

        if exists == 0 {
            return Err(DbError::QueryFailed {
                reason: format!("provider `{id}` does not exist"),
            });
        }

        sqlx::query("update llm_providers set is_default = 0, updated_at = CURRENT_TIMESTAMP where is_default = 1")
            .execute(&mut *transaction)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        sqlx::query(
            "update llm_providers set is_default = 1, updated_at = CURRENT_TIMESTAMP where id = ?1",
        )
        .bind(id.as_ref())
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

    async fn delete_provider(&self, id: &LlmProviderId) -> Result<bool, DbError> {
        let result = sqlx::query("delete from llm_providers where id = ?1")
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }

    async fn get_provider(&self, id: &LlmProviderId) -> Result<Option<LlmProviderRecord>, DbError> {
        let row = sqlx::query(
            "select id, kind, display_name, base_url, encrypted_api_key, api_key_nonce, is_default, extra_headers
             from llm_providers
             where id = ?1",
        )
        .bind(id.as_ref())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| self.map_record(row)).transpose()
    }

    async fn get_provider_summary(
        &self,
        id: &LlmProviderId,
    ) -> Result<Option<LlmProviderSummary>, DbError> {
        let row = sqlx::query(
            "select id, kind, display_name, base_url, model, encrypted_api_key, api_key_nonce, is_default, extra_headers
             from llm_providers
             where id = ?1",
        )
        .bind(id.as_ref())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| self.map_summary(row)).transpose()
    }

    async fn list_providers(&self) -> Result<Vec<LlmProviderSummary>, DbError> {
        let rows = sqlx::query(
            "select id, kind, display_name, base_url, encrypted_api_key, api_key_nonce, is_default, extra_headers
             from llm_providers
             order by display_name asc",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter().map(|row| self.map_summary(row)).collect()
    }

    async fn get_default_provider(&self) -> Result<Option<LlmProviderRecord>, DbError> {
        let row = sqlx::query(
            "select id, kind, display_name, base_url, encrypted_api_key, api_key_nonce, is_default, extra_headers
             from llm_providers
             where is_default = 1
             limit 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| self.map_record(row)).transpose()
    }
}
