use argus_protocol::{AgentId, AgentRecord, ArgusError, ProviderId, Result};
use sqlx::SqlitePool;

/// Manager for agent templates.
pub struct TemplateManager {
    pool: SqlitePool,
}

impl TemplateManager {
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

        rows.into_iter().map(|row| self.map_agent_record(row)).collect()
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

    /// Delete a template.
    pub async fn delete(&self, id: AgentId) -> Result<()> {
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
    fn map_agent_record(
        &self,
        row: sqlx::sqlite::SqliteRow,
    ) -> Result<AgentRecord> {
        use sqlx::Row;

        let tool_names_str: String = row.try_get("tool_names").map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;
        let tool_names: Vec<String> =
            serde_json::from_str(&tool_names_str).unwrap_or_default();

        // Convert temperature from INTEGER to f32 (stored as value * 100)
        let temperature: Option<f32> = row
            .try_get::<Option<i64>, _>("temperature")
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?
            .map(|t| t as f32 / 100.0);

        let provider_id: Option<i64> = row
            .try_get("provider_id")
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
            display_name: row.try_get("display_name").map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?,
            description: row.try_get("description").map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?,
            version: row.try_get("version").map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?,
            provider_id: provider_id.map(ProviderId::new),
            system_prompt: row.try_get("system_prompt").map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?,
            tool_names,
            max_tokens,
            temperature,
        })
    }
}
