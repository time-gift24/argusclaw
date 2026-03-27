use std::sync::Arc;

use argus_protocol::{AgentId, AgentRecord, ArgusError, Result};
use argus_repository::traits::AgentRepository;
use argus_repository::ArgusSqlite;

/// Manager for agent templates.
pub struct TemplateManager {
    repository: Arc<dyn AgentRepository>,
    /// Concrete instance for repair operations that need transactions.
    sqlite: Arc<ArgusSqlite>,
}

fn format_delete_blocked_reason(id: AgentId, thread_count: i64, job_count: i64) -> String {
    let mut blockers = Vec::new();

    if thread_count > 0 {
        blockers.push(format!("{} 个会话线程", thread_count));
    }
    if job_count > 0 {
        blockers.push(format!("{} 个任务", job_count));
    }

    format!(
        "无法删除智能体 {}：当前仍被 {} 引用，请先删除相关会话或任务。",
        id.inner(),
        blockers.join("、")
    )
}

impl TemplateManager {
    /// Seed builtin agents from embedded TOML definitions at runtime.
    pub async fn seed_builtin_agents(&self) -> Result<()> {
        use crate::config::TomlAgentDef;
        use crate::generated_agents::get_builtin_agent_definitions;

        tracing::info!("seeding builtin agents from embedded TOML data");

        let agent_definitions = get_builtin_agent_definitions();
        let mut seeded_count = 0;

        for toml_str in agent_definitions {
            let def: TomlAgentDef =
                toml::from_str(toml_str).map_err(|e| ArgusError::DatabaseError {
                    reason: format!("failed to parse embedded TOML: {}", e),
                })?;

            let record = def.to_agent_record();
            let agent_id = self
                .upsert_by_display_name(&record)
                .await
                .map_err(|e| ArgusError::DatabaseError {
                    reason: format!("failed to seed agent '{}': {}", record.display_name, e),
                })?;

            tracing::info!(
                "seeded builtin agent '{}' (id={})",
                record.display_name,
                agent_id.inner()
            );
            seeded_count += 1;
        }

        tracing::info!("successfully seeded {} builtin agent(s)", seeded_count);
        Ok(())
    }

    pub fn new(repository: Arc<dyn AgentRepository>, sqlite: Arc<ArgusSqlite>) -> Self {
        Self { repository, sqlite }
    }

    /// Upsert (create or update) an agent template.
    pub async fn upsert(&self, template: AgentRecord) -> Result<AgentId> {
        let id = self
            .repository
            .upsert(&template)
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;
        Ok(id)
    }

    /// Upsert an agent by display_name (insert or update if exists).
    pub async fn upsert_by_display_name(&self, record: &AgentRecord) -> Result<AgentId> {
        let id = self
            .repository
            .upsert(record)
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;
        Ok(id)
    }

    /// Repair legacy placeholder agent IDs that were incorrectly persisted as `0`.
    pub async fn repair_placeholder_ids(&self) -> Result<()> {
        self.sqlite
            .repair_placeholder_ids()
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })
    }

    /// Get a template by ID.
    pub async fn get(&self, id: AgentId) -> Result<Option<AgentRecord>> {
        self.repository
            .get(&id)
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })
    }

    /// List all templates.
    pub async fn list(&self) -> Result<Vec<AgentRecord>> {
        self.repository
            .list()
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })
    }

    /// Find a template by display name.
    pub async fn find_by_display_name(&self, display_name: &str) -> Result<Option<AgentRecord>> {
        self.repository
            .find_by_display_name(display_name)
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })
    }

    /// Delete a template.
    pub async fn delete(&self, id: AgentId) -> Result<()> {
        let (thread_count, job_count) = self
            .repository
            .count_references(&id)
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        if thread_count > 0 || job_count > 0 {
            return Err(ArgusError::DatabaseError {
                reason: format_delete_blocked_reason(id, thread_count, job_count),
            });
        }

        self.repository
            .delete(&id)
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        Ok(())
    }

    /// List all subagents of a given parent agent.
    pub async fn list_subagents(&self, parent_id: AgentId) -> Result<Vec<AgentRecord>> {
        self.repository
            .list_by_parent_id(&parent_id)
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })
    }

    /// Add a subagent to a parent agent.
    pub async fn add_subagent(&self, parent_id: AgentId, child_id: AgentId) -> Result<()> {
        self.repository
            .add_subagent(&parent_id, &child_id)
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })
    }

    /// Remove a subagent from its parent.
    pub async fn remove_subagent(&self, parent_id: AgentId, child_id: AgentId) -> Result<()> {
        self.repository
            .remove_subagent(&parent_id, &child_id)
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })
    }
}
