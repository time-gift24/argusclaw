//! AgentRepository implementation for SQLite.

use async_trait::async_trait;

use crate::error::DbError;
use crate::traits::AgentRepository;
use crate::types::{AgentId, AgentRecord};
use argus_protocol::{AgentType, ProviderId};

use super::{ArgusSqlite, DbResult};

#[async_trait]
impl AgentRepository for ArgusSqlite {
    async fn upsert(&self, record: &AgentRecord) -> DbResult<AgentId> {
        let tool_names_json =
            serde_json::to_string(&record.tool_names).map_err(|e| DbError::QueryFailed {
                reason: format!("failed to serialize tool_names: {e}"),
            })?;
        let temperature_int = record.temperature.map(|t| (t * 100.0) as i64);

        let thinking_config_json = record
            .thinking_config
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| DbError::QueryFailed {
                reason: format!("failed to serialize thinking_config: {e}"),
            })?;

        let agent_type_str = match record.agent_type {
            AgentType::Standard => "standard",
            AgentType::Subagent => "subagent",
        };

        if record.id.into_inner() == 0 {
            sqlx::query(
                "INSERT INTO agents (display_name, description, version, provider_id, model_id, system_prompt, tool_names, max_tokens, temperature, thinking_config, parent_agent_id, agent_type)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(display_name) DO UPDATE SET
                     description = excluded.description,
                     version = excluded.version,
                     provider_id = excluded.provider_id,
                     model_id = excluded.model_id,
                     system_prompt = excluded.system_prompt,
                     tool_names = excluded.tool_names,
                     max_tokens = excluded.max_tokens,
                     temperature = excluded.temperature,
                     thinking_config = excluded.thinking_config,
                     parent_agent_id = excluded.parent_agent_id,
                     agent_type = excluded.agent_type,
                     updated_at = CURRENT_TIMESTAMP",
            )
            .bind(&record.display_name)
            .bind(&record.description)
            .bind(&record.version)
            .bind(record.provider_id.as_ref().map(|id| id.into_inner()))
            .bind(&record.model_id)
            .bind(&record.system_prompt)
            .bind(&tool_names_json)
            .bind(record.max_tokens.map(|t| t as i64))
            .bind(temperature_int)
            .bind(&thinking_config_json)
            .bind(record.parent_agent_id.as_ref().map(|id| id.into_inner()))
            .bind(agent_type_str)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

            let id = sqlx::query_scalar::<_, i64>("SELECT id FROM agents WHERE display_name = ?")
                .bind(&record.display_name)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: format!("failed to fetch id after upsert: {e}"),
                })?;

            Ok(AgentId::new(id))
        } else {
            sqlx::query(
                "INSERT INTO agents (id, display_name, description, version, provider_id, model_id, system_prompt, tool_names, max_tokens, temperature, thinking_config, parent_agent_id, agent_type)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(id) DO UPDATE SET
                     display_name = excluded.display_name,
                     description = excluded.description,
                     version = excluded.version,
                     provider_id = excluded.provider_id,
                     model_id = excluded.model_id,
                     system_prompt = excluded.system_prompt,
                     tool_names = excluded.tool_names,
                     max_tokens = excluded.max_tokens,
                     temperature = excluded.temperature,
                     thinking_config = excluded.thinking_config,
                     parent_agent_id = excluded.parent_agent_id,
                     agent_type = excluded.agent_type,
                     updated_at = CURRENT_TIMESTAMP",
            )
            .bind(record.id.into_inner())
            .bind(&record.display_name)
            .bind(&record.description)
            .bind(&record.version)
            .bind(record.provider_id.as_ref().map(|id| id.into_inner()))
            .bind(&record.model_id)
            .bind(&record.system_prompt)
            .bind(&tool_names_json)
            .bind(record.max_tokens.map(|t| t as i64))
            .bind(temperature_int)
            .bind(&thinking_config_json)
            .bind(record.parent_agent_id.as_ref().map(|id| id.into_inner()))
            .bind(agent_type_str)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

            Ok(record.id)
        }
    }

    async fn get(&self, id: &AgentId) -> DbResult<Option<AgentRecord>> {
        let row = sqlx::query(
            "SELECT id, display_name, description, version, provider_id, model_id, system_prompt, tool_names, max_tokens, temperature, thinking_config, parent_agent_id, agent_type
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
            "SELECT id, display_name, description, version, provider_id, model_id, system_prompt, tool_names, max_tokens, temperature, thinking_config, parent_agent_id, agent_type
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
            "SELECT id, display_name, description, version, provider_id, model_id, system_prompt, tool_names, max_tokens, temperature, thinking_config, parent_agent_id, agent_type
             FROM agents ORDER BY display_name ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_agent_record(r)).collect()
    }

    async fn list_by_parent_id(&self, parent_id: &AgentId) -> DbResult<Vec<AgentRecord>> {
        let rows = sqlx::query(
            "SELECT id, display_name, description, version, provider_id, model_id, system_prompt, tool_names, max_tokens, temperature, thinking_config, parent_agent_id, agent_type
             FROM agents WHERE parent_agent_id = ?1 ORDER BY display_name ASC",
        )
        .bind(parent_id.into_inner())
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

    async fn find_id_by_display_name(&self, display_name: &str) -> DbResult<Option<AgentId>> {
        let id: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM agents WHERE display_name = ?1 LIMIT 1",
        )
        .bind(display_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(id.map(AgentId::new))
    }

    async fn count_references(&self, id: &AgentId) -> DbResult<(i64, i64)> {
        let thread_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM threads WHERE template_id = ?1")
                .bind(id.into_inner())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        let job_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE agent_id = ?1")
                .bind(id.into_inner())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok((thread_count, job_count))
    }

    async fn add_subagent(&self, parent_id: &AgentId, child_id: &AgentId) -> DbResult<()> {
        sqlx::query(
            r#"
            UPDATE agents
            SET parent_agent_id = ?1, agent_type = 'subagent', updated_at = CURRENT_TIMESTAMP
            WHERE id = ?2
            "#,
        )
        .bind(parent_id.into_inner())
        .bind(child_id.into_inner())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(())
    }

    async fn remove_subagent(&self, parent_id: &AgentId, child_id: &AgentId) -> DbResult<()> {
        sqlx::query(
            r#"
            UPDATE agents
            SET parent_agent_id = NULL, agent_type = 'standard', updated_at = CURRENT_TIMESTAMP
            WHERE id = ?1 AND parent_agent_id = ?2
            "#,
        )
        .bind(child_id.into_inner())
        .bind(parent_id.into_inner())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(())
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
        let model_id: Option<String> = Self::get_column(&row, "model_id")?;

        // Parse thinking_config from JSON
        let thinking_config: Option<argus_protocol::llm::ThinkingConfig> =
            Self::get_column::<Option<String>>(&row, "thinking_config")?
                .as_ref()
                .map(|json_str| serde_json::from_str(json_str))
                .transpose()
                .map_err(|e| DbError::QueryFailed {
                    reason: format!("failed to parse thinking_config: {e}"),
                })?;

        // Parse parent_agent_id
        let parent_agent_id: Option<i64> = Self::get_column(&row, "parent_agent_id")?;

        // Parse agent_type
        let agent_type_str: String = Self::get_column(&row, "agent_type")?;
        let agent_type = match agent_type_str.as_str() {
            "subagent" => AgentType::Subagent,
            _ => AgentType::Standard,
        };

        Ok(AgentRecord {
            id: AgentId::new(Self::get_column(&row, "id")?),
            display_name: Self::get_column(&row, "display_name")?,
            description: Self::get_column(&row, "description")?,
            version: Self::get_column(&row, "version")?,
            provider_id: provider_id.map(ProviderId::new),
            model_id,
            system_prompt: Self::get_column(&row, "system_prompt")?,
            tool_names,
            max_tokens: Self::get_column::<Option<i64>>(&row, "max_tokens")?.map(|t| t as u32),
            temperature,
            thinking_config,
            parent_agent_id: parent_agent_id.map(AgentId::new),
            agent_type,
        })
    }
}
