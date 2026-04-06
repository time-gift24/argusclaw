//! LlmProviderRepository implementation for PostgreSQL.
//!
//! This is a minimal implementation focused on server-side provider lookup.
//! Full upsert with encryption is handled differently than SQLite because
//! the server manages credentials through ProviderTokenCredentialRepository.

use async_trait::async_trait;
use sqlx::Row;

use argus_protocol::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderKindParseError, LlmProviderRecord,
    LlmProviderRepository, ModelConfig, ProviderSecretStatus, SecretString,
};
use argus_protocol::ArgusError;

use crate::error::DbError;

use super::{ArgusPostgres, DbResult};

/// Map DbError to ArgusError for protocol trait compatibility.
fn map_err(e: DbError) -> ArgusError {
    e.into()
}

fn serialize_json<T: serde::Serialize>(value: &T) -> Result<String, DbError> {
    serde_json::to_string(value).map_err(|e| DbError::QueryFailed {
        reason: format!("failed to serialize: {e}"),
    })
}

#[async_trait]
impl LlmProviderRepository for ArgusPostgres {
    async fn upsert_provider(
        &self,
        record: &LlmProviderRecord,
    ) -> Result<LlmProviderId, ArgusError> {
        if record.models.is_empty() {
            return Err(DbError::QueryFailed {
                reason: "At least one model is required".to_string(),
            }
            .into());
        }
        if !record.models.contains(&record.default_model) {
            return Err(DbError::QueryFailed {
                reason: format!(
                    "Default model '{}' must be in models list",
                    record.default_model
                ),
            }
            .into());
        }

        let extra_headers_json = serialize_json(&record.extra_headers).map_err(map_err)?;
        let models_json = serialize_json(&record.models).map_err(map_err)?;
        let meta_data_json = serialize_json(&record.meta_data).map_err(map_err)?;
        let model_config_json = serialize_json(&record.model_config).map_err(map_err)?;

        let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        if record.is_default {
            sqlx::query(
                "UPDATE llm_providers SET is_default = FALSE, updated_at = NOW() \
                 WHERE id != $1 AND is_default = TRUE",
            )
            .bind(record.id.into_inner())
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        }

        let provider_id = if record.id.into_inner() == 0 {
            sqlx::query(
                "INSERT INTO llm_providers (kind, display_name, base_url, models, model_config, \
                 default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers, meta_data) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            )
            .bind(record.kind.as_str())
            .bind(&record.display_name)
            .bind(&record.base_url)
            .bind(&models_json)
            .bind(&model_config_json)
            .bind(&record.default_model)
            .bind(record.api_key.expose_secret().as_bytes())
            .bind(&Vec::<u8>::new())
            .bind(record.is_default)
            .bind(&extra_headers_json)
            .bind(&meta_data_json)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

            let new_id: i64 = sqlx::query_scalar("SELECT LASTVAL()")
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: format!("failed to get last inserted id: {e}"),
                })?;

            LlmProviderId::new(new_id)
        } else {
            sqlx::query(
                "INSERT INTO llm_providers (id, kind, display_name, base_url, models, model_config, \
                 default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers, meta_data) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
                 ON CONFLICT (id) DO UPDATE SET \
                     kind = EXCLUDED.kind, \
                     display_name = EXCLUDED.display_name, \
                     base_url = EXCLUDED.base_url, \
                     models = EXCLUDED.models, \
                     model_config = EXCLUDED.model_config, \
                     default_model = EXCLUDED.default_model, \
                     encrypted_api_key = EXCLUDED.encrypted_api_key, \
                     api_key_nonce = EXCLUDED.api_key_nonce, \
                     is_default = EXCLUDED.is_default, \
                     extra_headers = EXCLUDED.extra_headers, \
                     meta_data = EXCLUDED.meta_data, \
                     updated_at = NOW()",
            )
            .bind(record.id.into_inner())
            .bind(record.kind.as_str())
            .bind(&record.display_name)
            .bind(&record.base_url)
            .bind(&models_json)
            .bind(&model_config_json)
            .bind(&record.default_model)
            .bind(record.api_key.expose_secret().as_bytes())
            .bind(&Vec::<u8>::new())
            .bind(record.is_default)
            .bind(&extra_headers_json)
            .bind(&meta_data_json)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

            record.id
        };

