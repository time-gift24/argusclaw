//! JobManager for dispatching and managing background jobs.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tokio::time::{Instant, sleep};
use uuid::Uuid;

use crate::dispatch_tool::DispatchJobTool;
use crate::error::JobError;
use crate::get_job_result_tool::GetJobResultTool;
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

const JOB_RUNTIME_EXECUTION_DELAY: Duration = Duration::from_millis(25);
const JOB_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const JOB_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

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
        let wait_for_result = args.wait_for_result;

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

        tracing::info!("job {} dispatched for agent {:?}", job_id, args.agent_id);
        self.spawn_background_execution(job_id.clone(), args);

        if wait_for_result {
            let result = self.wait_for_result(&job_id).await?;
            let status = if result.success {
                "completed"
            } else {
                "failed"
            };
            return Ok(JobDispatchResult {
                job_id,
                status: status.to_string(),
                result: Some(result),
            });
        }

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

    /// Create a GetJobResultTool for this manager.
    pub fn create_get_result_tool(self: Arc<Self>) -> GetJobResultTool {
        GetJobResultTool::new(self)
    }

    fn spawn_background_execution(&self, job_id: String, args: JobDispatchArgs) {
        let jobs = Arc::clone(&self.jobs);
        let broadcaster = Arc::clone(&self.broadcaster);

        tokio::spawn(async move {
            {
                let mut guard = jobs.write().await;
                if let Some(state) = guard.get_mut(&job_id) {
                    state.status = "running".to_string();
                }
            }

            let (final_status, final_result) = match Self::execute_job(args).await {
                Ok(result) => ("completed".to_string(), result),
                Err(err) => (
                    "failed".to_string(),
                    JobResult {
                        success: false,
                        message: err.to_string(),
                        token_usage: None,
                    },
                ),
            };

            {
                let mut guard = jobs.write().await;
                if let Some(state) = guard.get_mut(&job_id) {
                    state.status = final_status.clone();
                    state.result = Some(final_result.clone());
                }
            }

            if final_status == "completed" {
                broadcaster.broadcast_completed(job_id.clone(), None);
            } else {
                broadcaster.broadcast_failed(job_id.clone(), None, final_result.message.clone());
            }
        });
    }

    async fn wait_for_result(&self, job_id: &str) -> Result<JobResult, JobError> {
        let start = Instant::now();

        loop {
            let maybe_result = {
                let jobs = self.jobs.read().await;
                let state = jobs
                    .get(job_id)
                    .ok_or_else(|| JobError::JobNotFound(job_id.to_string()))?;
                state.result.clone()
            };

            if let Some(result) = maybe_result {
                return Ok(result);
            }

            if start.elapsed() >= JOB_WAIT_TIMEOUT {
                return Err(JobError::ExecutionFailed(format!(
                    "timed out waiting for job {job_id} after {}s",
                    JOB_WAIT_TIMEOUT.as_secs()
                )));
            }

            sleep(JOB_WAIT_POLL_INTERVAL).await;
        }
    }

    async fn execute_job(args: JobDispatchArgs) -> Result<JobResult, JobError> {
        if args.prompt.trim().is_empty() {
            return Err(JobError::ExecutionFailed(
                "prompt cannot be empty".to_string(),
            ));
        }

        // Keep runtime behavior deterministic for now while full turn execution is wired in.
        sleep(JOB_RUNTIME_EXECUTION_DELAY).await;

        Ok(JobResult {
            success: true,
            message: format!("job completed for agent {}", args.agent_id.inner()),
            token_usage: None,
        })
    }
}

impl Default for JobManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::{sleep, timeout};

    use argus_protocol::AgentId;

    fn build_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
    }

    #[test]
    fn dispatch_without_wait_starts_background_execution() {
        let rt = build_runtime();
        rt.block_on(async {
            let manager = JobManager::new();
            let dispatch = manager
                .dispatch(JobDispatchArgs {
                    prompt: "summarize project status".to_string(),
                    agent_id: AgentId::new(1),
                    context: None,
                    wait_for_result: false,
                })
                .await
                .expect("dispatch should succeed");

            assert_eq!(dispatch.status, "submitted");
            assert!(dispatch.result.is_none());

            let job_id = dispatch.job_id.clone();
            let completed = timeout(Duration::from_secs(1), async {
                loop {
                    if let Some(result) = manager.get_result(&job_id).await.expect("query result") {
                        break result;
                    }
                    sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("job should complete in background");

            assert!(completed.success);
        });
    }

    #[test]
    fn dispatch_with_wait_returns_completed_result() {
        let rt = build_runtime();
        rt.block_on(async {
            let manager = JobManager::new();
            let dispatch = manager
                .dispatch(JobDispatchArgs {
                    prompt: "collect diagnostics".to_string(),
                    agent_id: AgentId::new(1),
                    context: None,
                    wait_for_result: true,
                })
                .await
                .expect("dispatch should succeed");

            assert_eq!(dispatch.status, "completed");
            let result = dispatch.result.expect("result should be present");
            assert!(result.success);
        });
    }

    #[test]
    fn dispatch_emits_completion_event() {
        let rt = build_runtime();
        rt.block_on(async {
            let manager = JobManager::new();
            let mut rx = manager.broadcaster().subscribe();

            let dispatch = manager
                .dispatch(JobDispatchArgs {
                    prompt: "check rust warnings".to_string(),
                    agent_id: AgentId::new(1),
                    context: None,
                    wait_for_result: false,
                })
                .await
                .expect("dispatch should succeed");

            let event = timeout(Duration::from_secs(1), rx.recv())
                .await
                .expect("expected completion event")
                .expect("event should be readable");

            assert_eq!(event.job_id, dispatch.job_id);
            assert_eq!(event.status, "completed");
        });
    }
}
