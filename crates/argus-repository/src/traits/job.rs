//! Job repository trait.

use async_trait::async_trait;

use crate::error::DbError;
use crate::types::{JobId, JobRecord, JobResult, JobStatus};
use argus_protocol::ThreadId;

/// Repository trait for job persistence.
#[async_trait]
pub trait JobRepository: Send + Sync {
    /// Create a new job.
    async fn create(&self, job: &JobRecord) -> Result<(), DbError>;

    /// Get a job by ID.
    async fn get(&self, id: &JobId) -> Result<Option<JobRecord>, DbError>;

    /// Update the status of a job, optionally setting started/finished timestamps.
    async fn update_status(
        &self,
        id: &JobId,
        status: JobStatus,
        started_at: Option<&str>,
        finished_at: Option<&str>,
    ) -> Result<(), DbError>;

    /// Update the result of a job.
    async fn update_result(&self, id: &JobId, result: &JobResult) -> Result<(), DbError>;

    /// Associate a thread with a job.
    async fn update_thread_id(&self, id: &JobId, thread_id: &ThreadId) -> Result<(), DbError>;

    /// Find jobs that are ready to execute (pending with all dependencies satisfied).
    async fn find_ready_jobs(&self, limit: usize) -> Result<Vec<JobRecord>, DbError>;

    /// Find cron jobs that are due for execution.
    async fn find_due_cron_jobs(&self, now: &str) -> Result<Vec<JobRecord>, DbError>;

    /// Atomically claim a pending cron job for execution.
    async fn claim_cron_job(&self, id: &JobId, started_at: &str) -> Result<bool, DbError>;

    /// Update a cron job after a run completes.
    async fn update_cron_after_run(
        &self,
        id: &JobId,
        status: JobStatus,
        scheduled_at: Option<&str>,
        finished_at: &str,
        context: Option<&str>,
    ) -> Result<(), DbError>;

    /// List cron jobs, optionally including paused and in-flight records.
    async fn list_cron_jobs(
        &self,
        include_paused: bool,
        thread_id: Option<&ThreadId>,
    ) -> Result<Vec<JobRecord>, DbError>;

    /// Update the next scheduled time for a cron job.
    async fn update_scheduled_at(&self, id: &JobId, next: &str) -> Result<(), DbError>;

    /// List all jobs in a group.
    async fn list_by_group(&self, group_id: &str) -> Result<Vec<JobRecord>, DbError>;

    /// Delete a job. Returns true if a row was deleted.
    async fn delete(&self, id: &JobId) -> Result<bool, DbError>;
}
