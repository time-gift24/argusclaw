//! LlmProviderRepository implementation for SQLite.

use std::collections::HashMap;

use async_trait::async_trait;

use argus_protocol::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderKindParseError, LlmProviderRecord,
    LlmProviderRepository, ModelConfig, ProviderSecretStatus, SecretString,
};

use crate::error::DbError;
use crate::sqlite::ArgusSqlite;
use argus_protocol::ArgusError;

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
impl LlmProviderRepository for ArgusSqlite {
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

        let encrypted = self
            .write_cipher
            .encrypt(record.api_key.expose_secret())
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        let extra_headers_json = serialize_json(&record.extra_headers)?;
        let models_json = serialize_json(&record.models)?;
        let meta_data_json = serialize_json(&record.meta_data)?;
        let model_config_json = serialize_json(&record.model_config)?;

        let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        if record.is_default {
            sqlx::query("UPDATE llm_providers SET is_default = 0, updated_at = CURRENT_TIMESTAMP WHERE id != ?1 AND is_default = 1")
                .bind(record.id.into_inner())
                .execute(&mut *tx)
                .await
                .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        }

        let provider_id = if record.id.into_inner() == 0 {
            sqlx::query(
                "INSERT INTO llm_providers (kind, display_name, base_url, models, model_config, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers, meta_data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )
            .bind(record.kind.as_str())
            .bind(&record.display_name)
            .bind(&record.base_url)
            .bind(&models_json)
            .bind(&model_config_json)
            .bind(&record.default_model)
            .bind(&encrypted.ciphertext)
            .bind(&encrypted.nonce)
            .bind(i64::from(record.is_default))
            .bind(&extra_headers_json)
            .bind(&meta_data_json)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

            let new_id: i64 = sqlx::query_scalar("SELECT last_insert_rowid()")
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: format!("failed to get last_insert_rowid: {e}"),
                })?;

            LlmProviderId::new(new_id)
        } else {
            sqlx::query(
                "INSERT INTO llm_providers (id, kind, display_name, base_url, models, model_config, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers, meta_data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(id) DO UPDATE SET
                     kind = excluded.kind,
                     display_name = excluded.display_name,
                     base_url = excluded.base_url,
                     models = excluded.models,
                     model_config = excluded.model_config,
                     default_model = excluded.default_model,
                     encrypted_api_key = excluded.encrypted_api_key,
                     api_key_nonce = excluded.api_key_nonce,
                     is_default = excluded.is_default,
                     extra_headers = excluded.extra_headers,
                     meta_data = excluded.meta_data,
                     updated_at = CURRENT_TIMESTAMP",
            )
            .bind(record.id.into_inner())
            .bind(record.kind.as_str())
            .bind(&record.display_name)
            .bind(&record.base_url)
            .bind(&models_json)
            .bind(&model_config_json)
            .bind(&record.default_model)
            .bind(&encrypted.ciphertext)
            .bind(&encrypted.nonce)
            .bind(i64::from(record.is_default))
            .bind(&extra_headers_json)
            .bind(&meta_data_json)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

            record.id
        };

        tx.commit().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(provider_id)
    }

    async fn delete_provider(&self, id: &LlmProviderId) -> Result<bool, ArgusError> {
        let result = sqlx::query("DELETE FROM llm_providers WHERE id = ?1")
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

        let exists: i64 = sqlx::query_scalar("SELECT count(1) FROM llm_providers WHERE id = ?1")
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

        sqlx::query("UPDATE llm_providers SET is_default = 0, updated_at = CURRENT_TIMESTAMP WHERE is_default = 1")
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        sqlx::query(
            "UPDATE llm_providers SET is_default = 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
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
            "SELECT id, kind, display_name, base_url, models, model_config, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers, meta_data
             FROM llm_providers WHERE id = ?1",
        )
        .bind(id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_llm_record(r)).transpose()
    }

    async fn list_providers(&self) -> Result<Vec<LlmProviderRecord>, ArgusError> {
        let rows = sqlx::query(
            "SELECT id, kind, display_name, base_url, models, model_config, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers, meta_data
             FROM llm_providers ORDER BY display_name ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_llm_record(r)).collect()
    }

    async fn get_default_provider(&self) -> Result<Option<LlmProviderRecord>, ArgusError> {
        let row = sqlx::query(
            "SELECT id, kind, display_name, base_url, models, model_config, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers, meta_data
             FROM llm_providers WHERE is_default = 1 LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_llm_record(r)).transpose()
    }

    async fn get_default_provider_id(&self) -> Result<Option<LlmProviderId>, ArgusError> {
        let id: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM llm_providers WHERE is_default = 1 LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(id.map(LlmProviderId::new))
    }
}

