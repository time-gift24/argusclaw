//! JobRepository implementation for SQLite.

use async_trait::async_trait;

use crate::error::DbError;
use crate::traits::JobRepository;
use crate::types::{AgentId, JobId, JobRecord, JobResult, JobStatus, JobType};
use argus_protocol::ThreadId;

use super::{ArgusSqlite, DbResult};

#[async_trait]
impl JobRepository for ArgusSqlite {
    async fn create(&self, job: &JobRecord) -> DbResult<()> {
        let depends_on_json = serde_json::to_string(
            &job.depends_on
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".to_string());

        sqlx::query(
            "INSERT INTO jobs (id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at, parent_job_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
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
            "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at, parent_job_id, result
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
        status: JobStatus,
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
            "SELECT j.id, j.job_type, j.name, j.status, j.agent_id, j.context, j.prompt, j.thread_id, j.group_id, j.depends_on, j.cron_expr, j.scheduled_at, j.started_at, j.finished_at, j.parent_job_id, j.result
             FROM jobs j
             WHERE j.status = 'pending' AND j.job_type != 'cron'
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
            "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at, parent_job_id, result
             FROM jobs
             WHERE job_type = 'cron' AND status = 'pending' AND scheduled_at IS NOT NULL AND scheduled_at <= ?1
             ORDER BY scheduled_at ASC",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_job_record(r)).collect()
    }

    async fn claim_cron_job(&self, id: &JobId, started_at: &str) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE jobs
             SET status = 'running', started_at = ?1, updated_at = datetime('now')
             WHERE id = ?2 AND job_type = 'cron' AND status = 'pending'",
        )
        .bind(started_at)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(result.rows_affected() == 1)
    }

    async fn update_cron_after_run(
        &self,
        id: &JobId,
        status: JobStatus,
        scheduled_at: Option<&str>,
        finished_at: &str,
        context: Option<&str>,
    ) -> DbResult<()> {
        sqlx::query(
            "UPDATE jobs
             SET status = ?1, scheduled_at = ?2, finished_at = ?3, context = COALESCE(?4, context), updated_at = datetime('now')
             WHERE id = ?5 AND job_type = 'cron'",
        )
        .bind(status.as_str())
        .bind(scheduled_at)
        .bind(finished_at)
        .bind(context)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn list_cron_jobs(
        &self,
        include_paused: bool,
        thread_id: Option<&ThreadId>,
    ) -> DbResult<Vec<JobRecord>> {
        let rows = match (include_paused, thread_id) {
            (false, None) => {
                sqlx::query(
                    "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at, parent_job_id, result
                     FROM jobs
                     WHERE job_type = 'cron' AND status = 'pending'
                     ORDER BY scheduled_at ASC",
                )
                .fetch_all(&self.pool)
                .await
            }
            (false, Some(thread_id)) => {
                sqlx::query(
                    "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at, parent_job_id, result
                     FROM jobs
                     WHERE job_type = 'cron' AND status = 'pending' AND thread_id = ?1
                     ORDER BY scheduled_at ASC",
                )
                .bind(thread_id.to_string())
                .fetch_all(&self.pool)
                .await
            }
            (true, None) => {
                sqlx::query(
                    "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at, parent_job_id, result
                     FROM jobs
                     WHERE job_type = 'cron' AND status IN ('pending', 'paused', 'running', 'failed')
                     ORDER BY scheduled_at ASC",
                )
                .fetch_all(&self.pool)
                .await
            }
            (true, Some(thread_id)) => {
                sqlx::query(
                    "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at, parent_job_id, result
                     FROM jobs
                     WHERE job_type = 'cron' AND status IN ('pending', 'paused', 'running', 'failed') AND thread_id = ?1
                     ORDER BY scheduled_at ASC",
                )
                .bind(thread_id.to_string())
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

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
            "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at, parent_job_id, result
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
        let parent_job_id: Option<JobId> =
            Self::get_column::<Option<String>>(&row, "parent_job_id")?.map(|s| JobId::new(&s));
        let result: Option<JobResult> = Self::get_column::<Option<String>>(&row, "result")?
            .and_then(|s| serde_json::from_str(&s).ok());

        Ok(JobRecord {
            id: JobId::new(&Self::get_column::<String>(&row, "id")?),
            job_type: JobType::parse_str(&Self::get_column::<String>(&row, "job_type")?)
                .map_err(|e| DbError::QueryFailed { reason: e })?,
            name: Self::get_column(&row, "name")?,
            status: JobStatus::parse_str(&Self::get_column::<String>(&row, "status")?)
                .map_err(|e| DbError::QueryFailed { reason: e })?,
            agent_id: AgentId::new(Self::get_column(&row, "agent_id")?),
            context: Self::get_column(&row, "context")?,
            prompt: Self::get_column(&row, "prompt")?,
            thread_id,
            group_id: Self::get_column(&row, "group_id")?,
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
