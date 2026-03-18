//! LlmProviderRepository implementation for SQLite.

use async_trait::async_trait;

use crate::error::DbError;
use crate::traits::LlmProviderRepository;
use argus_protocol::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderKindParseError, LlmProviderRecord,
    LlmProviderSummary, ProviderSecretStatus,
};

use super::{ArgusSqlite, DbResult};

#[async_trait]
impl LlmProviderRepository for ArgusSqlite {
    async fn upsert_provider(&self, record: &LlmProviderRecord) -> DbResult<LlmProviderId> {
        if record.models.is_empty() {
            return Err(DbError::QueryFailed {
                reason: "At least one model is required".to_string(),
            });
        }
        if !record.models.contains(&record.default_model) {
            return Err(DbError::QueryFailed {
                reason: format!(
                    "Default model '{}' must be in models list",
                    record.default_model
                ),
            });
        }

        let encrypted = self
            .write_cipher
            .encrypt(record.api_key.expose_secret())
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        let extra_headers_json =
            serde_json::to_string(&record.extra_headers).map_err(|e| DbError::QueryFailed {
                reason: format!("failed to serialize extra_headers: {e}"),
            })?;
        let models_json =
            serde_json::to_string(&record.models).map_err(|e| DbError::QueryFailed {
                reason: format!("failed to serialize models: {e}"),
            })?;

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
                "INSERT INTO llm_providers (kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .bind(record.kind.as_str())
            .bind(&record.display_name)
            .bind(&record.base_url)
            .bind(&models_json)
            .bind(&record.default_model)
            .bind(&encrypted.ciphertext)
            .bind(&encrypted.nonce)
            .bind(i64::from(record.is_default))
            .bind(&extra_headers_json)
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
                "INSERT INTO llm_providers (id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
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
            .bind(&encrypted.ciphertext)
            .bind(&encrypted.nonce)
            .bind(i64::from(record.is_default))
            .bind(&extra_headers_json)
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

    async fn delete_provider(&self, id: &LlmProviderId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM llm_providers WHERE id = ?1")
            .bind(id.into_inner())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }

    async fn set_default_provider(&self, id: &LlmProviderId) -> DbResult<()> {
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
            });
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

    async fn get_provider(&self, id: &LlmProviderId) -> DbResult<Option<LlmProviderRecord>> {
        let row = sqlx::query(
            "SELECT id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers
             FROM llm_providers WHERE id = ?1",
        )
        .bind(id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_llm_record(r)).transpose()
    }

    async fn get_provider_summary(
        &self,
        id: &LlmProviderId,
    ) -> DbResult<Option<LlmProviderSummary>> {
        let row = sqlx::query(
            "SELECT id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers
             FROM llm_providers WHERE id = ?1",
        )
        .bind(id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_llm_summary(r)).transpose()
    }

    async fn list_providers(&self) -> DbResult<Vec<LlmProviderSummary>> {
        let rows = sqlx::query(
            "SELECT id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers
             FROM llm_providers ORDER BY display_name ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_llm_summary(r)).collect()
    }

    async fn get_default_provider(&self) -> DbResult<Option<LlmProviderRecord>> {
        let row = sqlx::query(
            "SELECT id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers
             FROM llm_providers WHERE is_default = 1 LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_llm_record(r)).transpose()
    }
}

impl ArgusSqlite {
    #[allow(clippy::type_complexity)]
    pub(super) fn parse_llm_shared_fields(
        row: sqlx::sqlite::SqliteRow,
    ) -> DbResult<(
        LlmProviderId,
        LlmProviderKind,
        String,
        String,
        Vec<String>,
        String,
        bool,
        std::collections::HashMap<String, String>,
        Vec<u8>,
        Vec<u8>,
    )> {
        let nonce: Vec<u8> = Self::get_column(&row, "api_key_nonce")?;
        let ciphertext: Vec<u8> = Self::get_column(&row, "encrypted_api_key")?;
        let extra_headers: std::collections::HashMap<String, String> = serde_json::from_str(
            &Self::get_column::<String>(&row, "extra_headers")?,
        )
        .map_err(|e| DbError::QueryFailed {
            reason: format!("failed to parse extra_headers: {e}"),
        })?;
        let models: Vec<String> =
            serde_json::from_str(&Self::get_column::<String>(&row, "models")?).map_err(|e| {
                DbError::QueryFailed {
                    reason: format!("failed to parse models: {e}"),
                }
            })?;
        let kind: LlmProviderKind = Self::get_column::<String>(&row, "kind")?.parse().map_err(
            |e: LlmProviderKindParseError| DbError::InvalidProviderKind {
                kind: e.to_string(),
            },
        )?;

        Ok((
            LlmProviderId::new(Self::get_column(&row, "id")?),
            kind,
            Self::get_column(&row, "display_name")?,
            Self::get_column(&row, "base_url")?,
            models,
            Self::get_column(&row, "default_model")?,
            Self::get_column::<i64>(&row, "is_default")? != 0,
            extra_headers,
            nonce,
            ciphertext,
        ))
    }

    fn map_llm_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<LlmProviderRecord> {
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
        ) = Self::parse_llm_shared_fields(row)?;

        Ok(LlmProviderRecord {
            id,
            kind,
            display_name,
            base_url,
            api_key: self.decrypt_secret(&nonce, &ciphertext)?,
            models,
            default_model,
            is_default,
            extra_headers,
            secret_status: ProviderSecretStatus::Ready,
        })
    }

    fn map_llm_summary(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<LlmProviderSummary> {
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
        ) = Self::parse_llm_shared_fields(row)?;

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
            models,
            default_model,
            is_default,
            extra_headers,
            secret_status,
        })
    }
}
