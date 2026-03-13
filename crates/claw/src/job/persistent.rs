//! PersistentJobBackend - wraps JobRepository for persistent job execution.

use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

use super::backend::JobBackend;
use super::error::JobError;
use super::repository::JobRepository;
use super::types::{JobRecord, JobRequest, JobResult, JobStatus, JobType};
use crate::agents::AgentManager;
use crate::workflow::{JobId, WorkflowStatus};

/// Persistent job backend using JobRepository.
///
/// This backend submits jobs to the database repository and polls for completion.
/// It's used for orchestrate mode where jobs should survive restarts.
/// The actual execution is performed by the Scheduler.
pub struct PersistentJobBackend {
    job_repository: Arc<dyn JobRepository>,
    #[allow(dead_code)] // Will be used in future for direct agent spawning
    agent_manager: Arc<AgentManager>,
}

impl PersistentJobBackend {
    /// Create a new PersistentJobBackend.
    #[must_use]
    pub fn new(job_repository: Arc<dyn JobRepository>, agent_manager: Arc<AgentManager>) -> Self {
        Self {
            job_repository,
            agent_manager,
        }
    }
}

#[async_trait]
impl JobBackend for PersistentJobBackend {
    async fn submit(&self, job: JobRequest) -> Result<JobId, JobError> {
        let job_id = JobId::new(Uuid::new_v4().to_string());

        let record = JobRecord {
            id: job_id.clone(),
            job_type: JobType::Standalone,
            name: format!("Dispatched job for {}", job.agent_id),
            status: WorkflowStatus::Pending,
            agent_id: job.agent_id,
            context: job.context,
            prompt: job.prompt,
            thread_id: None,
            group_id: None,
            depends_on: vec![],
            cron_expr: None,
            scheduled_at: None,
            started_at: None,
            finished_at: None,
        };

        self.job_repository
            .create(&record)
            .await
            .map_err(|e| JobError::ExecutionFailed {
                reason: e.to_string(),
            })?;

        Ok(job_id)
    }

    async fn wait(&self, job_id: &JobId) -> Result<JobResult, JobError> {
        // Poll for job completion (the Scheduler will execute it)
        loop {
            let record = self
                .job_repository
                .get(job_id)
                .await
                .map_err(|e| JobError::ExecutionFailed {
                    reason: e.to_string(),
                })?
                .ok_or_else(|| JobError::NotFound {
                    id: job_id.as_ref().to_string(),
                })?;

            if record.status.is_terminal() {
                return match record.status {
                    WorkflowStatus::Succeeded => Ok(JobResult {
                        summary: "Job completed".to_string(),
                        token_usage: Default::default(),
                    }),
                    WorkflowStatus::Failed => Err(JobError::ExecutionFailed {
                        reason: "Job failed".to_string(),
                    }),
                    WorkflowStatus::Cancelled => Err(JobError::Cancelled),
                    _ => unreachable!("terminal status check"),
                };
            }

            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }

    async fn cancel(&self, job_id: &JobId) -> Result<(), JobError> {
        let record = self
            .job_repository
            .get(job_id)
            .await
            .map_err(|e| JobError::ExecutionFailed {
                reason: e.to_string(),
            })?
            .ok_or_else(|| JobError::NotFound {
                id: job_id.as_ref().to_string(),
            })?;

        if record.status.is_terminal() {
            return Ok(());
        }

        self.job_repository
            .update_status(job_id, WorkflowStatus::Cancelled, None, None)
            .await
            .map_err(|e| JobError::ExecutionFailed {
                reason: e.to_string(),
            })
    }

    async fn status(&self, job_id: &JobId) -> Result<JobStatus, JobError> {
        let record = self
            .job_repository
            .get(job_id)
            .await
            .map_err(|e| JobError::ExecutionFailed {
                reason: e.to_string(),
            })?
            .ok_or_else(|| JobError::NotFound {
                id: job_id.as_ref().to_string(),
            })?;

        Ok(JobStatus::from(record.status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistent_backend_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PersistentJobBackend>();
    }
}
