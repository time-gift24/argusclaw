use argus_protocol::{AgentId, AgentRecord, ArgusError, ProviderId, Result};
use sqlx::SqlitePool;

/// Manager for agent templates.
pub struct TemplateManager {
    pool: SqlitePool,
}

fn format_delete_blocked_reason(
    id: AgentId,
    thread_count: i64,
    approval_request_count: i64,
    job_count: i64,
) -> String {
    let mut blockers = Vec::new();

    if thread_count > 0 {
        blockers.push(format!("{} 个会话线程", thread_count));
    }
    if approval_request_count > 0 {
        blockers.push(format!("{} 个审批请求", approval_request_count));
    }
    if job_count > 0 {
        blockers.push(format!("{} 个任务", job_count));
    }

    format!(
        "无法删除智能体 {}：当前仍被 {} 引用，请先删除相关会话、审批或任务。",
        id.inner(),
        blockers.join("、")
    )
}

impl TemplateManager {
    /// Find the agents directory by traversing up from the given start path.
    fn find_agents_directory(start_path: &str) -> Result<std::path::PathBuf> {
        use std::path::Path;
        let start = Path::new(start_path);

        // Traverse up the directory tree looking for agents/ directory
        let mut current = Some(start);

        while let Some(path) = current {
            // Check if agents/ exists at this level
            let agents_path = path.join("agents");
            if agents_path.exists() && agents_path.is_dir() {
                return Ok(agents_path);
            }

            // Move up one directory
            current = path.parent();
        }

        // If not found, provide a helpful error message
        Err(ArgusError::DatabaseError {
            reason: format!(
                "could not find agents/ directory starting from {}",
                start.display()
            ),
        })
    }

    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Upsert (create or update) an agent template.
    pub async fn upsert(&self, template: AgentRecord) -> Result<AgentId> {
        let tool_names_json =
            serde_json::to_string(&template.tool_names).map_err(|e| ArgusError::SerdeError {
                reason: e.to_string(),
            })?;

        // Convert temperature from f32 to i64 (stored as INTEGER * 100)
        let temperature_int = template.temperature.map(|t| (t * 100.0) as i64);

        if template.id.inner() == 0 {
            let result = sqlx::query(
                r#"
                INSERT INTO agents (display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature, created_at, updated_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, datetime('now'), datetime('now'))
                "#,
            )
            .bind(&template.display_name)
            .bind(&template.description)
            .bind(&template.version)
            .bind(template.provider_id.map(|p| p.inner()))
            .bind(&template.system_prompt)
            .bind(&tool_names_json)
            .bind(template.max_tokens.map(|t| t as i64))
            .bind(temperature_int)
            .execute(&self.pool)
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

            Ok(AgentId::new(result.last_insert_rowid()))
        } else {
            sqlx::query(
                r#"
                INSERT INTO agents (id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature, created_at, updated_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'), datetime('now'))
                ON CONFLICT(id) DO UPDATE SET
                    display_name = excluded.display_name,
                    description = excluded.description,
                    version = excluded.version,
                    provider_id = excluded.provider_id,
                    system_prompt = excluded.system_prompt,
                    tool_names = excluded.tool_names,
                    max_tokens = excluded.max_tokens,
                    temperature = excluded.temperature,
                    updated_at = datetime('now')
                "#,
            )
            .bind(template.id.inner())
            .bind(&template.display_name)
            .bind(&template.description)
            .bind(&template.version)
            .bind(template.provider_id.map(|p| p.inner()))
            .bind(&template.system_prompt)
            .bind(&tool_names_json)
            .bind(template.max_tokens.map(|t| t as i64))
            .bind(temperature_int)
            .execute(&self.pool)
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

            Ok(template.id)
        }
    }

    /// Upsert an agent by display_name (insert or update if exists).
    pub async fn upsert_by_display_name(&self, record: &AgentRecord) -> Result<AgentId> {
        let tool_names_json =
            serde_json::to_string(&record.tool_names).map_err(|e| ArgusError::SerdeError {
                reason: e.to_string(),
            })?;
        let temperature_int = record.temperature.map(|t| (t * 100.0) as i64);

        // Insert with ON CONFLICT(display_name) DO UPDATE
        sqlx::query(
            "INSERT INTO agents (display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(display_name) DO UPDATE SET
                 description = excluded.description,
                 version = excluded.version,
                 provider_id = excluded.provider_id,
                 system_prompt = excluded.system_prompt,
                 tool_names = excluded.tool_names,
                 max_tokens = excluded.max_tokens,
                 temperature = excluded.temperature,
                 updated_at = CURRENT_TIMESTAMP",
        )
        .bind(&record.display_name)
        .bind(&record.description)
        .bind(&record.version)
        .bind(record.provider_id.as_ref().map(|id| id.inner()))
        .bind(&record.system_prompt)
        .bind(&tool_names_json)
        .bind(record.max_tokens.map(|t| t as i64))
        .bind(temperature_int)
        .execute(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        // Fetch the agent ID (either newly inserted or updated)
        let id = sqlx::query_scalar::<_, i64>("SELECT id FROM agents WHERE display_name = ?1")
            .bind(&record.display_name)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        Ok(AgentId::new(id))
    }

    /// Repair legacy placeholder agent IDs that were incorrectly persisted as `0`.
    pub async fn repair_placeholder_ids(&self) -> Result<()> {
        let placeholder_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agents WHERE id = 0")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        if placeholder_count == 0 {
            return Ok(());
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        sqlx::query(
            r#"
            INSERT INTO agents (display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature, created_at, updated_at)
            SELECT display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature, created_at, updated_at
            FROM agents
            WHERE id = 0
            "#,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        let repaired_id: i64 = sqlx::query_scalar("SELECT last_insert_rowid()")
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        for statement in [
            "UPDATE threads SET template_id = ? WHERE template_id = 0",
            "UPDATE approval_requests SET agent_id = ? WHERE agent_id = 0",
            "UPDATE jobs SET agent_id = ? WHERE agent_id = 0",
        ] {
            sqlx::query(statement)
                .bind(repaired_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| ArgusError::DatabaseError {
                    reason: e.to_string(),
                })?;
        }

        sqlx::query("DELETE FROM agents WHERE id = 0")
            .execute(&mut *tx)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        tx.commit().await.map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    /// Seed builtin agents from agents/ directory at runtime.
    pub async fn seed_builtin_agents(&self) -> Result<()> {
        use crate::config::load_builtin_agents_from_dir;
        use std::env;

        tracing::info!("seeding builtin agents from agents/ directory");

        // Resolve agents directory using CARGO_MANIFEST_DIR
        let manifest_dir =
            env::var("CARGO_MANIFEST_DIR").map_err(|e| ArgusError::DatabaseError {
                reason: format!("CARGO_MANIFEST_DIR not set: {}", e),
            })?;

        // Find agents directory by traversing up from CARGO_MANIFEST_DIR
        let agents_dir = Self::find_agents_directory(&manifest_dir)?;

        tracing::info!("using agents directory: {}", agents_dir.display());

        // Load all TOML definitions
        let toml_defs =
            load_builtin_agents_from_dir(&agents_dir).map_err(|e| ArgusError::DatabaseError {
                reason: format!("failed to load builtin agents: {}", e),
            })?;

        tracing::info!(
            "loaded {} builtin agent definitions from {}",
            toml_defs.len(),
            agents_dir.display()
        );

        // Upsert each agent by display_name
        for def in toml_defs {
            let record = def.to_agent_record();
            let agent_id = self.upsert_by_display_name(&record).await.map_err(|e| {
                ArgusError::DatabaseError {
                    reason: format!("failed to seed agent '{}': {}", record.display_name, e),
                }
            })?;

            tracing::info!(
                "seeded builtin agent '{}' (id={})",
                record.display_name,
                agent_id.inner()
            );
        }

        tracing::info!("successfully seeded builtin agents");
        Ok(())
    }

    /// Get a template by ID.
    pub async fn get(&self, id: AgentId) -> Result<Option<AgentRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature
            FROM agents WHERE id = ?
            "#,
        )
        .bind(id.inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        match row {
            Some(row) => Ok(Some(self.map_agent_record(row)?)),
            None => Ok(None),
        }
    }

    /// List all templates.
    pub async fn list(&self) -> Result<Vec<AgentRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature
            FROM agents ORDER BY id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        rows.into_iter()
            .map(|row| self.map_agent_record(row))
            .collect()
    }

    /// Find a template by display name.
    pub async fn find_by_display_name(&self, display_name: &str) -> Result<Option<AgentRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature
            FROM agents WHERE display_name = ?
            "#,
        )
        .bind(display_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        match row {
            Some(row) => Ok(Some(self.map_agent_record(row)?)),
            None => Ok(None),
        }
    }

    async fn count_references(&self, id: AgentId) -> Result<(i64, i64, i64)> {
        let thread_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM threads WHERE template_id = ?")
                .bind(id.inner())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| ArgusError::DatabaseError {
                    reason: e.to_string(),
                })?;

        let approval_request_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM approval_requests WHERE agent_id = ?")
                .bind(id.inner())
                .fetch_one(&self.pool)
                .await
                .map_err(|e| ArgusError::DatabaseError {
                    reason: e.to_string(),
                })?;

        let job_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE agent_id = ?")
            .bind(id.inner())
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        Ok((thread_count, approval_request_count, job_count))
    }

