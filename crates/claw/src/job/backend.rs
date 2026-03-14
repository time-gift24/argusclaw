//! JobBackend trait - abstraction for job execution backends.

use async_trait::async_trait;

use crate::workflow::JobId;

use super::error::JobError;
use super::types::{JobRequest, JobResult, JobStatus};

/// Backend for job execution (InMemory or Persistent).
#[async_trait]
pub trait JobBackend: Send + Sync {
    /// Submit a job for execution, returns job ID.
    async fn submit(&self, job: JobRequest) -> Result<JobId, JobError>;

    /// Wait for job completion (blocking for sync mode).
    async fn wait(&self, job_id: &JobId) -> Result<JobResult, JobError>;

    /// Cancel a running job.
    async fn cancel(&self, job_id: &JobId) -> Result<(), JobError>;

    /// Query job status.
    async fn status(&self, job_id: &JobId) -> Result<JobStatus, JobError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper trait to verify backend implements Send + Sync.
    fn _assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn job_backend_is_send_sync() {
        _assert_send_sync::<Box<dyn JobBackend>>();
    }
}
