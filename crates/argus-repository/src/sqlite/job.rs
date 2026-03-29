//! JobRepository implementation for SQLite.

use async_trait::async_trait;

use crate::error::DbError;
use crate::traits::JobRepository;
use crate::types::{AgentId, JobId, JobRecord, JobResult, JobType, WorkflowId, WorkflowStatus};
use argus_protocol::ThreadId;

use super::{ArgusSqlite, DbResult};

#[async_trait]
impl JobRepository for ArgusSqlite {
    async fn create(&self, job: &JobRecord) -> DbResult<()> {
        let workflow_group_id = if job.job_type == JobType::Workflow {
            let group_id = job
                .group_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| DbError::QueryFailed {
                    reason: "workflow jobs require a non-empty group_id".to_string(),
                })?;
            let node_key = job
                .node_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| DbError::QueryFailed {
                    reason: "workflow jobs require a non-empty node_key".to_string(),
                })?;

            let workflow_exists = sqlx::query_scalar::<_, i64>(
                "SELECT EXISTS(SELECT 1 FROM workflows WHERE id = ?1)",
            )
            .bind(group_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

            if workflow_exists == 0 {
                return Err(DbError::QueryFailed {
                    reason: format!(
                        "workflow jobs require an existing workflow execution: {}",
                        group_id
                    ),
                });
            }

            Some((group_id.to_string(), node_key.to_string()))
        } else {
            None
        };

        if job.job_type == JobType::Workflow {
        } else if job.node_key.is_some() {
            return Err(DbError::QueryFailed {
                reason: "node_key is only valid for workflow jobs".to_string(),
            });
        }

        let group_id = workflow_group_id
            .as_ref()
            .map(|(group_id, _)| group_id.clone())
            .or_else(|| job.group_id.clone());
        let node_key = workflow_group_id
            .as_ref()
            .map(|(_, node_key)| node_key.clone())
            .or_else(|| job.node_key.clone());

