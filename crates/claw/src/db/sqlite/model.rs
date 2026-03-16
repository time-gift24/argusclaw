use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::db::DbError;
use crate::db::llm::{LlmModelId, LlmModelRecord, LlmModelRepository, LlmProviderId};

pub struct SqliteLlmModelRepository {
    pool: SqlitePool,
}

impl SqliteLlmModelRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn map_record(row: sqlx::sqlite::SqliteRow) -> Result<LlmModelRecord, DbError> {
        Ok(LlmModelRecord {
            id: LlmModelId::new(row.try_get::<String, _>("id").map_err(|e| {
                DbError::QueryFailed {
                    reason: e.to_string(),
                }
            })?),
            provider_id: LlmProviderId::new(row.try_get::<String, _>("provider_id").map_err(
                |e| DbError::QueryFailed {
                    reason: e.to_string(),
                },
            )?),
            name: row.try_get("name").map_err(|e| DbError::QueryFailed {
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
impl LlmModelRepository for SqliteLlmModelRepository {
    async fn upsert(&self, record: &LlmModelRecord) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO llm_models (id, provider_id, name, is_default)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                 provider_id = excluded.provider_id,
                 name = excluded.name,
                 is_default = excluded.is_default,
                 updated_at = CURRENT_TIMESTAMP",
        )
        .bind(record.id.as_ref())
        .bind(record.provider_id.as_ref())
        .bind(&record.name)
        .bind(i64::from(record.is_default))
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn delete(&self, id: &LlmModelId) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM llm_models WHERE id = ?1")
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }

    async fn get(&self, id: &LlmModelId) -> Result<Option<LlmModelRecord>, DbError> {
        let row =
            sqlx::query("SELECT id, provider_id, name, is_default FROM llm_models WHERE id = ?1")
                .bind(id.as_ref())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;

        row.map(Self::map_record).transpose()
    }

    async fn list_by_provider(
        &self,
        provider_id: &LlmProviderId,
    ) -> Result<Vec<LlmModelRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT id, provider_id, name, is_default
             FROM llm_models
             WHERE provider_id = ?1
             ORDER BY name ASC",
        )
        .bind(provider_id.as_ref())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter().map(Self::map_record).collect()
    }

    async fn set_default(&self, id: &LlmModelId) -> Result<(), DbError> {
        let mut transaction = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        // Get the provider_id for this model
        let provider_id: String =
            sqlx::query_scalar("SELECT provider_id FROM llm_models WHERE id = ?1")
                .bind(id.as_ref())
                .fetch_optional(&mut *transaction)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?
                .ok_or_else(|| DbError::QueryFailed {
                    reason: format!("model `{id}` does not exist"),
                })?;

        // Clear old default within same provider
        sqlx::query(
            "UPDATE llm_models SET is_default = 0, updated_at = CURRENT_TIMESTAMP
             WHERE provider_id = ?1 AND is_default = 1",
        )
        .bind(&provider_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        // Set new default
        sqlx::query(
            "UPDATE llm_models SET is_default = 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
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
}
