//! SQLite implementation of JobRepository.

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::agents::AgentId;
use crate::db::DbError;
use crate::job::repository::JobRepository;
use crate::job::types::{JobRecord, JobType};
use crate::protocol::ThreadId;
use crate::workflow::{JobId, WorkflowStatus};

/// Row type for jobs table (14 columns, excluding created_at/updated_at).
/// agent_id is now INTEGER (i64).
#[allow(dead_code)]
type JobRow = (
    String,         // id
    String,         // job_type
    String,         // name
    String,         // status
    i64,            // agent_id (INTEGER)
    Option<String>, // context
    String,         // prompt
    Option<String>, // thread_id
    Option<String>, // group_id
    String,         // depends_on
    Option<String>, // cron_expr
    Option<String>, // scheduled_at
    Option<String>, // started_at
    Option<String>, // finished_at
);

/// SQLite-backed job repository.
#[derive(Clone)]
pub struct SqliteJobRepository {
    pool: SqlitePool,
}

#[allow(clippy::type_complexity)]
impl SqliteJobRepository {
    /// Create a new SQLite job repository.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Parse a workflow status from string.
    fn parse_status(s: &str) -> Result<WorkflowStatus, DbError> {
        WorkflowStatus::parse_str(s).map_err(|e| DbError::QueryFailed { reason: e })
    }

    /// Parse a job type from string.
    fn parse_job_type(s: &str) -> Result<JobType, DbError> {
        JobType::parse_str(s).map_err(|e| DbError::QueryFailed { reason: e })
    }

    /// Parse depends_on JSON array into Vec<JobId>.
    fn parse_depends_on(s: &str) -> Vec<JobId> {
        serde_json::from_str::<Vec<String>>(s)
            .map(|ids| ids.into_iter().map(JobId::new).collect())
            .unwrap_or_default()
    }

    /// Serialize depends_on Vec<JobId> to JSON array string.
    fn serialize_depends_on(deps: &[JobId]) -> String {
        let ids: Vec<String> = deps.iter().map(|id| id.to_string()).collect();
        serde_json::to_string(&ids).unwrap_or_else(|_| "[]".to_string())
    }

    /// Parse optional thread_id string to ThreadId.
    fn parse_thread_id(s: Option<String>) -> Option<ThreadId> {
        s.and_then(|id| ThreadId::parse(&id).ok())
    }

    /// Helper to get a column value from a row.
    fn get_column<T>(row: &sqlx::sqlite::SqliteRow, col: &str) -> Result<T, DbError>
    where
        T: for<'r> sqlx::decode::Decode<'r, sqlx::Sqlite> + sqlx::types::Type<sqlx::Sqlite>,
    {
        row.try_get(col).map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })
    }

    /// Map a database row to JobRecord.
    fn map_row(row: JobRow) -> Result<JobRecord, DbError> {
        let (
            id,
            job_type,
            name,
            status,
            agent_id,
            context,
            prompt,
            thread_id,
            group_id,
            depends_on,
            cron_expr,
            scheduled_at,
            started_at,
            finished_at,
        ) = row;

        Ok(JobRecord {
            id: JobId::new(&id),
            job_type: Self::parse_job_type(&job_type)?,
            name,
            status: Self::parse_status(&status)?,
            agent_id: AgentId::new(agent_id),
            context,
            prompt,
            thread_id: Self::parse_thread_id(thread_id),
            group_id,
            depends_on: Self::parse_depends_on(&depends_on),
            cron_expr,
            scheduled_at,
            started_at,
            finished_at,
        })
    }
}