        let depends_on_json = serde_json::to_string(
            &job.depends_on
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".to_string());

        sqlx::query(
            "INSERT INTO jobs (id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, node_key, depends_on, cron_expr, scheduled_at, started_at, finished_at, parent_job_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        )
        .bind(job.id.to_string())
        .bind(job.job_type.as_str())
        .bind(&job.name)
        .bind(job.status.as_str())
        .bind(job.agent_id.into_inner())
        .bind(&job.context)
        .bind(&job.prompt)
        .bind(job.thread_id.map(|t| t.to_string()))
        .bind(&group_id)
        .bind(&node_key)
        .bind(&depends_on_json)
        .bind(&job.cron_expr)
        .bind(&job.scheduled_at)
        .bind(&job.started_at)
        .bind(&job.finished_at)
        .bind(job.parent_job_id.as_ref().map(|id| id.to_string()))
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(())
    }

    async fn get(&self, id: &JobId) -> DbResult<Option<JobRecord>> {
        let row = sqlx::query(
            "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, node_key, depends_on, cron_expr, scheduled_at, started_at, finished_at, parent_job_id, result
             FROM jobs WHERE id = ?1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_job_record(r)).transpose()
    }

    async fn update_status(
        &self,
        id: &JobId,
        status: WorkflowStatus,
        started_at: Option<&str>,
        finished_at: Option<&str>,
    ) -> DbResult<()> {
        let result = sqlx::query(
            "UPDATE jobs SET status = ?1, started_at = ?2, finished_at = ?3, updated_at = datetime('now')
             WHERE id = ?4 AND status NOT IN ('succeeded', 'failed', 'cancelled')",
        )
        .bind(status.as_str())
        .bind(started_at)
        .bind(finished_at)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        if result.rows_affected() == 0 {
            return Err(DbError::QueryFailed {
                reason: format!("job {} not found or in terminal state", id),
            });
        }

        Ok(())
    }

    async fn update_result(&self, id: &JobId, result: &JobResult) -> DbResult<()> {
        let result_json = serde_json::to_string(result).map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        sqlx::query("UPDATE jobs SET result = ?1, updated_at = datetime('now') WHERE id = ?2")
            .bind(&result_json)
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(())
    }

    async fn update_thread_id(&self, id: &JobId, thread_id: &ThreadId) -> DbResult<()> {
        sqlx::query("UPDATE jobs SET thread_id = ?1, updated_at = datetime('now') WHERE id = ?2")
            .bind(thread_id.to_string())
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(())
    }

    async fn find_ready_jobs(&self, limit: usize) -> DbResult<Vec<JobRecord>> {
        let rows = sqlx::query(
            "SELECT j.id, j.job_type, j.name, j.status, j.agent_id, j.context, j.prompt, j.thread_id, j.group_id, j.node_key, j.depends_on, j.cron_expr, j.scheduled_at, j.started_at, j.finished_at, j.parent_job_id, j.result
             FROM jobs j
             WHERE j.status = 'pending' AND j.job_type != 'cron'
               AND (
                   j.job_type != 'workflow'
                   OR EXISTS (SELECT 1 FROM workflows w WHERE w.id = j.group_id)
               )
               AND NOT EXISTS (
                   SELECT 1 FROM jobs dep
                   WHERE dep.id IN (SELECT value FROM json_each(j.depends_on))
                     AND dep.status != 'succeeded'
               )
             ORDER BY j.created_at ASC LIMIT ?1",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_job_record(r)).collect()
    }

    async fn find_due_cron_jobs(&self, now: &str) -> DbResult<Vec<JobRecord>> {
        let rows = sqlx::query(
            "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, node_key, depends_on, cron_expr, scheduled_at, started_at, finished_at, parent_job_id, result
             FROM jobs
             WHERE job_type = 'cron' AND scheduled_at IS NOT NULL AND scheduled_at <= ?1
             ORDER BY scheduled_at ASC",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_job_record(r)).collect()
    }

    async fn update_scheduled_at(&self, id: &JobId, next: &str) -> DbResult<()> {
        sqlx::query(
            "UPDATE jobs SET scheduled_at = ?1, updated_at = datetime('now') WHERE id = ?2",
        )
        .bind(next)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn list_by_group(&self, group_id: &str) -> DbResult<Vec<JobRecord>> {
        let rows = sqlx::query(
            "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, node_key, depends_on, cron_expr, scheduled_at, started_at, finished_at, parent_job_id, result
             FROM jobs WHERE group_id = ?1 ORDER BY created_at ASC",
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_job_record(r)).collect()
    }

    async fn delete(&self, id: &JobId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM jobs WHERE id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }
}

impl ArgusSqlite {
    fn map_job_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<JobRecord> {
        let depends_on: Vec<JobId> =
            serde_json::from_str::<Vec<String>>(&Self::get_column::<String>(&row, "depends_on")?)
                .map(|ids| ids.into_iter().map(JobId::new).collect())
                .unwrap_or_default();
        let thread_id: Option<ThreadId> = Self::get_column::<Option<String>>(&row, "thread_id")?
            .and_then(|s| ThreadId::parse(&s).ok());
        let parent_job_id: Option<WorkflowId> =
            Self::get_column::<Option<String>>(&row, "parent_job_id")?.map(|s| WorkflowId::new(&s));
        let result: Option<JobResult> = Self::get_column::<Option<String>>(&row, "result")?
            .and_then(|s| serde_json::from_str(&s).ok());

        Ok(JobRecord {
            id: JobId::new(&Self::get_column::<String>(&row, "id")?),
            job_type: JobType::parse_str(&Self::get_column::<String>(&row, "job_type")?)
                .map_err(|e| DbError::QueryFailed { reason: e })?,
            name: Self::get_column(&row, "name")?,
            status: WorkflowStatus::parse_str(&Self::get_column::<String>(&row, "status")?)
                .map_err(|e| DbError::QueryFailed { reason: e })?,
            agent_id: AgentId::new(Self::get_column(&row, "agent_id")?),
            context: Self::get_column(&row, "context")?,
            prompt: Self::get_column(&row, "prompt")?,
            thread_id,
            group_id: Self::get_column(&row, "group_id")?,
            node_key: Self::get_column(&row, "node_key")?,
            depends_on,
            cron_expr: Self::get_column(&row, "cron_expr")?,
            scheduled_at: Self::get_column(&row, "scheduled_at")?,
            started_at: Self::get_column(&row, "started_at")?,
            finished_at: Self::get_column(&row, "finished_at")?,
            parent_job_id,
            result,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::traits::{JobRepository, WorkflowRepository};
    use crate::types::WorkflowRecord;
    use uuid::Uuid;

    async fn test_repo() -> ArgusSqlite {
        let db_path =
            std::env::temp_dir().join(format!("argus-repository-job-{}.sqlite", Uuid::new_v4()));
        let pool = crate::connect_path(&db_path)
            .await
            .expect("create sqlite pool");
        crate::migrate(&pool).await.expect("run migrations");
        ArgusSqlite::new(pool)
    }

    async fn seed_test_agent(repo: &ArgusSqlite) -> AgentId {
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

        AgentId::new(7)
    }

    async fn seed_workflow_execution(repo: &ArgusSqlite, id: &WorkflowId) {
        repo.create_workflow_execution(&WorkflowRecord {
            id: id.clone(),
            name: "Workflow".to_string(),
            status: WorkflowStatus::Pending,
            template_id: None,
            template_version: None,
            initiating_thread_id: None,
        })
        .await
        .expect("seed workflow execution");
    }

    #[tokio::test]
    async fn workflow_job_round_trips_node_key() {
        let repo = test_repo().await;
        let agent_id = seed_test_agent(&repo).await;
        let workflow_id = WorkflowId::new("wf-1");
        let job_id = JobId::new("job-1");
        seed_workflow_execution(&repo, &workflow_id).await;

        repo.create(&JobRecord {
            id: job_id.clone(),
            job_type: JobType::Workflow,
            name: "Collect".to_string(),
            status: WorkflowStatus::Pending,
            agent_id,
            context: None,
            prompt: "Collect context".to_string(),
            thread_id: None,
            group_id: Some(workflow_id.to_string()),
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

        let job = repo.get(&job_id).await.unwrap().unwrap();
        assert_eq!(job.node_key, Some("collect".to_string()));
    }

    #[tokio::test]
    async fn workflow_job_requires_group_and_node_key() {
        let repo = test_repo().await;
        let agent_id = seed_test_agent(&repo).await;

        let err = repo
            .create(&JobRecord {
                id: JobId::new("job-2"),
                job_type: JobType::Workflow,
                name: "Broken".to_string(),
                status: WorkflowStatus::Pending,
                agent_id,
                context: None,
                prompt: "Broken".to_string(),
                thread_id: None,
                group_id: None,
                node_key: None,
                depends_on: vec![],
                cron_expr: None,
                scheduled_at: None,
                started_at: None,
                finished_at: None,
                parent_job_id: None,
                result: None,
            })
            .await
            .unwrap_err();

        assert!(err.to_string().contains("workflow jobs require"));
    }

    #[tokio::test]
    async fn workflow_job_requires_existing_execution() {
        let repo = test_repo().await;
        let agent_id = seed_test_agent(&repo).await;

        let err = repo
            .create(&JobRecord {
                id: JobId::new("job-3"),
                job_type: JobType::Workflow,
                name: "Broken".to_string(),
                status: WorkflowStatus::Pending,
                agent_id,
                context: None,
                prompt: "Broken".to_string(),
                thread_id: None,
                group_id: Some("wf-missing".to_string()),
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
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("workflow jobs require an existing workflow execution"));
    }

    #[tokio::test]
    async fn find_ready_jobs_skips_orphaned_workflow_jobs() {
        let repo = test_repo().await;
        let agent_id = seed_test_agent(&repo).await;

        sqlx::query(
            "INSERT INTO jobs (id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, node_key, depends_on, cron_expr, scheduled_at, started_at, finished_at, parent_job_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        )
        .bind("job-orphan")
        .bind(JobType::Workflow.as_str())
        .bind("Orphan")
        .bind(WorkflowStatus::Pending.as_str())
        .bind(agent_id.into_inner())
        .bind(Option::<String>::None)
        .bind("Collect context")
        .bind(Option::<String>::None)
        .bind("wf-missing")
        .bind("collect")
        .bind("[]")
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .execute(repo.pool())
        .await
        .unwrap();

        let ready_jobs = repo.find_ready_jobs(10).await.unwrap();
        assert!(ready_jobs.is_empty());
    }
}
