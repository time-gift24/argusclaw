//! AgentRepository implementation for PostgreSQL.

use async_trait::async_trait;
use sqlx::Row;

use crate::error::DbError;
use crate::traits::AgentRepository;
use crate::types::{AgentId, AgentRecord};
use argus_protocol::{AgentType, ProviderId};

use super::{ArgusPostgres, DbResult};

#[async_trait]
impl AgentRepository for ArgusPostgres {
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
                r#"
                INSERT INTO agents (display_name, description, version, provider_id, model_id,
                                    system_prompt, tool_names, max_tokens, temperature,
                                    thinking_config, parent_agent_id, agent_type, is_enabled)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
                ON CONFLICT (display_name) DO UPDATE SET
                    description = EXCLUDED.description,
                    version = EXCLUDED.version,
                    provider_id = EXCLUDED.provider_id,
                    model_id = EXCLUDED.model_id,
                    system_prompt = EXCLUDED.system_prompt,
                    tool_names = EXCLUDED.tool_names,
                    max_tokens = EXCLUDED.max_tokens,
                    temperature = EXCLUDED.temperature,
                    thinking_config = EXCLUDED.thinking_config,
                    parent_agent_id = EXCLUDED.parent_agent_id,
                    agent_type = EXCLUDED.agent_type,
                    is_enabled = EXCLUDED.is_enabled,
                    updated_at = NOW()
                "#,
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
            .bind(record.is_enabled)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

