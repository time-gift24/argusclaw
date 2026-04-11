//! AgentRepository implementation for SQLite.

use async_trait::async_trait;
use sqlx::Row;

use crate::error::DbError;
use crate::traits::AgentRepository;
use crate::types::{AgentId, AgentRecord};
use argus_protocol::ProviderId;

use super::{ArgusSqlite, DbResult};

#[async_trait]
impl AgentRepository for ArgusSqlite {
    async fn upsert(&self, record: &AgentRecord) -> DbResult<AgentId> {
        let tool_names_json =
            serde_json::to_string(&record.tool_names).map_err(|e| DbError::QueryFailed {
                reason: format!("failed to serialize tool_names: {e}"),
            })?;
        let subagent_names_json =
            serde_json::to_string(&record.subagent_names).map_err(|e| DbError::QueryFailed {
                reason: format!("failed to serialize subagent_names: {e}"),
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

        if record.id.into_inner() == 0 {
            sqlx::query(
                "INSERT INTO agents (display_name, description, version, provider_id, model_id, system_prompt, tool_names, subagent_names, max_tokens, temperature, thinking_config)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(display_name) DO UPDATE SET
                     description = excluded.description,
                     version = excluded.version,
                     provider_id = excluded.provider_id,
                     model_id = excluded.model_id,
                     system_prompt = excluded.system_prompt,
                     tool_names = excluded.tool_names,
                     subagent_names = excluded.subagent_names,
                     max_tokens = excluded.max_tokens,
                     temperature = excluded.temperature,
                     thinking_config = excluded.thinking_config,
                     updated_at = CURRENT_TIMESTAMP",
            )
            .bind(&record.display_name)
            .bind(&record.description)
            .bind(&record.version)
            .bind(record.provider_id.as_ref().map(|id| id.into_inner()))
            .bind(&record.model_id)
            .bind(&record.system_prompt)
            .bind(&tool_names_json)
            .bind(&subagent_names_json)
            .bind(record.max_tokens.map(|t| t as i64))
            .bind(temperature_int)
            .bind(&thinking_config_json)
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
            let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
            let previous_display_name: Option<String> =
                sqlx::query_scalar("SELECT display_name FROM agents WHERE id = ?1")
                    .bind(record.id.into_inner())
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| DbError::QueryFailed {
                        reason: format!("failed to fetch existing agent before update: {e}"),
                    })?;

            sqlx::query(
                "INSERT INTO agents (id, display_name, description, version, provider_id, model_id, system_prompt, tool_names, subagent_names, max_tokens, temperature, thinking_config)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(id) DO UPDATE SET
                     display_name = excluded.display_name,
                     description = excluded.description,
                     version = excluded.version,
                     provider_id = excluded.provider_id,
                     model_id = excluded.model_id,
                     system_prompt = excluded.system_prompt,
                     tool_names = excluded.tool_names,
                     subagent_names = excluded.subagent_names,
                     max_tokens = excluded.max_tokens,
                     temperature = excluded.temperature,
                     thinking_config = excluded.thinking_config,
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
            .bind(&subagent_names_json)
            .bind(record.max_tokens.map(|t| t as i64))
            .bind(temperature_int)
            .bind(&thinking_config_json)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
            if let Some(old_display_name) =
                previous_display_name.filter(|name| name != &record.display_name)
            {
                self.rewrite_subagent_references(
                    &mut tx,
                    &old_display_name,
                    &record.display_name,
                )
                .await?;
            }
            tx.commit().await.map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

            Ok(record.id)
        }
    }

    async fn get(&self, id: &AgentId) -> DbResult<Option<AgentRecord>> {
        let row = sqlx::query(
            "SELECT id, display_name, description, version, provider_id, model_id, system_prompt, tool_names, subagent_names, max_tokens, temperature, thinking_config
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
            "SELECT id, display_name, description, version, provider_id, model_id, system_prompt, tool_names, subagent_names, max_tokens, temperature, thinking_config
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
            "SELECT id, display_name, description, version, provider_id, model_id, system_prompt, tool_names, subagent_names, max_tokens, temperature, thinking_config
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

    async fn find_id_by_display_name(&self, display_name: &str) -> DbResult<Option<AgentId>> {
        let id: Option<i64> =
            sqlx::query_scalar("SELECT id FROM agents WHERE display_name = ?1 LIMIT 1")
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
            sqlx::query_scalar("SELECT COUNT(*) FROM threads WHERE template_id = ?1")
                .bind(id.into_inner())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;

        let job_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE agent_id = ?1")
            .bind(id.into_inner())
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok((thread_count, job_count))
    }
}

impl ArgusSqlite {
    async fn rewrite_subagent_references(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        old_display_name: &str,
        new_display_name: &str,
    ) -> DbResult<()> {
        let candidate_rows = sqlx::query(
            "SELECT id, subagent_names FROM agents
             WHERE subagent_names LIKE '%' || ?1 || '%'",
        )
        .bind(old_display_name)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: format!("failed to fetch subagent references for rename: {e}"),
        })?;

        for row in candidate_rows {
            let id = row.get::<i64, _>("id");
            let subagent_names: Vec<String> = serde_json::from_str(
                &row.get::<String, _>("subagent_names"),
            )
            .map_err(|e| DbError::QueryFailed {
                reason: format!("failed to parse subagent_names during rename rewrite: {e}"),
            })?;
            let mut changed = false;
            let rewritten_names: Vec<String> = subagent_names
                .into_iter()
                .map(|name| {
                    if name == old_display_name {
                        changed = true;
                        new_display_name.to_string()
                    } else {
                        name
                    }
                })
                .collect();
            if !changed {
                continue;
            }

            let rewritten_json =
                serde_json::to_string(&rewritten_names).map_err(|e| DbError::QueryFailed {
                    reason: format!(
                        "failed to serialize rewritten subagent_names during rename rewrite: {e}"
                    ),
                })?;

            sqlx::query(
                "UPDATE agents
                 SET subagent_names = ?1, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?2",
            )
            .bind(rewritten_json)
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: format!("failed to persist rewritten subagent_names: {e}"),
            })?;
        }

        Ok(())
    }