    /// Delete a template.
    pub async fn delete(&self, id: AgentId) -> Result<()> {
        let (thread_count, approval_request_count, job_count) = self.count_references(id).await?;

        if thread_count > 0 || approval_request_count > 0 || job_count > 0 {
            return Err(ArgusError::DatabaseError {
                reason: format_delete_blocked_reason(
                    id,
                    thread_count,
                    approval_request_count,
                    job_count,
                ),
            });
        }

        sqlx::query("DELETE FROM agents WHERE id = ?")
            .bind(id.inner())
            .execute(&self.pool)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        Ok(())
    }
}

impl TemplateManager {
    fn map_agent_record(&self, row: sqlx::sqlite::SqliteRow) -> Result<AgentRecord> {
        use sqlx::Row;

        let tool_names_str: String =
            row.try_get("tool_names")
                .map_err(|e| ArgusError::DatabaseError {
                    reason: e.to_string(),
                })?;
        let tool_names: Vec<String> = serde_json::from_str(&tool_names_str).unwrap_or_default();

        // Convert temperature from INTEGER to f32 (stored as value * 100)
        let temperature: Option<f32> = row
            .try_get::<Option<i64>, _>("temperature")
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?
            .map(|t| t as f32 / 100.0);

        let provider_id: Option<i64> =
            row.try_get("provider_id")
                .map_err(|e| ArgusError::DatabaseError {
                    reason: e.to_string(),
                })?;

        let max_tokens: Option<u32> = row
            .try_get::<Option<i64>, _>("max_tokens")
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?
            .map(|t| t as u32);

        Ok(AgentRecord {
            id: AgentId::new(row.try_get("id").map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?),
            display_name: row
                .try_get("display_name")
                .map_err(|e| ArgusError::DatabaseError {
                    reason: e.to_string(),
                })?,
            description: row
                .try_get("description")
                .map_err(|e| ArgusError::DatabaseError {
                    reason: e.to_string(),
                })?,
            version: row
                .try_get("version")
                .map_err(|e| ArgusError::DatabaseError {
                    reason: e.to_string(),
                })?,
            provider_id: provider_id.map(ProviderId::new),
            system_prompt: row
                .try_get("system_prompt")
                .map_err(|e| ArgusError::DatabaseError {
                    reason: e.to_string(),
                })?,
            tool_names,
            max_tokens,
            temperature,
        })
    }
}
