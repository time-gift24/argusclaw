//! WorkflowRepository implementation for SQLite.

use argus_protocol::ThreadId;
use async_trait::async_trait;

use crate::error::DbError;
use crate::traits::WorkflowRepository;
use crate::types::{
    WorkflowId, WorkflowProgressRecord, WorkflowRecord, WorkflowStatus, WorkflowTemplateId,
    WorkflowTemplateNodeRecord, WorkflowTemplateRecord,
};

use super::{ArgusSqlite, DbResult};

#[async_trait]
impl WorkflowRepository for ArgusSqlite {
    async fn create_workflow_template(&self, template: &WorkflowTemplateRecord) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO workflow_templates (id, version, name, description)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(template.id.as_ref())
        .bind(template.version)
        .bind(&template.name)
        .bind(&template.description)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn get_workflow_template(
        &self,
        id: &WorkflowTemplateId,
        version: i64,
    ) -> DbResult<Option<WorkflowTemplateRecord>> {
        let row = sqlx::query(
            "SELECT id, version, name, description
             FROM workflow_templates
             WHERE id = ?1 AND version = ?2",
        )
        .bind(id.as_ref())
        .bind(version)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| self.map_workflow_template_record(row))
            .transpose()
    }

    async fn list_workflow_templates(&self) -> DbResult<Vec<WorkflowTemplateRecord>> {
        let rows = sqlx::query(
            "SELECT id, version, name, description
             FROM workflow_templates
             ORDER BY id ASC, version ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter()
            .map(|row| self.map_workflow_template_record(row))
            .collect()
    }

    async fn update_workflow_template(&self, template: &WorkflowTemplateRecord) -> DbResult<()> {
        let result = sqlx::query(
            "UPDATE workflow_templates
             SET name = ?3, description = ?4, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1 AND version = ?2",
        )
        .bind(template.id.as_ref())
        .bind(template.version)
        .bind(&template.name)
        .bind(&template.description)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        if result.rows_affected() == 0 {
            return Err(DbError::QueryFailed {
                reason: format!(
                    "workflow template not found: {}@{}",
                    template.id, template.version
                ),
            });
        }

        Ok(())
    }

    async fn delete_workflow_template(
        &self,
        id: &WorkflowTemplateId,
        version: i64,
    ) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM workflow_templates WHERE id = ?1 AND version = ?2")
            .bind(id.as_ref())
            .bind(version)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }

    async fn create_workflow_template_node(
        &self,
        node: &WorkflowTemplateNodeRecord,
    ) -> DbResult<()> {
        let depends_on_keys =
            serde_json::to_string(&node.depends_on_keys).map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        sqlx::query(
            "INSERT INTO workflow_template_nodes (
                template_id, template_version, node_key, name, agent_id, prompt, context, depends_on_keys
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(node.template_id.as_ref())
        .bind(node.template_version)
        .bind(&node.node_key)
        .bind(&node.name)
        .bind(node.agent_id.into_inner())
        .bind(&node.prompt)
        .bind(&node.context)
        .bind(&depends_on_keys)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn get_workflow_template_node(
        &self,
        template_id: &WorkflowTemplateId,
        version: i64,
        node_key: &str,
    ) -> DbResult<Option<WorkflowTemplateNodeRecord>> {
        let row = sqlx::query(
            "SELECT template_id, template_version, node_key, name, agent_id, prompt, context, depends_on_keys
             FROM workflow_template_nodes
             WHERE template_id = ?1 AND template_version = ?2 AND node_key = ?3",
        )
        .bind(template_id.as_ref())
        .bind(version)
        .bind(node_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| self.map_workflow_template_node_record(row))
            .transpose()
    }

    async fn list_workflow_template_nodes(
        &self,
        template_id: &WorkflowTemplateId,
        version: i64,
    ) -> DbResult<Vec<WorkflowTemplateNodeRecord>> {
        let rows = sqlx::query(
            "SELECT template_id, template_version, node_key, name, agent_id, prompt, context, depends_on_keys
             FROM workflow_template_nodes
             WHERE template_id = ?1 AND template_version = ?2
             ORDER BY node_key ASC",
        )
        .bind(template_id.as_ref())
        .bind(version)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter()
            .map(|row| self.map_workflow_template_node_record(row))
            .collect()
    }

    async fn create_workflow_execution(&self, workflow: &WorkflowRecord) -> DbResult<()> {
        let template_id = workflow
            .template_id
            .as_ref()
            .map(|id| id.as_ref().to_string());
        let initiating_thread_id = workflow
            .initiating_thread_id
            .as_ref()
            .map(|thread_id| thread_id.to_string());

        sqlx::query(
            "INSERT INTO workflows (
                id, name, status, template_id, template_version, initiating_thread_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(workflow.id.as_ref())
        .bind(&workflow.name)
        .bind(workflow.status.as_str())
        .bind(&template_id)
        .bind(workflow.template_version)
        .bind(&initiating_thread_id)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn get_workflow_execution(&self, id: &WorkflowId) -> DbResult<Option<WorkflowRecord>> {
        let row = sqlx::query(
            "SELECT id, name, status, template_id, template_version, initiating_thread_id
             FROM workflows
             WHERE id = ?1",
        )
        .bind(id.as_ref())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| self.map_workflow_record(row)).transpose()
    }

    async fn update_workflow_execution_status(
        &self,
        id: &WorkflowId,
        status: WorkflowStatus,
    ) -> DbResult<()> {
        let result = sqlx::query(
            "UPDATE workflows
             SET status = ?1, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?2",
        )
        .bind(status.as_str())
        .bind(id.as_ref())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        if result.rows_affected() == 0 {
            return Err(DbError::QueryFailed {
                reason: format!("workflow not found: {}", id),
            });
        }

        Ok(())
    }

    async fn list_workflow_executions(&self) -> DbResult<Vec<WorkflowRecord>> {
        let rows = sqlx::query(
            "SELECT id, name, status, template_id, template_version, initiating_thread_id
             FROM workflows
             ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter()
            .map(|row| self.map_workflow_record(row))
            .collect()
    }

    async fn list_workflow_executions_by_initiating_thread(
        &self,
        thread_id: &ThreadId,
    ) -> DbResult<Vec<WorkflowRecord>> {
        let rows = sqlx::query(
            "SELECT id, name, status, template_id, template_version, initiating_thread_id
             FROM workflows
             WHERE initiating_thread_id = ?1
             ORDER BY created_at ASC",
        )
        .bind(thread_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter()
            .map(|row| self.map_workflow_record(row))
            .collect()
    }

    async fn delete_workflow_execution(&self, id: &WorkflowId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM workflows WHERE id = ?1")
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }

    async fn get_workflow_progress(
        &self,
        id: &WorkflowId,
    ) -> DbResult<Option<WorkflowProgressRecord>> {
        let row = sqlx::query(
            "SELECT
                w.id AS workflow_id,
                w.template_id,
                w.template_version,
                w.status,
                COUNT(j.id) AS total_jobs,
                COALESCE(SUM(CASE WHEN j.status = 'pending' THEN 1 ELSE 0 END), 0) AS pending_jobs,
                COALESCE(SUM(CASE WHEN j.status = 'running' THEN 1 ELSE 0 END), 0) AS running_jobs,
                COALESCE(SUM(CASE WHEN j.status = 'succeeded' THEN 1 ELSE 0 END), 0) AS succeeded_jobs,
                COALESCE(SUM(CASE WHEN j.status = 'failed' THEN 1 ELSE 0 END), 0) AS failed_jobs,
                COALESCE(SUM(CASE WHEN j.status = 'cancelled' THEN 1 ELSE 0 END), 0) AS cancelled_jobs
             FROM workflows w
             LEFT JOIN jobs j
                ON j.group_id = w.id
               AND j.job_type = 'workflow'
             WHERE w.id = ?1
             GROUP BY w.id, w.template_id, w.template_version, w.status",
        )
        .bind(id.as_ref())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| self.map_workflow_progress_record(row))
            .transpose()
    }
}

impl ArgusSqlite {
    fn map_workflow_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<WorkflowRecord> {
        let template_id =
            Self::get_column::<Option<String>>(&row, "template_id")?.map(WorkflowTemplateId::new);
        let template_version = Self::get_column::<Option<i64>>(&row, "template_version")?;
        let initiating_thread_id =
            Self::get_column::<Option<String>>(&row, "initiating_thread_id")?
                .and_then(|value| ThreadId::parse(&value).ok());

        Ok(WorkflowRecord {
            id: WorkflowId::new(Self::get_column::<String>(&row, "id")?),
            name: Self::get_column(&row, "name")?,
            status: WorkflowStatus::parse_str(&Self::get_column::<String>(&row, "status")?)
                .map_err(|e| DbError::QueryFailed { reason: e })?,
            template_id,
            template_version,
            initiating_thread_id,
        })
    }

    fn map_workflow_template_record(
        &self,
        row: sqlx::sqlite::SqliteRow,
    ) -> DbResult<WorkflowTemplateRecord> {
        Ok(WorkflowTemplateRecord {
            id: WorkflowTemplateId::new(Self::get_column::<String>(&row, "id")?),
            version: Self::get_column(&row, "version")?,
            name: Self::get_column(&row, "name")?,
            description: Self::get_column(&row, "description")?,
        })
    }

    fn map_workflow_template_node_record(
        &self,
        row: sqlx::sqlite::SqliteRow,
    ) -> DbResult<WorkflowTemplateNodeRecord> {
        let depends_on_keys: Vec<String> =
            serde_json::from_str(&Self::get_column::<String>(&row, "depends_on_keys")?).map_err(
                |e| DbError::QueryFailed {
                    reason: e.to_string(),
                },
            )?;

        Ok(WorkflowTemplateNodeRecord {
            template_id: WorkflowTemplateId::new(Self::get_column::<String>(&row, "template_id")?),
            template_version: Self::get_column(&row, "template_version")?,
            node_key: Self::get_column(&row, "node_key")?,
            name: Self::get_column(&row, "name")?,
            agent_id: argus_protocol::AgentId::new(Self::get_column(&row, "agent_id")?),
            prompt: Self::get_column(&row, "prompt")?,
            context: Self::get_column(&row, "context")?,
            depends_on_keys,
        })
    }

    fn map_workflow_progress_record(
        &self,
        row: sqlx::sqlite::SqliteRow,
    ) -> DbResult<WorkflowProgressRecord> {
        let template_id =
            Self::get_column::<Option<String>>(&row, "template_id")?.map(WorkflowTemplateId::new);

        Ok(WorkflowProgressRecord {
            workflow_id: WorkflowId::new(Self::get_column::<String>(&row, "workflow_id")?),
            template_id,
            template_version: Self::get_column(&row, "template_version")?,
            status: WorkflowStatus::parse_str(&Self::get_column::<String>(&row, "status")?)
                .map_err(|e| DbError::QueryFailed { reason: e })?,
            total_jobs: Self::get_column(&row, "total_jobs")?,
            pending_jobs: Self::get_column(&row, "pending_jobs")?,
            running_jobs: Self::get_column(&row, "running_jobs")?,
            succeeded_jobs: Self::get_column(&row, "succeeded_jobs")?,
            failed_jobs: Self::get_column(&row, "failed_jobs")?,
            cancelled_jobs: Self::get_column(&row, "cancelled_jobs")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::traits::{JobRepository, WorkflowRepository};
    use crate::types::{
        JobId, JobRecord, JobType, WorkflowRecord, WorkflowTemplateNodeRecord,
        WorkflowTemplateRecord,
    };
    use uuid::Uuid;

    async fn test_repo() -> ArgusSqlite {
        let db_path =
            std::env::temp_dir().join(format!("argus-repository-{}.sqlite", Uuid::new_v4()));
        let pool = crate::connect_path(&db_path)
            .await
            .expect("create sqlite pool");
        crate::migrate(&pool).await.expect("run migrations");
        ArgusSqlite::new(pool)
    }

    async fn seed_test_agent(repo: &ArgusSqlite) -> argus_protocol::AgentId {
        let provider_id: i64 =
            sqlx::query_scalar("SELECT id FROM llm_providers ORDER BY id LIMIT 1")
                .fetch_one(repo.pool())
                .await
                .expect("default provider");

        sqlx::query(
            "INSERT INTO agents (id, display_name, description, version, provider_id, model_id, system_prompt, tool_names, max_tokens, temperature, thinking_config)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )
        .bind(7_i64)
        .bind("Workflow Test Agent")
        .bind("Test agent")
        .bind("1.0.0")
        .bind(provider_id)
        .bind(Option::<String>::None)
        .bind("You are a test agent.")
        .bind("[]")
        .bind(Option::<i64>::None)
        .bind(Option::<i64>::None)
        .bind(r#"{"type":"disabled","clear_thinking":false}"#)
        .execute(repo.pool())
        .await
        .expect("seed agent");

        argus_protocol::AgentId::new(7)
    }

    #[tokio::test]
    async fn list_workflow_execution_progress_counts_grouped_jobs() {
        let repo = test_repo().await;
        let agent_id = seed_test_agent(&repo).await;
        let template_id = WorkflowTemplateId::new("tpl-1");
        let execution_id = WorkflowId::new("wf-1");

        repo.create_workflow_template(&WorkflowTemplateRecord {
            id: template_id.clone(),
            name: "demo".to_string(),
            version: 1,
            description: "demo template".to_string(),
        })
        .await
        .unwrap();

        repo.update_workflow_template(&WorkflowTemplateRecord {
            id: template_id.clone(),
            name: "demo v2".to_string(),
            version: 1,
            description: "updated demo template".to_string(),
        })
        .await
        .unwrap();

        repo.create_workflow_template_node(&WorkflowTemplateNodeRecord {
            template_id: template_id.clone(),
            template_version: 1,
            node_key: "collect".to_string(),
            name: "Collect".to_string(),
            agent_id,
            prompt: "Collect context".to_string(),
            context: None,
            depends_on_keys: vec![],
        })
        .await
        .unwrap();

        repo.create_workflow_template_node(&WorkflowTemplateNodeRecord {
            template_id: template_id.clone(),
            template_version: 1,
            node_key: "summarize".to_string(),
            name: "Summarize".to_string(),
            agent_id,
            prompt: "Summarize context".to_string(),
            context: None,
            depends_on_keys: vec!["collect".to_string()],
        })
        .await
        .unwrap();

        repo.create_workflow_execution(&WorkflowRecord {
            id: execution_id.clone(),
            name: "demo".to_string(),
            status: WorkflowStatus::Pending,
            template_id: Some(template_id.clone()),
            template_version: Some(1),
            initiating_thread_id: None,
        })
        .await
        .unwrap();

        let header = repo.get_workflow_execution(&execution_id).await.unwrap();
        assert!(header.is_some());
        let header = header.unwrap();
        assert_eq!(header.template_id, Some(template_id.clone()));
        assert_eq!(header.template_version, Some(1));

        let template = repo
            .get_workflow_template(&template_id, 1)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(template.name, "demo v2");
        assert_eq!(template.description, "updated demo template");

        repo.create(&JobRecord {
            id: JobId::new("job-1"),
            job_type: JobType::Workflow,
            name: "Collect".to_string(),
            status: WorkflowStatus::Pending,
            agent_id,
            context: None,
            prompt: "Collect context".to_string(),
            thread_id: None,
            group_id: Some(execution_id.to_string()),
            node_key: Some("collect".to_string()),
            depends_on: vec![],
            cron_expr: None,
            scheduled_at: None,
            started_at: None,
            finished_at: None,
            parent_job_id: None,
            result: None,
        })
        .await
        .unwrap();

        repo.create(&JobRecord {
            id: JobId::new("job-2"),
            job_type: JobType::Workflow,
            name: "Summarize".to_string(),
            status: WorkflowStatus::Running,
            agent_id,
            context: None,
            prompt: "Summarize context".to_string(),
            thread_id: None,
            group_id: Some(execution_id.to_string()),
            node_key: Some("summarize".to_string()),
            depends_on: vec![JobId::new("job-1")],
            cron_expr: None,
            scheduled_at: None,
            started_at: Some("2026-03-29T00:00:00Z".to_string()),
            finished_at: None,
            parent_job_id: None,
            result: None,
        })
        .await
        .unwrap();

        let progress = repo.get_workflow_progress(&execution_id).await.unwrap();
        assert!(progress.is_some());
        let progress = progress.unwrap();
        assert_eq!(progress.total_jobs, 2);
        assert_eq!(progress.pending_jobs, 1);
        assert_eq!(progress.running_jobs, 1);
    }
}
