//! AgentRepository implementation for SQLite.

use async_trait::async_trait;

use crate::error::DbError;
use crate::traits::AgentRepository;
use crate::types::{AgentId, AgentRecord};
use argus_protocol::ProviderId;

use super::{ArgusSqlite, DbResult};

#[async_trait]
impl AgentRepository for ArgusSqlite {
    async fn upsert(&self, record: &AgentRecord) -> DbResult<()> {
        let tool_names_json =
            serde_json::to_string(&record.tool_names).map_err(|e| DbError::QueryFailed {
                reason: format!("failed to serialize tool_names: {e}"),
            })?;
        let temperature_int = record.temperature.map(|t| (t * 100.0) as i64);

        if record.id.into_inner() == 0 {
            sqlx::query(
                "INSERT INTO agents (display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .bind(&record.display_name)
            .bind(&record.description)
            .bind(&record.version)
            .bind(record.provider_id.as_ref().map(|id| id.into_inner()))
            .bind(&record.system_prompt)
            .bind(&tool_names_json)
            .bind(record.max_tokens.map(|t| t as i64))
            .bind(temperature_int)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        } else {
            sqlx::query(
                "INSERT INTO agents (id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature)
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
                     updated_at = CURRENT_TIMESTAMP",
            )
            .bind(record.id.into_inner())
            .bind(&record.display_name)
            .bind(&record.description)
            .bind(&record.version)
            .bind(record.provider_id.as_ref().map(|id| id.into_inner()))
            .bind(&record.system_prompt)
            .bind(&tool_names_json)
            .bind(record.max_tokens.map(|t| t as i64))
            .bind(temperature_int)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        }

        Ok(())
    }

    async fn get(&self, id: &AgentId) -> DbResult<Option<AgentRecord>> {
        let row = sqlx::query(
            "SELECT id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature
             FROM agents WHERE id = ?1",
        )
        .bind(id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_agent_record(r)).transpose()
    }

    async fn find_by_display_name(&self, display_name: &str) -> DbResult<Option<AgentRecord>> {
        let row = sqlx::query(
            "SELECT id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature
             FROM agents WHERE display_name = ?1 LIMIT 1",
        )
        .bind(display_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_agent_record(r)).transpose()
    }

    async fn list(&self) -> DbResult<Vec<AgentRecord>> {
        let rows = sqlx::query(
            "SELECT id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature
             FROM agents ORDER BY display_name ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_agent_record(r)).collect()
    }

    async fn delete(&self, id: &AgentId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM agents WHERE id = ?1")
            .bind(id.into_inner())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }
}

impl ArgusSqlite {
    fn map_agent_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<AgentRecord> {
        let tool_names: Vec<String> =
            serde_json::from_str(&Self::get_column::<String>(&row, "tool_names")?).map_err(
                |e| DbError::QueryFailed {
                    reason: format!("failed to parse tool_names: {e}"),
                },
            )?;
        let temperature: Option<f32> =
            Self::get_column::<Option<i64>>(&row, "temperature")?.map(|t| t as f32 / 100.0);
        let provider_id: Option<i64> = Self::get_column::<Option<i64>>(&row, "provider_id")?;

        Ok(AgentRecord {
            id: AgentId::new(Self::get_column(&row, "id")?),
            display_name: Self::get_column(&row, "display_name")?,
            description: Self::get_column(&row, "description")?,
            version: Self::get_column(&row, "version")?,
            provider_id: provider_id.map(ProviderId::new),
            system_prompt: Self::get_column(&row, "system_prompt")?,
            tool_names,
            max_tokens: Self::get_column::<Option<i64>>(&row, "max_tokens")?.map(|t| t as u32),
            temperature,
        })
    }
}
