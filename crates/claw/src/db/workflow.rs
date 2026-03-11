//! SQLite implementation of the workflow repository.

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::agents::AgentId;
use crate::db::DbError;
use crate::workflow::{
    JobId, JobRecord, StageId, StageRecord, WorkflowId, WorkflowRecord, WorkflowStatus,
};

/// SQLite-backed workflow repository.
pub struct SqliteWorkflowRepository {
    pool: SqlitePool,
}

impl SqliteWorkflowRepository {
    /// Create a new SQLite workflow repository.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Get a column value from a row.
    fn get<T>(row: &sqlx::sqlite::SqliteRow, col: &str) -> Result<T, DbError>
    where
        T: for<'r> sqlx::decode::Decode<'r, sqlx::Sqlite> + sqlx::types::Type<sqlx::Sqlite>,
    {
        row.try_get(col).map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })
    }

    /// Parse a workflow status from a string.
    fn parse_status(s: String) -> Result<WorkflowStatus, DbError> {
        WorkflowStatus::parse_str(&s).map_err(|_| DbError::QueryFailed {
            reason: format!("invalid workflow status: {s}"),
        })
    }

    /// Map a database row to a WorkflowRecord.
    fn map_workflow(row: sqlx::sqlite::SqliteRow) -> Result<WorkflowRecord, DbError> {
        Ok(WorkflowRecord {
            id: WorkflowId::new(Self::get::<String>(&row, "id")?),
            name: Self::get(&row, "name")?,
            status: Self::parse_status(Self::get(&row, "status")?)?,
        })
    }

    /// Map a database row to a StageRecord.
    fn map_stage(row: sqlx::sqlite::SqliteRow) -> Result<StageRecord, DbError> {
        Ok(StageRecord {
            id: StageId::new(Self::get::<String>(&row, "id")?),
            workflow_id: WorkflowId::new(Self::get::<String>(&row, "workflow_id")?),
            name: Self::get(&row, "name")?,
            sequence: Self::get(&row, "sequence")?,
            status: Self::parse_status(Self::get(&row, "status")?)?,
        })
    }

    /// Map a database row to a JobRecord.
    fn map_job(row: sqlx::sqlite::SqliteRow) -> Result<JobRecord, DbError> {
        Ok(JobRecord {
            id: JobId::new(Self::get::<String>(&row, "id")?),
            stage_id: StageId::new(Self::get::<String>(&row, "stage_id")?),
            agent_id: AgentId::new(Self::get::<String>(&row, "agent_id")?),
            name: Self::get(&row, "name")?,
            status: Self::parse_status(Self::get(&row, "status")?)?,
            started_at: Self::get::<Option<String>>(&row, "started_at")?,
            finished_at: Self::get::<Option<String>>(&row, "finished_at")?,
        })
    }

    /// Convert a WorkflowStatus to its string representation.
    fn status_as_str(status: WorkflowStatus) -> &'static str {
        status.as_str()
    }
}

#[async_trait]
impl crate::workflow::WorkflowRepository for SqliteWorkflowRepository {
    // Workflow CRUD
    async fn create_workflow(&self, workflow: &WorkflowRecord) -> Result<(), DbError> {
        sqlx::query("INSERT INTO workflows (id, name, status) VALUES (?1, ?2, ?3)")
            .bind(workflow.id.as_ref())
            .bind(&workflow.name)
            .bind(Self::status_as_str(workflow.status))
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(())
    }

    async fn get_workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowRecord>, DbError> {
        let row = sqlx::query("SELECT id, name, status FROM workflows WHERE id = ?1")
            .bind(id.as_ref())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        row.map(Self::map_workflow).transpose()
    }

