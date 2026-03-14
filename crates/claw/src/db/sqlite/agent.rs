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

    /// Returns a reference to the underlying pool.
    ///
    /// This is exposed for integration tests that need to insert test data.
    /// Production code should use the repository methods instead.
    #[must_use]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Helper to get a column value with consistent error mapping.
    fn get<T>(row: &sqlx::sqlite::SqliteRow, col: &str) -> Result<T, DbError>
    where
        T: for<'r> sqlx::decode::Decode<'r, sqlx::Sqlite> + sqlx::types::Type<sqlx::Sqlite>,
    {
        row.try_get(col).map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })
    }

    fn map_record(row: sqlx::sqlite::SqliteRow) -> Result<AgentRecord, DbError> {
        let tool_names_json: String = Self::get::<String>(&row, "tool_names")?;
        let tool_names: Vec<String> =
            serde_json::from_str(&tool_names_json).map_err(|e| DbError::QueryFailed {
                reason: format!("failed to parse tool_names JSON: {e}"),
            })?;

        let temperature: Option<f32> =
            Self::get::<Option<i64>>(&row, "temperature")?.map(|t| t as f32 / 100.0);

        // Handle nullable provider_id - NULL becomes empty string
        let provider_id: String =
            Self::get::<Option<String>>(&row, "provider_id")?.unwrap_or_default();

        Ok(AgentRecord {
            id: AgentId::new(Self::get::<String>(&row, "id")?),
            display_name: Self::get::<String>(&row, "display_name")?,
            description: Self::get::<String>(&row, "description")?,
            version: Self::get::<String>(&row, "version")?,
            provider_id,
            system_prompt: Self::get::<String>(&row, "system_prompt")?,
            tool_names,
            max_tokens: Self::get::<Option<i64>>(&row, "max_tokens")?.map(|t| t as u32),
            temperature,
        })
    }

    fn map_summary(row: sqlx::sqlite::SqliteRow) -> Result<AgentSummary, DbError> {
        // Handle nullable provider_id - NULL becomes empty string
        let provider_id: String =
            Self::get::<Option<String>>(&row, "provider_id")?.unwrap_or_default();

        Ok(AgentSummary {
            id: AgentId::new(Self::get::<String>(&row, "id")?),
            display_name: Self::get::<String>(&row, "display_name")?,
            description: Self::get::<String>(&row, "description")?,
            version: Self::get::<String>(&row, "version")?,
            provider_id,
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