            let id: i64 = sqlx::query_scalar("SELECT id FROM agents WHERE display_name = $1")
                .bind(&record.display_name)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: format!("failed to fetch id after upsert: {e}"),
                })?;

            Ok(AgentId::new(id))
        } else {
            sqlx::query(
                r#"
                INSERT INTO agents (id, display_name, description, version, provider_id, model_id,
                                    system_prompt, tool_names, max_tokens, temperature,
                                    thinking_config, parent_agent_id, agent_type, is_enabled)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
                ON CONFLICT (id) DO UPDATE SET
                    display_name = EXCLUDED.display_name,
                    description = EXCLUDED.description,
                    version = EXCLUDED.version,
                    provider_id = EXCLUDED.provider_id,
                    model_id = EXCLUDED.model_id,
                    system_prompt = EXCLUDED.system_prompt,
                    tool_names = EXCLUDED.tool_names,
                    max_tokens = EXCLUDED.max_tokens,
                    temperature = EXCLUDED.temperature,
                    thinking_config = EXCLUDED.thinking_config,
                    parent_agent_id = EXCLUDED.parent_agent_id,
                    agent_type = EXCLUDED.agent_type,
                    is_enabled = EXCLUDED.is_enabled,
                    updated_at = NOW()
                "#,
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
            .bind(record.is_enabled)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

            Ok(record.id)
        }
    }

    async fn get(&self, id: &AgentId) -> DbResult<Option<AgentRecord>> {
        let row = sqlx::query(
            "SELECT id, display_name, description, version, provider_id, model_id, \
                    system_prompt, tool_names, max_tokens, temperature, thinking_config, \
                    parent_agent_id, agent_type, is_enabled \
             FROM agents WHERE id = $1",
        )
        .bind(id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| map_agent_record(&row)).transpose()
    }

    async fn find_by_display_name(&self, display_name: &str) -> DbResult<Option<AgentRecord>> {
        let row = sqlx::query(
            "SELECT id, display_name, description, version, provider_id, model_id, \
                    system_prompt, tool_names, max_tokens, temperature, thinking_config, \
                    parent_agent_id, agent_type, is_enabled \
             FROM agents WHERE display_name = $1 LIMIT 1",
        )
        .bind(display_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| map_agent_record(&row)).transpose()
    }

    async fn list(&self) -> DbResult<Vec<AgentRecord>> {
        let rows = sqlx::query(
            "SELECT id, display_name, description, version, provider_id, model_id, \
                    system_prompt, tool_names, max_tokens, temperature, thinking_config, \
                    parent_agent_id, agent_type, is_enabled \
             FROM agents ORDER BY display_name ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.iter().map(|row| map_agent_record(row)).collect()
    }

    async fn list_by_parent_id(&self, parent_id: &AgentId) -> DbResult<Vec<AgentRecord>> {
        let rows = sqlx::query(
            "SELECT id, display_name, description, version, provider_id, model_id, \
                    system_prompt, tool_names, max_tokens, temperature, thinking_config, \
                    parent_agent_id, agent_type, is_enabled \
             FROM agents WHERE parent_agent_id = $1 ORDER BY display_name ASC",
        )
        .bind(parent_id.into_inner())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.iter().map(|row| map_agent_record(row)).collect()
    }

    async fn delete(&self, id: &AgentId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM agents WHERE id = $1")
            .bind(id.into_inner())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }

    async fn find_id_by_display_name(&self, display_name: &str) -> DbResult<Option<AgentId>> {
        let id: Option<i64> =
            sqlx::query_scalar("SELECT id FROM agents WHERE display_name = $1 LIMIT 1")
                .bind(display_name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;

        Ok(id.map(AgentId::new))
    }

    async fn count_references(&self, id: &AgentId) -> DbResult<(i64, i64)> {
        let thread_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM threads WHERE template_id = $1")
                .bind(id.into_inner())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;

        let job_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE agent_id = $1")
            .bind(id.into_inner())
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok((thread_count, job_count))
    }

    async fn add_subagent(&self, parent_id: &AgentId, child_id: &AgentId) -> DbResult<()> {
        sqlx::query(
            "UPDATE agents SET parent_agent_id = $1, agent_type = 'subagent', updated_at = NOW() \
             WHERE id = $2",
        )
        .bind(parent_id.into_inner())
        .bind(child_id.into_inner())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn remove_subagent(&self, parent_id: &AgentId, child_id: &AgentId) -> DbResult<()> {
        sqlx::query(
            "UPDATE agents SET parent_agent_id = NULL, agent_type = 'standard', updated_at = NOW() \
             WHERE id = $1 AND parent_agent_id = $2",
        )
        .bind(child_id.into_inner())
        .bind(parent_id.into_inner())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
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

fn map_agent_record(row: &sqlx::postgres::PgRow) -> DbResult<AgentRecord> {
    let tool_names: Vec<String> = serde_json::from_str(&get_column::<String>(row, "tool_names")?)
        .map_err(|e| DbError::QueryFailed {
        reason: format!("failed to parse tool_names: {e}"),
    })?;
    let temperature: Option<f32> =
        get_column::<Option<i64>>(row, "temperature")?.map(|t| t as f32 / 100.0);
    let provider_id: Option<i64> = get_column(row, "provider_id")?;
    let model_id: Option<String> = get_column(row, "model_id")?;

    let thinking_config: Option<argus_protocol::llm::ThinkingConfig> =
        get_column::<Option<String>>(row, "thinking_config")?
            .as_ref()
            .map(|json_str| serde_json::from_str(json_str))
            .transpose()
            .map_err(|e| DbError::QueryFailed {
                reason: format!("failed to parse thinking_config: {e}"),
            })?;

    let parent_agent_id: Option<i64> = get_column(row, "parent_agent_id")?;
    let agent_type_str: String = get_column(row, "agent_type")?;
    let agent_type = match agent_type_str.as_str() {
        "subagent" => AgentType::Subagent,
        _ => AgentType::Standard,
    };

    let is_enabled: bool = get_column(row, "is_enabled")?;

    Ok(AgentRecord {
        id: AgentId::new(get_column(row, "id")?),
        display_name: get_column(row, "display_name")?,
        description: get_column(row, "description")?,
        version: get_column(row, "version")?,
        provider_id: provider_id.map(ProviderId::new),
        model_id,
        system_prompt: get_column(row, "system_prompt")?,
        tool_names,
        max_tokens: get_column::<Option<i64>>(row, "max_tokens")?.map(|t| t as u32),
        temperature,
        thinking_config,
        parent_agent_id: parent_agent_id.map(AgentId::new),
        agent_type,
        is_enabled,
    })
}
