//! SQLite implementation of AgentRepository.

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::agents::{AgentId, AgentRecord, AgentRepository, AgentSummary};
use crate::db::DbError;

pub struct SqliteAgentRepository {
    pool: SqlitePool,
}

impl SqliteAgentRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn map_record(row: sqlx::sqlite::SqliteRow) -> Result<AgentRecord, DbError> {
        let tool_names_json: String =
            row.try_get("tool_names")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;
        let tool_names: Vec<String> =
            serde_json::from_str(&tool_names_json).map_err(|e| DbError::QueryFailed {
                reason: format!("failed to parse tool_names JSON: {e}"),
            })?;

        let temperature: Option<f32> = row
            .try_get::<Option<i64>, _>("temperature")
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?
            .map(|t| t as f32 / 100.0);

        Ok(AgentRecord {
            id: AgentId::new(
                row.try_get::<String, _>("id")
                    .map_err(|e| DbError::QueryFailed {
                        reason: e.to_string(),
                    })?,
            ),
            display_name: row
                .try_get("display_name")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?,
            description: row
                .try_get("description")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?,
            version: row.try_get("version").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            provider_id: row
                .try_get("provider_id")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?,
            system_prompt: row
                .try_get("system_prompt")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?,
            tool_names,
            max_tokens: row
                .try_get("max_tokens")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?,
            temperature,
        })
    }

    fn map_summary(row: sqlx::sqlite::SqliteRow) -> Result<AgentSummary, DbError> {
        Ok(AgentSummary {
            id: AgentId::new(
                row.try_get::<String, _>("id")
                    .map_err(|e| DbError::QueryFailed {
                        reason: e.to_string(),
                    })?,
            ),
            display_name: row
                .try_get("display_name")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?,
            description: row
                .try_get("description")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?,
            version: row.try_get("version").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            provider_id: row
                .try_get("provider_id")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?,
        })
    }
}

#[async_trait]
impl AgentRepository for SqliteAgentRepository {
    async fn upsert(&self, record: &AgentRecord) -> Result<(), DbError> {
        let tool_names_json =
            serde_json::to_string(&record.tool_names).map_err(|e| DbError::QueryFailed {
                reason: format!("failed to serialize tool_names: {e}"),
            })?;

        let temperature_int = record.temperature.map(|t| (t * 100.0) as i64);

        sqlx::query(
            r#"INSERT INTO agents (id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
               ON CONFLICT(id) DO UPDATE SET
                   display_name = excluded.display_name,
                   description = excluded.description,
                   version = excluded.version,
                   provider_id = excluded.provider_id,
                   system_prompt = excluded.system_prompt,
                   tool_names = excluded.tool_names,
                   max_tokens = excluded.max_tokens,
                   temperature = excluded.temperature,
                   updated_at = CURRENT_TIMESTAMP"#,
        )
        .bind(record.id.as_ref())
        .bind(&record.display_name)
        .bind(&record.description)
        .bind(&record.version)
        .bind(&record.provider_id)
        .bind(&record.system_prompt)
        .bind(&tool_names_json)
        .bind(record.max_tokens.map(|t| t as i64))
        .bind(temperature_int)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn get(&self, id: &AgentId) -> Result<Option<AgentRecord>, DbError> {
        let row = sqlx::query(
            r#"SELECT id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature
               FROM agents
               WHERE id = ?1"#,
        )
        .bind(id.as_ref())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(Self::map_record).transpose()
    }

    async fn list(&self) -> Result<Vec<AgentSummary>, DbError> {
        let rows = sqlx::query(
            r#"SELECT id, display_name, description, version, provider_id
               FROM agents
               ORDER BY display_name ASC"#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter().map(Self::map_summary).collect()
    }

    async fn delete(&self, id: &AgentId) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM agents WHERE id = ?1")
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }
}
