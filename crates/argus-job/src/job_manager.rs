//! JobManager for dispatching and managing background jobs.

use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::dispatch_tool::DispatchJobTool;
use crate::error::JobError;
use crate::sse_broadcaster::SseBroadcaster;
use crate::types::{JobDispatchArgs, JobDispatchResult, JobResult};

/// Manages job dispatch and lifecycle.
#[derive(Debug)]
pub struct JobManager {
    jobs: Arc<RwLock<std::collections::HashMap<String, JobState>>>,
    broadcaster: Arc<SseBroadcaster>,
}

#[derive(Debug, Clone)]
struct JobState {
    status: String,
    result: Option<JobResult>,
}

impl JobManager {
    /// Create a new JobManager.
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            broadcaster: Arc::new(SseBroadcaster::new()),
        }
    }

    /// Get the SSE broadcaster for this manager.
    pub fn broadcaster(&self) -> &SseBroadcaster {
        &self.broadcaster
    }

    /// Dispatch a new job.
    pub async fn dispatch(&self, args: JobDispatchArgs) -> Result<JobDispatchResult, JobError> {
        let job_id = Uuid::new_v4().to_string();

        // Store initial job state
        {
            let mut jobs = self.jobs.write().await;
            jobs.insert(
                job_id.clone(),
                JobState {
                    status: "submitted".to_string(),
                    result: None,
                },
            );
        }

        // For now, we just track the job. Actual execution would be done
        // by spawning a background task that runs the turn execution.
        tracing::info!("job {} dispatched for agent {:?}", job_id, args.agent_id);

        Ok(JobDispatchResult {
            job_id,
            status: "submitted".to_string(),
            result: None,
        })
    }

    /// Get the result of a job.
    pub async fn get_result(&self, job_id: &str) -> Result<Option<JobResult>, JobError> {
        let jobs = self.jobs.read().await;
        Ok(jobs.get(job_id).and_then(|s| s.result.clone()))
    }

    /// Mark a job as completed.
    pub async fn mark_completed(&self, job_id: &str, result: JobResult) {
        let mut jobs = self.jobs.write().await;
        if let Some(state) = jobs.get_mut(job_id) {
            state.status = "completed".to_string();
            state.result = Some(result);
        }
        self.broadcaster
            .broadcast_completed(job_id.to_string(), None);
    }

    /// Mark a job as failed.
    pub async fn mark_failed(&self, job_id: &str, message: String) {
        let mut jobs = self.jobs.write().await;
        if let Some(state) = jobs.get_mut(job_id) {
            state.status = "failed".to_string();
            state.result = Some(JobResult {
                success: false,
                message: message.clone(),
                token_usage: None,
            });
        }
        self.broadcaster
            .broadcast_failed(job_id.to_string(), None, message);
    }

    /// Create a DispatchJobTool for this manager.
    pub fn create_dispatch_tool(self: Arc<Self>) -> DispatchJobTool {
        DispatchJobTool::new(self)
    }
}

impl Default for JobManager {
    fn default() -> Self {
        Self::new()
    }
}