    async fn update_workflow_status(
        &self,
        id: &WorkflowId,
        status: WorkflowStatus,
    ) -> Result<(), DbError> {
        let result = sqlx::query("UPDATE workflows SET status = ?1 WHERE id = ?2")
            .bind(Self::status_as_str(status))
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

    async fn list_workflows(&self) -> Result<Vec<WorkflowRecord>, DbError> {
        let rows = sqlx::query("SELECT id, name, status FROM workflows ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        rows.into_iter().map(Self::map_workflow).collect()
    }

    async fn delete_workflow(&self, id: &WorkflowId) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM workflows WHERE id = ?1")
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }

    // Stage CRUD
    async fn create_stage(&self, stage: &StageRecord) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO stages (id, workflow_id, name, sequence, status) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(stage.id.as_ref())
        .bind(stage.workflow_id.as_ref())
        .bind(&stage.name)
        .bind(stage.sequence)
        .bind(Self::status_as_str(stage.status))
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn list_stages_by_workflow(
        &self,
        workflow_id: &WorkflowId,
    ) -> Result<Vec<StageRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT id, workflow_id, name, sequence, status FROM stages WHERE workflow_id = ?1 ORDER BY sequence",
        )
        .bind(workflow_id.as_ref())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter().map(Self::map_stage).collect()
    }

    async fn update_stage_status(
        &self,
        id: &StageId,
        status: WorkflowStatus,
    ) -> Result<(), DbError> {
        sqlx::query("UPDATE stages SET status = ?1 WHERE id = ?2")
            .bind(Self::status_as_str(status))
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(())
    }

    // Job CRUD
    async fn create_job(&self, job: &JobRecord) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO jobs (id, stage_id, agent_id, name, status, started_at, finished_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(job.id.as_ref())
        .bind(job.stage_id.as_ref())
        .bind(job.agent_id.as_ref())
        .bind(&job.name)
        .bind(Self::status_as_str(job.status))
        .bind(&job.started_at)
        .bind(&job.finished_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn list_jobs_by_stage(&self, stage_id: &StageId) -> Result<Vec<JobRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT id, stage_id, agent_id, name, status, started_at, finished_at FROM jobs WHERE stage_id = ?1 ORDER BY name",
        )
        .bind(stage_id.as_ref())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter().map(Self::map_job).collect()
    }

    async fn update_job_status(
        &self,
        id: &JobId,
        status: WorkflowStatus,
        started_at: Option<&str>,
        finished_at: Option<&str>,
    ) -> Result<(), DbError> {
        // Check current status - terminal states cannot be changed
        let current_status: Option<String> = sqlx::query_scalar("SELECT status FROM jobs WHERE id = ?1")
            .bind(id.as_ref())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        let current_status = current_status.ok_or_else(|| DbError::QueryFailed {
            reason: format!("job not found: {}", id),
        })?;

        let current = WorkflowStatus::parse_str(&current_status).map_err(|e| DbError::QueryFailed {
            reason: e,
        })?;

        if current.is_terminal() {
            return Err(DbError::QueryFailed {
                reason: format!("cannot change status from terminal state: {}", current),
            });
        }

        let result = sqlx::query("UPDATE jobs SET status = ?1, started_at = ?2, finished_at = ?3 WHERE id = ?4")
            .bind(Self::status_as_str(status))
            .bind(started_at)
            .bind(finished_at)
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        if result.rows_affected() == 0 {
            return Err(DbError::QueryFailed {
                reason: format!("job not found: {}", id),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::sqlite::{connect, migrate};
    use crate::workflow::WorkflowRepository;

    async fn create_test_pool() -> SqlitePool {
        let pool = connect("sqlite::memory:").await.expect("failed to connect");
        migrate(&pool).await.expect("failed to migrate");
        pool
    }

    #[tokio::test]
    async fn create_and_get_workflow() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        let workflow = WorkflowRecord::for_test("wf-1", "Test Workflow");
        repo.create_workflow(&workflow).await.unwrap();

        let retrieved = repo.get_workflow(&workflow.id).await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, workflow.id);
        assert_eq!(retrieved.name, workflow.name);
        assert_eq!(retrieved.status, workflow.status);
    }

    #[tokio::test]
    async fn list_workflows() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        let wf1 = WorkflowRecord::for_test("wf-1", "Alpha Workflow");
        let wf2 = WorkflowRecord::for_test("wf-2", "Beta Workflow");
        let wf3 = WorkflowRecord::for_test("wf-3", "Gamma Workflow");

        repo.create_workflow(&wf1).await.unwrap();
        repo.create_workflow(&wf2).await.unwrap();
        repo.create_workflow(&wf3).await.unwrap();

        let workflows = repo.list_workflows().await.unwrap();
        assert_eq!(workflows.len(), 3);
        // Should be ordered by name
        assert_eq!(workflows[0].name, "Alpha Workflow");
        assert_eq!(workflows[1].name, "Beta Workflow");
        assert_eq!(workflows[2].name, "Gamma Workflow");
    }

    #[tokio::test]
    async fn update_workflow_status() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        let workflow = WorkflowRecord::for_test("wf-1", "Test Workflow");
        repo.create_workflow(&workflow).await.unwrap();

        repo.update_workflow_status(&workflow.id, WorkflowStatus::Running)
            .await
            .unwrap();

        let retrieved = repo.get_workflow(&workflow.id).await.unwrap().unwrap();
        assert_eq!(retrieved.status, WorkflowStatus::Running);
    }

    #[tokio::test]
    async fn delete_workflow() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        let workflow = WorkflowRecord::for_test("wf-1", "Test Workflow");
        repo.create_workflow(&workflow).await.unwrap();

        let deleted = repo.delete_workflow(&workflow.id).await.unwrap();
        assert!(deleted);

        let retrieved = repo.get_workflow(&workflow.id).await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn create_and_list_stages() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        // Create a workflow first
        let workflow = WorkflowRecord::for_test("wf-1", "Test Workflow");
        repo.create_workflow(&workflow).await.unwrap();

        // Create stages
        let stage1 = StageRecord::for_test("stage-1", "wf-1", "Setup", 1);
        let stage2 = StageRecord::for_test("stage-2", "wf-1", "Execution", 2);
        let stage3 = StageRecord::for_test("stage-3", "wf-1", "Cleanup", 3);

        repo.create_stage(&stage1).await.unwrap();
        repo.create_stage(&stage2).await.unwrap();
        repo.create_stage(&stage3).await.unwrap();

        // List stages
        let stages = repo.list_stages_by_workflow(&workflow.id).await.unwrap();
        assert_eq!(stages.len(), 3);
        // Should be ordered by sequence
        assert_eq!(stages[0].sequence, 1);
        assert_eq!(stages[0].name, "Setup");
        assert_eq!(stages[1].sequence, 2);
        assert_eq!(stages[1].name, "Execution");
        assert_eq!(stages[2].sequence, 3);
        assert_eq!(stages[2].name, "Cleanup");
    }

    #[tokio::test]
    async fn create_and_list_jobs() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        // Create workflow and stage
        let workflow = WorkflowRecord::for_test("wf-1", "Test Workflow");
        repo.create_workflow(&workflow).await.unwrap();

        let stage = StageRecord::for_test("stage-1", "wf-1", "Setup", 1);
        repo.create_stage(&stage).await.unwrap();

        // Create llm_provider (required by agents foreign key)
        sqlx::query("INSERT INTO llm_providers (id, kind, display_name, base_url, model, encrypted_api_key, api_key_nonce) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")
            .bind("test-provider")
            .bind("openai_compatible")
            .bind("Test Provider")
            .bind("https://api.example.com")
            .bind("gpt-4")
            .bind(vec![1, 2, 3, 4])  // Dummy encrypted key
            .bind(vec![5, 6, 7, 8])  // Dummy nonce
            .execute(&repo.pool)
            .await
            .unwrap();

        // Create agents (required by jobs foreign key)
        for agent_id in &["agent-1", "agent-2", "agent-3"] {
            sqlx::query("INSERT INTO agents (id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)")
                .bind(agent_id)
                .bind(format!("Agent {agent_id}"))
                .bind("Test agent")
                .bind("1.0.0")
                .bind("test-provider")
                .bind("You are a test agent")
                .bind("[]")
                .bind(1000_i64)
                .bind(70_i64)  // Temperature as integer (0.7 * 100)
                .execute(&repo.pool)
                .await
                .unwrap();
        }

        // Create jobs
        let job1 = JobRecord::for_test("job-1", "stage-1", "agent-1", "Job A");
        let job2 = JobRecord::for_test("job-2", "stage-1", "agent-2", "Job B");
        let job3 = JobRecord::for_test("job-3", "stage-1", "agent-3", "Job C");

        repo.create_job(&job1).await.unwrap();
        repo.create_job(&job2).await.unwrap();
        repo.create_job(&job3).await.unwrap();

        // List jobs
        let jobs = repo.list_jobs_by_stage(&stage.id).await.unwrap();
        assert_eq!(jobs.len(), 3);
        // Should be ordered by name
        assert_eq!(jobs[0].name, "Job A");
        assert_eq!(jobs[1].name, "Job B");
        assert_eq!(jobs[2].name, "Job C");
    }

    #[tokio::test]
    async fn update_job_status_with_timestamps() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        // Create workflow and stage
        let workflow = WorkflowRecord::for_test("wf-1", "Test Workflow");
        repo.create_workflow(&workflow).await.unwrap();

        let stage = StageRecord::for_test("stage-1", "wf-1", "Setup", 1);
        repo.create_stage(&stage).await.unwrap();

        // Create llm_provider (required by agents foreign key)
        sqlx::query("INSERT INTO llm_providers (id, kind, display_name, base_url, model, encrypted_api_key, api_key_nonce) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")
            .bind("test-provider")
            .bind("openai_compatible")
            .bind("Test Provider")
            .bind("https://api.example.com")
            .bind("gpt-4")
            .bind(vec![1, 2, 3, 4])  // Dummy encrypted key
            .bind(vec![5, 6, 7, 8])  // Dummy nonce
            .execute(&repo.pool)
            .await
            .unwrap();

        // Create agent (required by jobs foreign key)
        sqlx::query("INSERT INTO agents (id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)")
            .bind("agent-1")
            .bind("Agent 1")
            .bind("Test agent")
            .bind("1.0.0")
            .bind("test-provider")
            .bind("You are a test agent")
            .bind("[]")
            .bind(1000_i64)
            .bind(70_i64)  // Temperature as integer (0.7 * 100)
            .execute(&repo.pool)
            .await
            .unwrap();

        // Create job
        let job = JobRecord::for_test("job-1", "stage-1", "agent-1", "Test Job");
        repo.create_job(&job).await.unwrap();

        // Update to running with started_at timestamp
        repo.update_job_status(
            &job.id,
            WorkflowStatus::Running,
            Some("2024-03-11T10:00:00Z"),
            None,
        )
        .await
        .unwrap();

        let jobs = repo.list_jobs_by_stage(&stage.id).await.unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, WorkflowStatus::Running);
        assert_eq!(jobs[0].started_at, Some("2024-03-11T10:00:00Z".to_string()));
        assert!(jobs[0].finished_at.is_none());

        // Update to succeeded with finished_at timestamp
        repo.update_job_status(
            &job.id,
            WorkflowStatus::Succeeded,
            Some("2024-03-11T10:00:00Z"),
            Some("2024-03-11T10:05:00Z"),
        )
        .await
        .unwrap();

        let jobs = repo.list_jobs_by_stage(&stage.id).await.unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, WorkflowStatus::Succeeded);
        assert_eq!(jobs[0].started_at, Some("2024-03-11T10:00:00Z".to_string()));
        assert_eq!(
            jobs[0].finished_at,
            Some("2024-03-11T10:05:00Z".to_string())
        );
    }
}
