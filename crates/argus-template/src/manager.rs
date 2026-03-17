use argus_protocol::{AgentId, ProviderId, Result, ArgusError};
use serde::{Deserialize, Serialize};
use sqlx::{SqlitePool, Row};

/// Agent template - a saved agent configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTemplate {
    /// Template ID.
    pub id: AgentId,
    /// Display name.
    pub display_name: String,
    /// Description.
    pub description: String,
    /// Version string.
    pub version: String,
    /// Associated provider ID (optional).
    pub provider_id: Option<ProviderId>,
    /// System prompt.
    pub system_prompt: String,
    /// List of tool names enabled for this template.
    pub tool_names: Vec<String>,
    /// Max tokens for LLM requests.
    pub max_tokens: Option<i32>,
    /// Temperature setting.
    pub temperature: Option<i32>,
}

/// Manager for agent templates.
pub struct TemplateManager {
    pool: SqlitePool,
}

impl TemplateManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Upsert (create or update) an agent template.
    pub async fn upsert(&self, template: AgentTemplate) -> Result<AgentId> {
        let tool_names_json = serde_json::to_string(&template.tool_names)
            .map_err(|e| ArgusError::SerdeError { reason: e.to_string() })?;

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
        .bind(template.max_tokens)
        .bind(template.temperature)
        .execute(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        Ok(template.id)
    }

    /// Get a template by ID.
    pub async fn get(&self, id: AgentId) -> Result<Option<AgentTemplate>> {
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
            Some(row) => {
                let tool_names_str: String = row.get("tool_names");
                let tool_names: Vec<String> = serde_json::from_str(&tool_names_str)
                    .unwrap_or_default();

                Ok(Some(AgentTemplate {
                    id: AgentId::new(row.get("id")),
                    display_name: row.get("display_name"),
                    description: row.get("description"),
                    version: row.get("version"),
                    provider_id: row.get::<Option<i64>, _>("provider_id").map(ProviderId::new),
                    system_prompt: row.get("system_prompt"),
                    tool_names,
                    max_tokens: row.get("max_tokens"),
                    temperature: row.get("temperature"),
                }))
            }
            None => Ok(None),
        }
    }

    /// List all templates.
    pub async fn list(&self) -> Result<Vec<AgentTemplate>> {
        let rows = sqlx::query(
            r#"
            SELECT id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature
            FROM agents ORDER BY id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        let templates = rows
            .into_iter()
            .map(|row| {
                let tool_names_str: String = row.get("tool_names");
                let tool_names: Vec<String> = serde_json::from_str(&tool_names_str)
                    .unwrap_or_default();

                AgentTemplate {
                    id: AgentId::new(row.get("id")),
                    display_name: row.get("display_name"),
                    description: row.get("description"),
                    version: row.get("version"),
                    provider_id: row.get::<Option<i64>, _>("provider_id").map(ProviderId::new),
                    system_prompt: row.get("system_prompt"),
                    tool_names,
                    max_tokens: row.get("max_tokens"),
                    temperature: row.get("temperature"),
                }
            })
            .collect();

        Ok(templates)
    }

    /// Delete a template.
    pub async fn delete(&self, id: AgentId) -> Result<()> {
        sqlx::query("DELETE FROM agents WHERE id = ?")
            .bind(id.inner())
            .execute(&self.pool)
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        Ok(())
    }
}
