//! InMemoryJobBackend - manages jobs in memory for synchronous dispatch.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dashmap::DashMap;
use tokio::sync::{RwLock, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::agents::agent::AgentManager;
use crate::agents::thread::ThreadConfig;
use crate::workflow::JobId;

use super::backend::JobBackend;
use super::error::JobError;
use super::types::{JobRequest, JobResult, JobStatus};

/// Configuration for InMemoryJobBackend.
#[derive(Debug, Clone)]
pub struct InMemoryBackendConfig {
    /// Default timeout for jobs in seconds.
    pub default_timeout_secs: u64,
    /// Interval for progress notifications in seconds.
    pub progress_notify_interval_secs: u64,
    /// Maximum concurrent jobs.
    pub max_concurrent_jobs: usize,
}

impl Default for InMemoryBackendConfig {
    fn default() -> Self {
        Self {
            default_timeout_secs: 300,
            progress_notify_interval_secs: 60,
            max_concurrent_jobs: 10,
        }
    }
}

/// Internal representation of a job in memory.
struct InMemoryJob {
    /// Current status (shared state).
    status: Arc<RwLock<JobStatus>>,
    /// Receiver for the final result.
    result_rx: Option<oneshot::Receiver<Result<JobResult, JobError>>>,
    /// Cancellation token.
    cancel_token: CancellationToken,
    /// When the job was started.
    #[allow(dead_code)]
    started_at: Instant,
}

/// InMemoryJobBackend - manages jobs in memory for synchronous dispatch.
///
/// This backend uses DashMap for concurrent access, oneshot channels for wait(),
/// and CancellationToken for cancel. Jobs execute in spawned tokio tasks.
pub struct InMemoryJobBackend {
    /// Active jobs indexed by JobId.
    jobs: DashMap<JobId, InMemoryJob>,
    /// Agent manager for creating runtime agents.
    agent_manager: Arc<AgentManager>,
    /// Configuration.
    config: InMemoryBackendConfig,
}

impl InMemoryJobBackend {
    /// Create a new InMemoryJobBackend.
    pub fn new(agent_manager: Arc<AgentManager>) -> Self {
        Self {
            jobs: DashMap::new(),
            agent_manager,
            config: InMemoryBackendConfig::default(),
        }
    }

    /// Create a new InMemoryJobBackend with custom configuration.
    pub fn with_config(agent_manager: Arc<AgentManager>, config: InMemoryBackendConfig) -> Self {
        Self {
            jobs: DashMap::new(),
            agent_manager,
            config,
        }
    }

    /// Get the number of active jobs.
    #[must_use]
    pub fn job_count(&self) -> usize {
        self.jobs.len()
    }

    /// Get the configuration.
    #[must_use]
    pub fn config(&self) -> &InMemoryBackendConfig {
        &self.config
    }
}

#[async_trait]
impl JobBackend for InMemoryJobBackend {
    async fn submit(&self, request: JobRequest) -> Result<JobId, JobError> {
        // Check concurrency limit
        if self.jobs.len() >= self.config.max_concurrent_jobs {
            return Err(JobError::ConcurrencyLimit(self.config.max_concurrent_jobs));
        }

        // Generate job ID
        let job_id = JobId::new(Uuid::new_v4().to_string());

        // Create shared status
        let status = Arc::new(RwLock::new(JobStatus::Pending));

        // Create oneshot channel for result
        let (result_tx, result_rx) = oneshot::channel();

        // Create cancellation token
        let cancel_token = CancellationToken::new();

        // Store job (without result_tx - it goes to spawned task)
        let job = InMemoryJob {
            status: status.clone(),
            result_rx: Some(result_rx),
            cancel_token: cancel_token.clone(),
            started_at: Instant::now(),
        };

        self.jobs.insert(job_id.clone(), job);

        // Spawn execution task
        let agent_manager = self.agent_manager.clone();
        let timeout_secs = request.timeout_secs;

        tokio::spawn(async move {
            // Update status to Running
            {
                let mut s = status.write().await;
                *s = JobStatus::Running;
            }

            // Execute with timeout and cancellation support
            let result = execute_job_with_timeout(
                agent_manager,
                request,
                cancel_token.clone(),
                timeout_secs,
            )
            .await;

            // Update final status and send result
            match &result {
                Ok(_) => {
                    let mut s = status.write().await;
                    *s = JobStatus::Succeeded;
                }
                Err(JobError::Cancelled) => {
                    let mut s = status.write().await;
                    *s = JobStatus::Cancelled;
                }
                Err(JobError::Timeout) => {
                    let mut s = status.write().await;
                    *s = JobStatus::TimedOut;
                }
                Err(_) => {
                    let mut s = status.write().await;
                    *s = JobStatus::Failed;
                }
            }

            // Send result through channel (ignore if receiver dropped)
            let _ = result_tx.send(result);
        });

        Ok(job_id)
    }

    async fn wait(&self, job_id: &JobId) -> Result<JobResult, JobError> {
        // Get the job and take the result receiver
        let result_rx = {
            let mut entry = self
                .jobs
                .get_mut(job_id)
                .ok_or_else(|| JobError::NotFound {
                    id: job_id.to_string(),
                })?;

            entry.result_rx.take().ok_or(JobError::AlreadyConsumed)?
        };

        // Wait for result
        result_rx.await.map_err(|_| JobError::ChannelClosed)?
    }

    async fn cancel(&self, job_id: &JobId) -> Result<(), JobError> {
        let entry = self.jobs.get(job_id).ok_or_else(|| JobError::NotFound {
            id: job_id.to_string(),
        })?;

        entry.cancel_token.cancel();
        Ok(())
    }

    async fn status(&self, job_id: &JobId) -> Result<JobStatus, JobError> {
        let entry = self.jobs.get(job_id).ok_or_else(|| JobError::NotFound {
            id: job_id.to_string(),
        })?;

        let status = entry.status.read().await;
        Ok(*status)
    }
}

/// Execute a job with timeout and cancellation support.
async fn execute_job_with_timeout(
    agent_manager: Arc<AgentManager>,
    request: JobRequest,
    cancel_token: CancellationToken,
    timeout_secs: u64,
) -> Result<JobResult, JobError> {
    tokio::select! {
        // Cancellation path
        _ = cancel_token.cancelled() => {
            Err(JobError::Cancelled)
        }
        // Timeout path
        _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => {
            cancel_token.cancel();
            Err(JobError::Timeout)
        }
        // Main execution path
        result = execute_job(agent_manager, request) => {
            result
        }
    }
}

/// Execute a job by creating a runtime agent and running the prompt.
async fn execute_job(
    agent_manager: Arc<AgentManager>,
    request: JobRequest,
) -> Result<JobResult, JobError> {
    // Get agent template
    let template = agent_manager
        .get_template(&request.agent_id)
        .await?
        .ok_or_else(|| JobError::AgentNotFound {
            id: request.agent_id.to_string(),
        })?;

    // Create runtime agent
    let runtime_id =
        agent_manager
            .create_agent(&template)
            .await
            .map_err(|e| JobError::ExecutionFailed {
                reason: e.to_string(),
            })?;

    // Get the runtime agent
    let agent = agent_manager.get(runtime_id);

    let Some(agent) = agent else {
        // Cleanup on error
        agent_manager.delete(runtime_id);
        return Err(JobError::AgentCreationFailed);
    };

    // Create a thread
    let thread_id = agent.create_thread(ThreadConfig::default());

    // Get mutable access to thread and send message
    let result = async {
        let Some(mut thread) = agent.get_thread_mut(&thread_id) else {
            return Err(JobError::ThreadNotFound);
        };

        // Build the full prompt with context if provided
        let full_prompt = if let Some(context) = &request.context {
            format!("Context: {}\n\nTask: {}", context, request.prompt)
        } else {
            request.prompt.clone()
        };

        // Send message and wait for result
        let handle = thread.send_message(full_prompt).await;

        // Wait for completion
        let output = handle.wait_for_result().await?;

        // Extract summary from last message
        let summary = output
            .messages
            .last()
            .map(|m| {
                // Truncate summary if too long
                let content = &m.content;
                if content.len() > 1000 {
                    format!("{}...", &content[..997])
                } else {
                    content.clone()
                }
            })
            .unwrap_or_else(|| "No response generated".to_string());

        Ok(JobResult {
            summary,
            token_usage: output.token_usage,
        })
    }
    .await;

    // Cleanup: delete the runtime agent
    agent_manager.delete(runtime_id);

    result
}

impl std::fmt::Debug for InMemoryJobBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryJobBackend")
            .field("job_count", &self.jobs.len())
            .field("config", &self.config)
            .finish()
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn in_memory_backend_config_defaults() {
        let config = InMemoryBackendConfig::default();
        assert_eq!(config.default_timeout_secs, 300);
        assert_eq!(config.progress_notify_interval_secs, 60);
        assert_eq!(config.max_concurrent_jobs, 10);
    }

    #[test]
    fn in_memory_backend_config_custom() {
        let config = InMemoryBackendConfig {
            default_timeout_secs: 600,
            progress_notify_interval_secs: 30,
            max_concurrent_jobs: 5,
        };
        assert_eq!(config.default_timeout_secs, 600);
        assert_eq!(config.progress_notify_interval_secs, 30);
        assert_eq!(config.max_concurrent_jobs, 5);
    }
}