        tx.commit().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(provider_id)
    }

    async fn delete_provider(&self, id: &LlmProviderId) -> Result<bool, ArgusError> {
        let result = sqlx::query("DELETE FROM llm_providers WHERE id = $1")
            .bind(id.into_inner())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }

    async fn set_default_provider(&self, id: &LlmProviderId) -> Result<(), ArgusError> {
        let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        let exists: i64 =
            sqlx::query_scalar("SELECT COUNT(1) FROM llm_providers WHERE id = $1")
                .bind(id.into_inner())
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;

        if exists == 0 {
            return Err(DbError::NotFound {
                id: id.into_inner().to_string(),
            }
            .into());
        }

        sqlx::query(
            "UPDATE llm_providers SET is_default = FALSE, updated_at = NOW() WHERE is_default = TRUE",
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        sqlx::query(
            "UPDATE llm_providers SET is_default = TRUE, updated_at = NOW() WHERE id = $1",
        )
        .bind(id.into_inner())
        .execute(&mut *tx)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        tx.commit().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(())
    }

    async fn get_provider(
        &self,
        id: &LlmProviderId,
    ) -> Result<Option<LlmProviderRecord>, ArgusError> {
        let row = sqlx::query(
            "SELECT id, kind, display_name, base_url, models, model_config, default_model, \
                    encrypted_api_key, api_key_nonce, is_default, extra_headers, meta_data \
             FROM llm_providers WHERE id = $1",
        )
        .bind(id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| map_llm_record(&row)).transpose()
    }

    async fn list_providers(&self) -> Result<Vec<LlmProviderRecord>, ArgusError> {
        let rows = sqlx::query(
            "SELECT id, kind, display_name, base_url, models, model_config, default_model, \
                    encrypted_api_key, api_key_nonce, is_default, extra_headers, meta_data \
             FROM llm_providers ORDER BY display_name ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.iter().map(|row| map_llm_record(row)).collect()
    }

    async fn get_default_provider(&self) -> Result<Option<LlmProviderRecord>, ArgusError> {
        let row = sqlx::query(
            "SELECT id, kind, display_name, base_url, models, model_config, default_model, \
                    encrypted_api_key, api_key_nonce, is_default, extra_headers, meta_data \
             FROM llm_providers WHERE is_default = TRUE LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| map_llm_record(&row)).transpose()
    }

    async fn get_default_provider_id(&self) -> Result<Option<LlmProviderId>, ArgusError> {
        let id: Option<i64> =
            sqlx::query_scalar("SELECT id FROM llm_providers WHERE is_default = TRUE LIMIT 1")
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;

        Ok(id.map(LlmProviderId::new))
    }
}

fn get_column<T>(row: &sqlx::postgres::PgRow, col: &str) -> DbResult<T>
where
    T: for<'r> sqlx::decode::Decode<'r, sqlx::Postgres> + sqlx::types::Type<sqlx::Postgres>,
{
    row.try_get(col).map_err(|e| DbError::QueryFailed {
        reason: e.to_string(),
    })
}

fn map_llm_record(row: &sqlx::postgres::PgRow) -> Result<LlmProviderRecord, ArgusError> {
    let _ = map_err; // suppress unused warning
    let extra_headers: std::collections::HashMap<String, String> = serde_json::from_str(
        &get_column::<String>(row, "extra_headers").map_err(map_err)?,
    )
    .map_err(|e| {
        ArgusError::from(DbError::QueryFailed {
            reason: format!("failed to parse extra_headers: {e}"),
        })
    })?;
    let meta_data: std::collections::HashMap<String, String> = serde_json::from_str(
        &get_column::<String>(row, "meta_data").map_err(map_err)?,
    )
    .map_err(|e| {
        ArgusError::from(DbError::QueryFailed {
            reason: format!("failed to parse meta_data: {e}"),
        })
    })?;
    let model_config: std::collections::HashMap<String, ModelConfig> = serde_json::from_str(
        &get_column::<String>(row, "model_config").map_err(map_err)?,
    )
    .map_err(|e| {
        ArgusError::from(DbError::QueryFailed {
            reason: format!("failed to parse model_config: {e}"),
        })
    })?;

    let models_raw: String = get_column(row, "models").map_err(map_err)?;
    let models: Vec<String> = match serde_json::from_str::<Vec<String>>(&models_raw) {
        Ok(models) => models,
        Err(_) => {
            serde_json::from_str::<Vec<serde_json::Value>>(&models_raw)
                .map_err(|e| {
                    ArgusError::from(DbError::QueryFailed {
                        reason: format!("failed to parse models: {e}"),
                    })
                })?
                .into_iter()
                .map(|v| {
                    v.get("id")
                        .or_else(|| v.get("name"))
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string())
                        .ok_or_else(|| {
                            ArgusError::from(DbError::QueryFailed {
                                reason: format!(
                                    "invalid model object format, expected 'id' or 'name' field: {}",
                                    v
                                ),
                            })
                        })
                })
                .collect::<Result<Vec<String>, ArgusError>>()?
        }
    };

    let kind: LlmProviderKind =
        get_column::<String>(row, "kind")
            .map_err(map_err)?
            .parse()
            .map_err(|e: LlmProviderKindParseError| {
                ArgusError::from(DbError::InvalidProviderKind {
                    kind: e.to_string(),
                })
            })?;

    // For PostgreSQL, the encrypted_api_key is stored as raw bytes.
    // In the server context, we treat the stored bytes as the plaintext API key.
    let api_key_bytes: Vec<u8> = get_column(row, "encrypted_api_key").map_err(map_err)?;
    let api_key = SecretString::new(
        String::from_utf8(api_key_bytes).unwrap_or_default(),
    );
    // NOTE: from_utf8 may fail on encrypted bytes; unwrap_or_default is
    // intentional because the server stores API keys as UTF-8 plaintext,
    // not encrypted binary. A failed decode means an empty key, which is
    // handled upstream as a missing/invalid credential.

    Ok(LlmProviderRecord {
        id: LlmProviderId::new(get_column(row, "id").map_err(map_err)?),
        kind,
        display_name: get_column(row, "display_name").map_err(map_err)?,
        base_url: get_column(row, "base_url").map_err(map_err)?,
        api_key,
        models,
        model_config,
        default_model: get_column(row, "default_model").map_err(map_err)?,
        is_default: get_column::<bool>(row, "is_default").map_err(map_err)?,
        extra_headers,
        secret_status: ProviderSecretStatus::Ready,
        meta_data,
    })
}
