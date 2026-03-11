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
        sqlx::query(
            "INSERT INTO workflows (id, name, status) VALUES (?1, ?2, ?3)",
        )
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
        sqlx::query("UPDATE workflows SET status = ?1 WHERE id = ?2")
            .bind(Self::status_as_str(status))
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

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
        sqlx::query(
            "UPDATE jobs SET status = ?1, started_at = ?2, finished_at = ?3 WHERE id = ?4",
        )
        .bind(Self::status_as_str(status))
        .bind(started_at)
        .bind(finished_at)
        .bind(id.as_ref())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }
}