    fn map_agent_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<AgentRecord> {
        let tool_names: Vec<String> =
            serde_json::from_str(&Self::get_column::<String>(&row, "tool_names")?).map_err(
                |e| DbError::QueryFailed {
                    reason: format!("failed to parse tool_names: {e}"),
                },
            )?;
        let subagent_names: Vec<String> =
            serde_json::from_str(&Self::get_column::<String>(&row, "subagent_names")?).map_err(
                |e| DbError::QueryFailed {
                    reason: format!("failed to parse subagent_names: {e}"),
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

        Ok(AgentRecord {
            id: AgentId::new(Self::get_column(&row, "id")?),
            display_name: Self::get_column(&row, "display_name")?,
            description: Self::get_column(&row, "description")?,
            version: Self::get_column(&row, "version")?,
            provider_id: provider_id.map(ProviderId::new),
            model_id,
            system_prompt: Self::get_column(&row, "system_prompt")?,
            tool_names,
            subagent_names,
            max_tokens: Self::get_column::<Option<i64>>(&row, "max_tokens")?.map(|t| t as u32),
            temperature,
            thinking_config,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{connect, migrate, traits::AgentRepository};
    use argus_protocol::{AgentId, AgentRecord};

    use super::ArgusSqlite;

    fn sample_agent(display_name: &str, subagent_names: Vec<String>) -> AgentRecord {
        AgentRecord {
            id: AgentId::new(0),
            display_name: display_name.to_string(),
            description: format!("{display_name} description"),
            version: "1.0.0".to_string(),
            provider_id: None,
            model_id: None,
            system_prompt: format!("{display_name} prompt"),
            tool_names: vec![],
            subagent_names,
            max_tokens: None,
            temperature: None,
            thinking_config: None,
        }
    }

    #[tokio::test]
    async fn renaming_agent_rewrites_inbound_subagent_names() {
        let pool = connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite should connect");
        migrate(&pool)
            .await
            .expect("repository migrations should succeed");
        let sqlite = ArgusSqlite::new(pool);

        let worker_id = AgentRepository::upsert(&sqlite, &sample_agent("Worker", vec![]))
            .await
            .expect("worker should upsert");
        let _ = AgentRepository::upsert(
            &sqlite,
            &sample_agent("Dispatcher", vec!["Worker".to_string()]),
        )
        .await
        .expect("dispatcher should upsert");

        let mut renamed_worker = AgentRepository::get(&sqlite, &worker_id)
            .await
            .expect("worker lookup should succeed")
            .expect("worker should exist");
        renamed_worker.display_name = "Worker Renamed".to_string();
        AgentRepository::upsert(&sqlite, &renamed_worker)
            .await
            .expect("rename should upsert");

        let dispatcher = AgentRepository::find_by_display_name(&sqlite, "Dispatcher")
            .await
            .expect("dispatcher lookup should succeed")
            .expect("dispatcher should exist");
        assert_eq!(dispatcher.subagent_names, vec!["Worker Renamed"]);
    }
}