impl ArgusSqlite {
    #[allow(clippy::type_complexity)]
    pub(super) fn parse_llm_shared_fields(
        row: sqlx::sqlite::SqliteRow,
    ) -> Result<
        (
            LlmProviderId,
            LlmProviderKind,
            String,
            String,
            Vec<String>,
            HashMap<String, ModelConfig>,
            String,
            bool,
            HashMap<String, String>,
            HashMap<String, String>,
            Vec<u8>,
            Vec<u8>,
        ),
        ArgusError,
    > {
        let nonce: Vec<u8> = Self::get_column(&row, "api_key_nonce").map_err(map_err)?;
        let ciphertext: Vec<u8> = Self::get_column(&row, "encrypted_api_key").map_err(map_err)?;
        let extra_headers: HashMap<String, String> = serde_json::from_str(
            &Self::get_column::<String>(&row, "extra_headers").map_err(map_err)?,
        )
        .map_err(|e| {
            ArgusError::from(DbError::QueryFailed {
                reason: format!("failed to parse extra_headers: {e}"),
            })
        })?;
        let meta_data: HashMap<String, String> =
            serde_json::from_str(&Self::get_column::<String>(&row, "meta_data").map_err(map_err)?)
                .map_err(|e| {
                    ArgusError::from(DbError::QueryFailed {
                        reason: format!("failed to parse meta_data: {e}"),
                    })
                })?;
        let model_config: HashMap<String, ModelConfig> = serde_json::from_str(
            &Self::get_column::<String>(&row, "model_config").map_err(map_err)?,
        )
        .map_err(|e| {
            ArgusError::from(DbError::QueryFailed {
                reason: format!("failed to parse model_config: {e}"),
            })
        })?;
        // Try to parse models as Vec<String> (new format)
        // If that fails, try to parse as array of objects and extract the "id" or "name" field (old format)
        let models_raw: String = Self::get_column::<String>(&row, "models").map_err(map_err)?;
        let models: Vec<String> = match serde_json::from_str::<Vec<String>>(&models_raw) {
            Ok(models) => models,
            Err(_) => {
                // Try old format: array of objects with "id" or "name" field
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
        let kind: LlmProviderKind = Self::get_column::<String>(&row, "kind")
            .map_err(map_err)?
            .parse()
            .map_err(|e: LlmProviderKindParseError| {
                ArgusError::from(DbError::InvalidProviderKind {
                    kind: e.to_string(),
                })
            })?;

        Ok((
            LlmProviderId::new(Self::get_column(&row, "id").map_err(map_err)?),
            kind,
            Self::get_column(&row, "display_name").map_err(map_err)?,
            Self::get_column(&row, "base_url").map_err(map_err)?,
            models,
            model_config,
            Self::get_column(&row, "default_model").map_err(map_err)?,
            Self::get_column::<i64>(&row, "is_default").map_err(map_err)? != 0,
            extra_headers,
            meta_data,
            nonce,
            ciphertext,
        ))
    }

    fn map_llm_record(
        &self,
        row: sqlx::sqlite::SqliteRow,
    ) -> Result<LlmProviderRecord, ArgusError> {
        let (
            id,
            kind,
            display_name,
            base_url,
            models,
            model_config,
            default_model,
            is_default,
            extra_headers,
            meta_data,
            nonce,
            ciphertext,
        ) = Self::parse_llm_shared_fields(row)?;

        // Attempt decryption; if it fails, return record with empty api_key
        // and RequiresReentry status so the user knows to re-enter the key
        let (api_key, secret_status) = match self.decrypt_secret(&nonce, &ciphertext) {
            Ok(key) => (key, ProviderSecretStatus::Ready),
            Err(_) => (
                SecretString::new(String::new()),
                ProviderSecretStatus::RequiresReentry,
            ),
        };

        Ok(LlmProviderRecord {
            id,
            kind,
            display_name,
            base_url,
            api_key,
            models,
            model_config,
            default_model,
            is_default,
            extra_headers,
            secret_status,
            meta_data,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json;

    #[test]
    fn test_parse_models_new_format() {
        let models_json = r#"["model1", "model2"]"#;
        let models: Vec<String> = serde_json::from_str(models_json).unwrap();
        assert_eq!(models, vec!["model1", "model2"]);
    }

    #[test]
    fn test_parse_models_old_format_with_id() {
        let models_json = r#"[{"id":"model1","name":"Model 1"},{"id":"model2","name":"Model 2"}]"#;
        let objects: Vec<serde_json::Value> = serde_json::from_str(models_json).unwrap();
        let models: Vec<String> = objects
            .into_iter()
            .map(|v| {
                v.get("id")
                    .or_else(|| v.get("name"))
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string())
                    .unwrap()
            })
            .collect();
        assert_eq!(models, vec!["model1", "model2"]);
    }

    #[test]
    fn test_parse_models_old_format_with_name_only() {
        let models_json = r#"[{"name":"model1"},{"name":"model2"}]"#;
        let objects: Vec<serde_json::Value> = serde_json::from_str(models_json).unwrap();
        let models: Vec<String> = objects
            .into_iter()
            .map(|v| {
                v.get("id")
                    .or_else(|| v.get("name"))
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string())
                    .unwrap()
            })
            .collect();
        assert_eq!(models, vec!["model1", "model2"]);
    }
}
