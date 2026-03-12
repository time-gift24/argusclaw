use std::str::FromStr;
use std::sync::Arc;

use chrono::Utc;
use cron::Schedule;
use dashmap::DashMap;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::agents::AgentManager;
use crate::agents::thread::ThreadConfig;
use crate::job::repository::JobRepository;
use crate::job::types::{JobRecord, JobType};
use crate::workflow::{JobId, WorkflowStatus};

use super::config::SchedulerConfig;
use super::error::SchedulerError;

pub struct Scheduler {
    config: SchedulerConfig,
    job_repository: Arc<dyn JobRepository>,
    agent_manager: Arc<AgentManager>,
    running_jobs: DashMap<JobId, JoinHandle<()>>,
    shutdown: CancellationToken,
}

impl Scheduler {
    #[must_use]
    pub fn new(
        config: SchedulerConfig,
        job_repository: Arc<dyn JobRepository>,
        agent_manager: Arc<AgentManager>,
    ) -> Self {
        Self {
            config,
            job_repository,
            agent_manager,
            running_jobs: DashMap::new(),
            shutdown: CancellationToken::new(),
        }
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub fn running_count(&self) -> usize {
        self.running_jobs.len()
    }

    /// Main scheduler loop.
    pub async fn run(&self) {
        tracing::info!(
            poll_interval = ?self.config.poll_interval,
            max_concurrent = self.config.max_concurrent_jobs,
            "Scheduler started"
        );
        loop {
            tokio::select! {
                () = self.shutdown.cancelled() => {
                    tracing::info!("Scheduler shutting down");
                    break;
                }
                () = tokio::time::sleep(self.config.poll_interval) => {
                    if let Err(e) = self.tick().await {
                        tracing::error!("Scheduler tick failed: {e}");
                    }
                }
            }
        }
        self.wait_for_running_jobs().await;
        tracing::info!("Scheduler stopped");
    }

    /// Execute one polling cycle: cleanup, check cron, dispatch ready jobs.
    async fn tick(&self) -> Result<(), SchedulerError> {
        self.cleanup_finished();
        self.check_cron_jobs().await?;

        let running = self.running_jobs.len();
        let available = self.config.max_concurrent_jobs.saturating_sub(running);
        if available == 0 {
            return Ok(());
        }

        let ready_jobs = self.job_repository.find_ready_jobs(available).await?;
        for job in ready_jobs {
            if let Err(e) = self.dispatch(job).await {
                tracing::error!("Failed to dispatch job: {e}");
            }
        }

        Ok(())
    }

    /// Remove completed JoinHandles from running_jobs.
    fn cleanup_finished(&self) {
        self.running_jobs
            .retain(|_id, handle| !handle.is_finished());
    }

    /// Check for due cron job templates and spawn new jobs.
    async fn check_cron_jobs(&self) -> Result<(), SchedulerError> {
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let due_templates = self.job_repository.find_due_cron_jobs(&now).await?;

        for template in due_templates {
            let Some(cron_expr) = &template.cron_expr else {
                tracing::warn!("Cron job {} missing cron expression", template.id);
                continue;
            };

            // Calculate next scheduled time
            let next_time = match self.next_cron_time(cron_expr) {
                Ok(time) => time,
                Err(e) => {
                    tracing::error!(
                        "Failed to parse cron expression for job {}: {}",
                        template.id,
                        e
                    );
                    continue;
                }
            };

            // Create new standalone job from template
            let new_job = JobRecord {
                id: JobId::new(Uuid::new_v4().to_string()),
                job_type: JobType::Standalone,
                name: template.name.clone(),
                status: WorkflowStatus::Pending,
                agent_id: template.agent_id.clone(),
                context: template.context.clone(),
                prompt: template.prompt.clone(),
                thread_id: None,
                group_id: Some(template.id.as_ref().to_string()),
                depends_on: vec![],
                cron_expr: None,
                scheduled_at: Some(next_time.clone()),
                started_at: None,
                finished_at: None,
            };

            if let Err(e) = self.job_repository.create(&new_job).await {
                tracing::error!("Failed to create job from cron template: {}", e);
                continue;
            }

            // Update template's next scheduled time
            if let Err(e) = self
                .job_repository
                .update_scheduled_at(&template.id, &next_time)
                .await
            {
                tracing::error!("Failed to update scheduled time for cron template: {}", e);
            }
        }

        Ok(())
    }

    /// Dispatch and execute a job.
    async fn dispatch(&self, job: JobRecord) -> Result<(), SchedulerError> {
        // Update status to Running
        self.job_repository
            .update_status(
                &job.id,
                WorkflowStatus::Running,
                Some(&Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()),
                None,
            )
            .await
            .map_err(|e| SchedulerError::DispatchFailed {
                job_id: job.id.as_ref().to_string(),
                reason: e.to_string(),
            })?;

        // Get agent template
        let Some(agent_record) = self.agent_manager.get_template(&job.agent_id).await? else {
            let err = SchedulerError::DispatchFailed {
                job_id: job.id.as_ref().to_string(),
                reason: format!("Agent template not found: {}", job.agent_id),
            };
            let _ = self
                .job_repository
                .update_status(
                    &job.id,
                    WorkflowStatus::Failed,
                    None,
                    Some(&Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()),
                )
                .await;
            return Err(err);
        };

        // Create runtime agent
        let runtime_id = match self.agent_manager.create_agent(&agent_record).await {
            Ok(id) => id,
            Err(e) => {
                let err = SchedulerError::DispatchFailed {
                    job_id: job.id.as_ref().to_string(),
                    reason: e.to_string(),
                };
                let _ = self
                    .job_repository
                    .update_status(
                        &job.id,
                        WorkflowStatus::Failed,
                        None,
                        Some(&Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()),
                    )
                    .await;
                return Err(err);
            }
        };

        // Create thread
        let thread_id = {
            let agent = self.agent_manager.get(runtime_id);
            if let Some(agent) = agent {
                agent.create_thread(ThreadConfig::default())
            } else {
                let err = SchedulerError::DispatchFailed {
                    job_id: job.id.as_ref().to_string(),
                    reason: "Agent not found after creation".to_string(),
                };
                let _ = self
                    .job_repository
                    .update_status(
                        &job.id,
                        WorkflowStatus::Failed,
                        None,
                        Some(&Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()),
                    )
                    .await;
                return Err(err);
            }
        };

        // Update job with thread_id
        if let Err(e) = self
            .job_repository
            .update_thread_id(&job.id, &thread_id)
            .await
        {
            tracing::error!("Failed to update thread_id for job {}: {}", job.id, e);
        }

        // Spawn async task to execute the job
        let job_id = job.id.clone();
        let job_repository = self.job_repository.clone();
        let agent_manager = self.agent_manager.clone();

        let handle = tokio::spawn(async move {
            // Get agent and send message
            if let Some(agent) = agent_manager.get(runtime_id) {
                if let Some(mut thread) = agent.get_thread_mut(&thread_id) {
                    let handle = thread.send_message(job.prompt.clone()).await;
                    match handle.wait_for_result().await {
                        Ok(_output) => {
                            let _ = job_repository
                                .update_status(
                                    &job_id,
                                    WorkflowStatus::Succeeded,
                                    None,
                                    Some(&Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()),
                                )
                                .await;
                            tracing::info!("Job {} succeeded", job_id);
                        }
                        Err(e) => {
                            let _ = job_repository
                                .update_status(
                                    &job_id,
                                    WorkflowStatus::Failed,
                                    None,
                                    Some(&Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()),
                                )
                                .await;
                            tracing::error!("Job {} failed: {}", job_id, e);
                        }
                    }
                }
            }

            // Cleanup: delete the agent runtime
            let _ = agent_manager.delete(runtime_id);
        });

        // Track the running job
        self.running_jobs.insert(job.id, handle);

        Ok(())
    }

    /// Wait for all running jobs to complete during shutdown.
    async fn wait_for_running_jobs(&self) {
        // Take ownership of all handles by removing them from the map
        let mut handles = Vec::new();
        while !self.running_jobs.is_empty() {
            // Get keys and try to remove each one
            let keys: Vec<_> = self.running_jobs.iter().map(|r| r.key().clone()).collect();
            for key in keys {
                if let Some((_id, handle)) = self.running_jobs.remove(&key) {
                    handles.push(handle);
                }
            }
            // If we couldn't get any handles, break to avoid infinite loop
            if handles.is_empty() && !self.running_jobs.is_empty() {
                break;
            }
        }

        // Wait for all collected handles
        for handle in handles {
            let _ = handle.await;
        }
    }

    /// Calculate next trigger time for a cron expression.
    fn next_cron_time(&self, expr: &str) -> Result<String, String> {
        let schedule = Schedule::from_str(expr).map_err(|e| e.to_string())?;
        let next = schedule
            .upcoming(Utc)
            .next()
            .ok_or_else(|| "No next occurrence".to_string())?;
        Ok(next.format("%Y-%m-%d %H:%M:%S").to_string())
    }
}