#[async_trait]
impl JobRepository for SqliteJobRepository {
    async fn create(&self, job: &JobRecord) -> Result<(), DbError> {
        sqlx::query(
            r#"
            INSERT INTO jobs (id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            "#,
        )
        .bind(job.id.to_string())
        .bind(job.job_type.as_str())
        .bind(&job.name)
        .bind(job.status.as_str())
        .bind(job.agent_id.into_inner())
        .bind(&job.context)
        .bind(&job.prompt)
        .bind(job.thread_id.map(|t| t.to_string()))
        .bind(&job.group_id)
        .bind(Self::serialize_depends_on(&job.depends_on))
        .bind(&job.cron_expr)
        .bind(&job.scheduled_at)
        .bind(&job.started_at)
        .bind(&job.finished_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn get(&self, id: &JobId) -> Result<Option<JobRecord>, DbError> {
        let row = sqlx::query(
            r#"
            SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at
            FROM jobs
            WHERE id = ?1
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| {
            let row: JobRow = (
                Self::get_column(&row, "id")?,
                Self::get_column(&row, "job_type")?,
                Self::get_column(&row, "name")?,
                Self::get_column(&row, "status")?,
                Self::get_column(&row, "agent_id")?,
                Self::get_column(&row, "context")?,
                Self::get_column(&row, "prompt")?,
                Self::get_column(&row, "thread_id")?,
                Self::get_column(&row, "group_id")?,
                Self::get_column(&row, "depends_on")?,
                Self::get_column(&row, "cron_expr")?,
                Self::get_column(&row, "scheduled_at")?,
                Self::get_column(&row, "started_at")?,
                Self::get_column(&row, "finished_at")?,
            );
            Self::map_row(row)
        })
        .transpose()
    }

    async fn update_status(
        &self,
        id: &JobId,
        status: WorkflowStatus,
        started_at: Option<&str>,
        finished_at: Option<&str>,
    ) -> Result<(), DbError> {
        let result = sqlx::query(
            r#"
            UPDATE jobs
            SET status = ?1, started_at = ?2, finished_at = ?3, updated_at = datetime('now')
            WHERE id = ?4 AND status NOT IN ('succeeded', 'failed', 'cancelled')
            "#,
        )
        .bind(status.as_str())
        .bind(started_at)
        .bind(finished_at)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        if result.rows_affected() == 0 {
            return Err(DbError::QueryFailed {
                reason: format!("job {} not found or in terminal state", id),
            });
        }

        Ok(())
    }

    async fn update_thread_id(&self, id: &JobId, thread_id: &ThreadId) -> Result<(), DbError> {
        sqlx::query(
            r#"
            UPDATE jobs
            SET thread_id = ?1, updated_at = datetime('now')
            WHERE id = ?2
            "#,
        )
        .bind(thread_id.to_string())
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn find_ready_jobs(&self, limit: usize) -> Result<Vec<JobRecord>, DbError> {
        let rows = sqlx::query(
            r#"
            SELECT j.id, j.job_type, j.name, j.status, j.agent_id, j.context, j.prompt, j.thread_id, j.group_id, j.depends_on, j.cron_expr, j.scheduled_at, j.started_at, j.finished_at
            FROM jobs j
            WHERE j.status = 'pending' AND j.job_type != 'cron'
              AND NOT EXISTS (
                  SELECT 1 FROM jobs dep
                  WHERE dep.id IN (SELECT value FROM json_each(j.depends_on))
                    AND dep.status != 'succeeded'
              )
            ORDER BY j.created_at ASC
            LIMIT ?1
            "#,
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter()
            .map(|row| {
                let row: JobRow = (
                    Self::get_column(&row, "id")?,
                    Self::get_column(&row, "job_type")?,
                    Self::get_column(&row, "name")?,
                    Self::get_column(&row, "status")?,
                    Self::get_column(&row, "agent_id")?,
                    Self::get_column(&row, "context")?,
                    Self::get_column(&row, "prompt")?,
                    Self::get_column(&row, "thread_id")?,
                    Self::get_column(&row, "group_id")?,
                    Self::get_column(&row, "depends_on")?,
                    Self::get_column(&row, "cron_expr")?,
                    Self::get_column(&row, "scheduled_at")?,
                    Self::get_column(&row, "started_at")?,
                    Self::get_column(&row, "finished_at")?,
                );
                Self::map_row(row)
            })
            .collect()
    }

    async fn find_due_cron_jobs(&self, now: &str) -> Result<Vec<JobRecord>, DbError> {
        let rows = sqlx::query(
            r#"
            SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at
            FROM jobs
            WHERE job_type = 'cron' AND scheduled_at IS NOT NULL AND scheduled_at <= ?1
            ORDER BY scheduled_at ASC
            "#,
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter()
            .map(|row| {
                let row: JobRow = (
                    Self::get_column(&row, "id")?,
                    Self::get_column(&row, "job_type")?,
                    Self::get_column(&row, "name")?,
                    Self::get_column(&row, "status")?,
                    Self::get_column(&row, "agent_id")?,
                    Self::get_column(&row, "context")?,
                    Self::get_column(&row, "prompt")?,
                    Self::get_column(&row, "thread_id")?,
                    Self::get_column(&row, "group_id")?,
                    Self::get_column(&row, "depends_on")?,
                    Self::get_column(&row, "cron_expr")?,
                    Self::get_column(&row, "scheduled_at")?,
                    Self::get_column(&row, "started_at")?,
                    Self::get_column(&row, "finished_at")?,
                );
                Self::map_row(row)
            })
            .collect()
    }

    async fn update_scheduled_at(&self, id: &JobId, next: &str) -> Result<(), DbError> {
        sqlx::query(
            r#"
            UPDATE jobs
            SET scheduled_at = ?1, updated_at = datetime('now')
            WHERE id = ?2
            "#,
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

    async fn list_by_group(&self, group_id: &str) -> Result<Vec<JobRecord>, DbError> {
        let rows = sqlx::query(
            r#"
            SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at
            FROM jobs
            WHERE group_id = ?1
            ORDER BY created_at ASC
            "#,
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter()
            .map(|row| {
                let row: JobRow = (
                    Self::get_column(&row, "id")?,
                    Self::get_column(&row, "job_type")?,
                    Self::get_column(&row, "name")?,
                    Self::get_column(&row, "status")?,
                    Self::get_column(&row, "agent_id")?,
                    Self::get_column(&row, "context")?,
                    Self::get_column(&row, "prompt")?,
                    Self::get_column(&row, "thread_id")?,
                    Self::get_column(&row, "group_id")?,
                    Self::get_column(&row, "depends_on")?,
                    Self::get_column(&row, "cron_expr")?,
                    Self::get_column(&row, "scheduled_at")?,
                    Self::get_column(&row, "started_at")?,
                    Self::get_column(&row, "finished_at")?,
                );
                Self::map_row(row)
            })
            .collect()
    }

    async fn delete(&self, id: &JobId) -> Result<bool, DbError> {
        let result = sqlx::query(
            r#"
            DELETE FROM jobs WHERE id = ?1
            "#,
        )
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(result.rows_affected() > 0)
    }
}
