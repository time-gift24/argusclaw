use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::db::DbError;
// Import types from argus-protocol
use argus_protocol::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderKindParseError, LlmProviderRecord,
    LlmProviderRepository, ProviderSecretStatus, SecretString,
};
use argus_protocol::{ArgusError, Result};
// Import secret types from argus-llm
use argus_llm::secret::{
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
    LlmProviderKind,
    String,                  // display_name
    String,                  // base_url
    Vec<String>,             // models
    String,                  // default_model
    bool,                    // is_default
    HashMap<String, String>, // extra_headers
    Vec<u8>,                 // nonce
    Vec<u8>,                 // ciphertext
);

/// Convert DbError to ArgusError
fn db_to_argus(e: DbError) -> ArgusError {
    ArgusError::DatabaseError {
        reason: e.to_string(),
    }
}

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

    fn parse_shared_fields(
        row: sqlx::sqlite::SqliteRow,
    ) -> std::result::Result<SharedProviderFields, DbError> {
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

        let models_json: String = row.try_get("models").map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        let models: Vec<String> =
            serde_json::from_str(&models_json).map_err(|e| DbError::QueryFailed {
                reason: format!("failed to parse models: {e}"),
            })?;

        let kind_str: String = row.try_get("kind").map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        let kind = kind_str.parse().map_err(|e: LlmProviderKindParseError| {
            DbError::InvalidProviderKind {
                kind: e.kind.clone(),
            }
        })?;

        Ok((
            LlmProviderId::new(
                row.try_get::<i64, _>("id")
                    .map_err(|e| DbError::QueryFailed {
                        reason: e.to_string(),
                    })?,
            ),
            kind,
            row.try_get("display_name")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?,
            row.try_get("base_url").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            models,
            row.try_get("default_model")
                .map_err(|e| DbError::QueryFailed {
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

    fn decrypt_secret(
        &self,
        nonce: &[u8],
        ciphertext: &[u8],
    ) -> std::result::Result<SecretString, DbError> {
        let mut last_error = None;

        for cipher in &self.read_ciphers {
            match cipher.decrypt(nonce, ciphertext) {
                Ok(secret) => return Ok(secret),
                Err(error) => last_error = Some(error),
            }
        }

        Err(DbError::SecretDecryptionFailed {
            reason: last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "no key sources are configured".to_string()),
        })
    }

    fn map_record(
        &self,
        row: sqlx::sqlite::SqliteRow,
    ) -> std::result::Result<LlmProviderRecord, DbError> {
        let (
            id,
            kind,
            display_name,
            base_url,
            models,
            default_model,
            is_default,
            extra_headers,
            nonce,
            ciphertext,
        ) = Self::parse_shared_fields(row)?;

        // Attempt decryption; if it fails, return record with empty api_key
        // and RequiresReentry status so the user knows to re-enter the key
        let (api_key, secret_status) = match self.decrypt_secret(&nonce, &ciphertext) {
            Ok(key) => (key, ProviderSecretStatus::Ready),
            Err(_) => (SecretString::new(String::new()), ProviderSecretStatus::RequiresReentry),
        };

        Ok(LlmProviderRecord {
            id,
            kind,
            display_name,
            base_url,
            api_key,
            models,
            default_model,
            is_default,
            extra_headers,
            secret_status,
        })
    }
}

#[async_trait]
impl LlmProviderRepository for SqliteLlmProviderRepository {
    async fn upsert_provider(&self, record: &LlmProviderRecord) -> Result<LlmProviderId> {
        // Validate models
        if record.models.is_empty() {
            return Err(ArgusError::DatabaseError {
                reason: "At least one model is required".to_string(),
            });
        }
        if !record.models.contains(&record.default_model) {
            return Err(ArgusError::DatabaseError {
                reason: format!(
                    "Default model '{}' must be in models list",
                    record.default_model
                ),
            });
        }

        let encrypted_secret = self
            .write_cipher
            .encrypt(record.api_key.expose_secret())
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;
        let extra_headers_json = serde_json::to_string(&record.extra_headers).map_err(|e| {
            ArgusError::DatabaseError {
                reason: format!("failed to serialize extra_headers: {e}"),
            }
        })?;
        let models_json =
            serde_json::to_string(&record.models).map_err(|e| ArgusError::DatabaseError {
                reason: format!("failed to serialize models: {e}"),
            })?;
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        if record.is_default {
            sqlx::query(
                "update llm_providers set is_default = 0, updated_at = CURRENT_TIMESTAMP where id != ?1 and is_default = 1"
            )
            .bind(record.id.into_inner())
            .execute(&mut *transaction)
            .await
            .map_err(|e| db_to_argus(DbError::QueryFailed {
                reason: e.to_string(),
            }))?;
        }

        let provider_id = if record.id.into_inner() == 0 {
            // INSERT without id - let SQLite auto-generate
            sqlx::query(
                "insert into llm_providers (kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers)
                 values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .bind(record.kind.as_str())
            .bind(&record.display_name)
            .bind(&record.base_url)
            .bind(&models_json)
            .bind(&record.default_model)
            .bind(encrypted_secret.ciphertext)
            .bind(encrypted_secret.nonce)
            .bind(i64::from(record.is_default))
            .bind(&extra_headers_json)
            .execute(&mut *transaction)
            .await
            .map_err(|e| db_to_argus(DbError::QueryFailed {
                reason: e.to_string(),
            }))?;

            // Get the newly generated ID
            let new_id: i64 = sqlx::query_scalar("SELECT last_insert_rowid()")
                .fetch_one(&mut *transaction)
                .await
                .map_err(|e| ArgusError::DatabaseError {
                    reason: format!("failed to get last_insert_rowid: {e}"),
                })?;

            LlmProviderId::new(new_id)
        } else {
            sqlx::query(
                "insert into llm_providers (id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers)
                 values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 on conflict(id) do update set
                     kind = excluded.kind,
                     display_name = excluded.display_name,
                     base_url = excluded.base_url,
                     models = excluded.models,
                     default_model = excluded.default_model,
                     encrypted_api_key = excluded.encrypted_api_key,
                     api_key_nonce = excluded.api_key_nonce,
                     is_default = excluded.is_default,
                     extra_headers = excluded.extra_headers,
                     updated_at = CURRENT_TIMESTAMP",
            )
            .bind(record.id.into_inner())
            .bind(record.kind.as_str())
            .bind(&record.display_name)
            .bind(&record.base_url)
            .bind(&models_json)
            .bind(&record.default_model)
            .bind(encrypted_secret.ciphertext)
            .bind(encrypted_secret.nonce)
            .bind(i64::from(record.is_default))
            .bind(&extra_headers_json)
            .execute(&mut *transaction)
            .await
            .map_err(|e| db_to_argus(DbError::QueryFailed {
                reason: e.to_string(),
            }))?;

            record.id
        };

        transaction.commit().await.map_err(|e| {
            db_to_argus(DbError::QueryFailed {
                reason: e.to_string(),
            })
        })?;

        Ok(provider_id)
    }

    async fn set_default_provider(&self, id: &LlmProviderId) -> Result<()> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        let exists =
            sqlx::query_scalar::<_, i64>("select count(1) from llm_providers where id = ?1")
                .bind(id.into_inner())
                .fetch_one(&mut *transaction)
                .await
                .map_err(|e| {
                    db_to_argus(DbError::QueryFailed {
                        reason: e.to_string(),
                    })
                })?;

        if exists == 0 {
            return Err(ArgusError::ProviderNotFound(id.into_inner()));
        }

        sqlx::query(
            "update llm_providers set is_default = 0, updated_at = CURRENT_TIMESTAMP where is_default = 1"
        )
        .execute(&mut *transaction)
        .await
        .map_err(|e| db_to_argus(DbError::QueryFailed {
            reason: e.to_string(),
        }))?;

        sqlx::query(
            "update llm_providers set is_default = 1, updated_at = CURRENT_TIMESTAMP where id = ?1",
        )
        .bind(id.into_inner())
        .execute(&mut *transaction)
        .await
        .map_err(|e| {
            db_to_argus(DbError::QueryFailed {
                reason: e.to_string(),
            })
        })?;

        transaction.commit().await.map_err(|e| {
            db_to_argus(DbError::QueryFailed {
                reason: e.to_string(),
            })
        })?;

        Ok(())
    }

    async fn delete_provider(&self, id: &LlmProviderId) -> Result<bool> {
        let result = sqlx::query("delete from llm_providers where id = ?1")
            .bind(id.into_inner())
            .execute(&self.pool)
            .await
            .map_err(|e| {
                db_to_argus(DbError::QueryFailed {
                    reason: e.to_string(),
                })
            })?;

        Ok(result.rows_affected() > 0)
    }

    async fn get_provider(&self, id: &LlmProviderId) -> Result<Option<LlmProviderRecord>> {
        let row = sqlx::query(
            "select id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers
             from llm_providers
             where id = ?1",
        )
        .bind(id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db_to_argus(DbError::QueryFailed {
            reason: e.to_string(),
        }))?;

        row.map(|row| self.map_record(row).map_err(db_to_argus))
            .transpose()
    }

    async fn list_providers(&self) -> Result<Vec<LlmProviderRecord>> {
        let rows = sqlx::query(
            "select id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers
             from llm_providers
             order by display_name asc",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| db_to_argus(DbError::QueryFailed {
            reason: e.to_string(),
        }))?;

        rows.into_iter()
            .map(|row| self.map_record(row).map_err(db_to_argus))
            .collect()
    }

    async fn get_default_provider(&self) -> Result<Option<LlmProviderRecord>> {
        let row = sqlx::query(
            "select id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers
             from llm_providers
             where is_default = 1
             limit 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db_to_argus(DbError::QueryFailed {
            reason: e.to_string(),
        }))?;

        row.map(|row| self.map_record(row).map_err(db_to_argus))
            .transpose()
    }
}

// Also implement claw's repository trait for backward compatibility
#[async_trait]
impl crate::db::llm::LlmProviderRepository for SqliteLlmProviderRepository {
    async fn upsert_provider(
        &self,
        record: &crate::db::llm::LlmProviderRecord,
    ) -> std::result::Result<LlmProviderId, DbError> {
        // Convert claw record to argus-protocol record
        let argus_record = LlmProviderRecord {
            id: record.id,
            kind: record.kind,
            display_name: record.display_name.clone(),
            base_url: record.base_url.clone(),
            api_key: record.api_key.clone(),
            models: record.models.clone(),
            default_model: record.default_model.clone(),
            is_default: record.is_default,
            extra_headers: record.extra_headers.clone(),
            secret_status: record.secret_status,
        };

        <Self as LlmProviderRepository>::upsert_provider(self, &argus_record)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })
    }

    async fn delete_provider(&self, id: &LlmProviderId) -> std::result::Result<bool, DbError> {
        <Self as LlmProviderRepository>::delete_provider(self, id)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })
    }

    async fn set_default_provider(&self, id: &LlmProviderId) -> std::result::Result<(), DbError> {
        <Self as LlmProviderRepository>::set_default_provider(self, id)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })
    }

    async fn get_provider(
        &self,
        id: &LlmProviderId,
    ) -> std::result::Result<Option<crate::db::llm::LlmProviderRecord>, DbError> {
        let row = sqlx::query(
            "select id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers
             from llm_providers
             where id = ?1",
        )
        .bind(id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| {
            let (
                id,
                kind,
                display_name,
                base_url,
                models,
                default_model,
                is_default,
                extra_headers,
                nonce,
                ciphertext,
            ) = Self::parse_shared_fields(row)?;

            // Attempt decryption; if it fails, return record with empty api_key
            // and RequiresReentry status so the user knows to re-enter the key
            let (api_key, secret_status) = match self.decrypt_secret(&nonce, &ciphertext) {
                Ok(key) => (key, ProviderSecretStatus::Ready),
                Err(_) => (SecretString::new(String::new()), ProviderSecretStatus::RequiresReentry),
            };

            Ok(crate::db::llm::LlmProviderRecord {
                id,
                kind,
                display_name,
                base_url,
                api_key,
                models,
                default_model,
                is_default,
                extra_headers,
                secret_status,
            })
        })
        .transpose()
    }

    async fn list_providers(
        &self,
    ) -> std::result::Result<Vec<crate::db::llm::LlmProviderRecord>, DbError> {
        let rows = sqlx::query(
            "select id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers
             from llm_providers
             order by display_name asc",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter()
            .map(|row| {
                let (
                    id,
                    kind,
                    display_name,
                    base_url,
                    models,
                    default_model,
                    is_default,
                    extra_headers,
                    nonce,
                    ciphertext,
                ) = Self::parse_shared_fields(row)?;

                // Attempt decryption; if it fails, return record with empty api_key
                // and RequiresReentry status so the user knows to re-enter the key
                let (api_key, secret_status) = match self.decrypt_secret(&nonce, &ciphertext) {
                    Ok(key) => (key, ProviderSecretStatus::Ready),
                    Err(_) => (SecretString::new(String::new()), ProviderSecretStatus::RequiresReentry),
                };

                Ok(crate::db::llm::LlmProviderRecord {
                    id,
                    kind,
                    display_name,
                    base_url,
                    api_key,
                    models,
                    default_model,
                    is_default,
                    extra_headers,
                    secret_status,
                })
            })
            .collect()
    }

    async fn get_default_provider(
        &self,
    ) -> std::result::Result<Option<crate::db::llm::LlmProviderRecord>, DbError> {
        let row = sqlx::query(
            "select id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers
             from llm_providers
             where is_default = 1
             limit 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| {
            let (
                id,
                kind,
                display_name,
                base_url,
                models,
                default_model,
                is_default,
                extra_headers,
                nonce,
                ciphertext,
            ) = Self::parse_shared_fields(row)?;

            // Attempt decryption; if it fails, return record with empty api_key
            // and RequiresReentry status so the user knows to re-enter the key
            let (api_key, secret_status) = match self.decrypt_secret(&nonce, &ciphertext) {
                Ok(key) => (key, ProviderSecretStatus::Ready),
                Err(_) => (SecretString::new(String::new()), ProviderSecretStatus::RequiresReentry),
            };

            Ok(crate::db::llm::LlmProviderRecord {
                id,
                kind,
                display_name,
                base_url,
                api_key,
                models,
                default_model,
                is_default,
                extra_headers,
                secret_status,
            })
        })
        .transpose()
    }
}
