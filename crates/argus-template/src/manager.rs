use std::sync::Arc;

use argus_protocol::{AgentId, AgentRecord, ArgusError, Result};
use argus_repository::traits::{AgentRepository, TemplateRepairRepository};
use argus_repository::types::AgentDeleteReport;

/// Manager for agent templates.
pub struct TemplateManager {
    repository: Arc<dyn AgentRepository>,
    repair_repository: Arc<dyn TemplateRepairRepository>,
}

/// Options for deleting an agent template.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TemplateDeleteOptions {
    pub cascade_associations: bool,
}

fn format_delete_blocked_reason(
    id: AgentId,
    thread_count: i64,
    job_count: i64,
    run_count: i64,
    subagent_ref_count: usize,
) -> String {
    let mut blockers = Vec::new();

    if thread_count > 0 {
        blockers.push(format!("{} 个会话线程", thread_count));
    }
    if job_count > 0 {
        blockers.push(format!("{} 个任务", job_count));
    }
    if run_count > 0 {
        blockers.push(format!("{} 条运行记录", run_count));
    }
    if subagent_ref_count > 0 {
        blockers.push(format!(
            "{} 个智能体的 subagent_names 配置",
            subagent_ref_count
        ));
    }

    format!(
        "无法删除智能体 {}：当前仍被 {} 引用，请先移除相关会话、任务或调度配置。",
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
            seeded_count += 1;
        }

        tracing::info!("successfully seeded {} builtin agent(s)", seeded_count);
        Ok(())
    }

    pub fn new(
        repository: Arc<dyn AgentRepository>,
        repair_repository: Arc<dyn TemplateRepairRepository>,
    ) -> Self {
        Self {
            repository,
            repair_repository,
        }
    }

    /// Upsert (create or update) an agent template.
    pub async fn upsert(&self, template: AgentRecord) -> Result<AgentId> {
        let id =
            self.repository
                .upsert(&template)
                .await
                .map_err(|e| ArgusError::DatabaseError {
                    reason: e.to_string(),
                })?;
        Ok(id)
    }

    /// Upsert an agent by display_name (insert or update if exists).
    pub async fn upsert_by_display_name(&self, record: &AgentRecord) -> Result<AgentId> {
        let id = self
            .repository
            .upsert(record)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;
        Ok(id)
    }

    /// Repair legacy placeholder agent IDs that were incorrectly persisted as `0`.
    pub async fn repair_placeholder_ids(&self) -> Result<()> {
        self.repair_repository
            .repair_placeholder_ids()
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })
    }

    /// Get a template by ID.
    pub async fn get(&self, id: AgentId) -> Result<Option<AgentRecord>> {
        self.repository
            .get(&id)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })
    }

    /// List all templates.
    pub async fn list(&self) -> Result<Vec<AgentRecord>> {
        self.repository
            .list()
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })
    }

    /// Find a template by display name.
    pub async fn find_by_display_name(&self, display_name: &str) -> Result<Option<AgentRecord>> {
        self.repository
            .find_by_display_name(display_name)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })
    }

    /// Delete a template.
    pub async fn delete(&self, id: AgentId) -> Result<()> {
        self.delete_with_options(id, TemplateDeleteOptions::default())
            .await?;

        Ok(())
    }

    /// Delete a template, optionally removing associated jobs, threads, and empty sessions.
    pub async fn delete_with_options(
        &self,
        id: AgentId,
        options: TemplateDeleteOptions,
    ) -> Result<AgentDeleteReport> {
        let target_display_name = self.get(id).await?.map(|record| record.display_name);
        let (thread_count, job_count, run_count) = self
            .repository
            .count_references(&id)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;
        let subagent_ref_count = if let Some(display_name) = target_display_name {
            self.list()
                .await?
                .into_iter()
                .filter(|record| record.id != id)
                .filter(|record| {
                    record
                        .subagent_names
                        .iter()
                        .any(|name| name == &display_name)
                })
                .count()
        } else {
            0
        };

        if !options.cascade_associations
            && (thread_count > 0 || job_count > 0 || run_count > 0 || subagent_ref_count > 0)
        {
            return Err(ArgusError::DatabaseError {
                reason: format_delete_blocked_reason(
                    id,
                    thread_count,
                    job_count,
                    run_count,
                    subagent_ref_count,
                ),
            });
        }

        if options.cascade_associations {
            if subagent_ref_count > 0 {
                return Err(ArgusError::DatabaseError {
                    reason: format_delete_blocked_reason(
                        id,
                        thread_count,
                        job_count,
                        run_count,
                        subagent_ref_count,
                    ),
                });
            }

            return self
                .repository
                .delete_with_associations(&id)
                .await
                .map_err(|e| ArgusError::DatabaseError {
                    reason: e.to_string(),
                });
        }

        let agent_deleted =
            self.repository
                .delete(&id)
                .await
                .map_err(|e| ArgusError::DatabaseError {
                    reason: e.to_string(),
                })?;

        Ok(AgentDeleteReport::empty(agent_deleted))
    }

    /// Resolve subagent records by display name, skipping missing entries.
    pub async fn list_subagents_by_names(&self, names: &[String]) -> Result<Vec<AgentRecord>> {
        let mut results = Vec::new();
        for name in names {
            if let Some(record) = self
                .repository
                .find_by_display_name(name)
                .await
                .map_err(|e| ArgusError::DatabaseError {
                    reason: e.to_string(),
                })?
            {
                results.push(record);
            } else {
                tracing::warn!("subagent '{}' not found, skipping", name);
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use argus_repository::{connect_path, migrate, AgentRepository, ArgusSqlite};

    use super::TemplateManager;

    static NEXT_TEST_DB_ID: AtomicU64 = AtomicU64::new(0);

    async fn make_template_manager_for_test() -> TemplateManager {
        let unique = NEXT_TEST_DB_ID.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let db_path = std::env::temp_dir().join(format!(
            "argus-template-test-{}-{nanos}-{unique}.sqlite",
            std::process::id()
        ));
        let pool = connect_path(&db_path)
            .await
            .expect("test sqlite database should open");
        migrate(&pool)
            .await
            .expect("test sqlite database should migrate");

        let sqlite = Arc::new(ArgusSqlite::new(pool));
        TemplateManager::new(sqlite.clone() as Arc<dyn AgentRepository>, sqlite)
    }

    #[tokio::test]
    async fn seed_builtin_agents_allows_empty_agent_definitions() {
        let manager = make_template_manager_for_test().await;
        manager.seed_builtin_agents().await.unwrap();

        let records = manager.list().await.unwrap();
        assert!(records.is_empty());
    }

    #[tokio::test]
    async fn list_subagents_by_names_skips_missing_records() {
        let manager = make_template_manager_for_test().await;
        manager
            .upsert(argus_protocol::AgentRecord {
                id: argus_protocol::AgentId::new(0),
                display_name: "Worker".to_string(),
                description: "Does work".to_string(),
                version: "1.0.0".to_string(),
                provider_id: None,
                model_id: None,
                system_prompt: "Work.".to_string(),
                tool_names: vec![],
                subagent_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: None,
            })
            .await
            .expect("worker should upsert");

        let records = manager
            .list_subagents_by_names(&["Worker".to_string(), "Missing".to_string()])
            .await
            .expect("lookup should succeed");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].display_name, "Worker");
    }

    #[tokio::test]
    async fn delete_blocks_when_other_agents_reference_target_subagent_name() {
        let manager = make_template_manager_for_test().await;
        let worker_id = manager
            .upsert(argus_protocol::AgentRecord {
                id: argus_protocol::AgentId::new(0),
                display_name: "Worker".to_string(),
                description: "Does work".to_string(),
                version: "1.0.0".to_string(),
                provider_id: None,
                model_id: None,
                system_prompt: "Work.".to_string(),
                tool_names: vec![],
                subagent_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: None,
            })
            .await
            .expect("worker should upsert");
        manager
            .upsert(argus_protocol::AgentRecord {
                id: argus_protocol::AgentId::new(0),
                display_name: "Dispatcher".to_string(),
                description: "Routes work".to_string(),
                version: "1.0.0".to_string(),
                provider_id: None,
                model_id: None,
                system_prompt: "Dispatch.".to_string(),
                tool_names: vec![],
                subagent_names: vec!["Worker".to_string()],
                max_tokens: None,
                temperature: None,
                thinking_config: None,
            })
            .await
            .expect("dispatcher should upsert");

        let error = manager
            .delete(worker_id)
            .await
            .expect_err("delete should be blocked by subagent reference");

        let argus_protocol::ArgusError::DatabaseError { reason } = error else {
            panic!("expected database error when delete is blocked");
        };
        assert!(reason.contains("subagent_names"));
        assert!(reason.contains("调度配置"));
    }
}
