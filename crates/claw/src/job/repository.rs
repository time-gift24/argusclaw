use async_trait::async_trait;

use crate::protocol::ThreadId;
use crate::db::DbError;
use crate::workflow::{JobId, WorkflowStatus};

use super::types::JobRecord;

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
        status: WorkflowStatus,
        started_at: Option<&str>,
        finished_at: Option<&str>,
    ) -> Result<(), DbError>;

    /// Associate a thread with a job.
    async fn update_thread_id(&self, id: &JobId, thread_id: &ThreadId) -> Result<(), DbError>;

    /// Find jobs that are ready to execute (pending with all dependencies satisfied).
    async fn find_ready_jobs(&self, limit: usize) -> Result<Vec<JobRecord>, DbError>;

    /// Find cron jobs that are due for execution.
    async fn find_due_cron_jobs(&self, now: &str) -> Result<Vec<JobRecord>, DbError>;

    /// Update the next scheduled time for a cron job.
    async fn update_scheduled_at(&self, id: &JobId, next: &str) -> Result<(), DbError>;

    /// List all jobs in a group.
    async fn list_by_group(&self, group_id: &str) -> Result<Vec<JobRecord>, DbError>;

    /// Delete a job. Returns true if a row was deleted.
    async fn delete(&self, id: &JobId) -> Result<bool, DbError>;
}
