//! JobManager for dispatching and managing background jobs.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use argus_protocol::llm::{ChatMessage, LlmProvider, Role};
use argus_protocol::tool::NamedTool;
use argus_protocol::{AgentRecord, ProviderResolver};
use argus_template::TemplateManager;
use argus_tool::ToolManager;
use argus_turn::{TurnBuilder, TurnConfig, TurnOutput};
use sqlx::SqlitePool;
use tokio::sync::{RwLock, broadcast};
use tokio::time::{Instant, sleep};
use uuid::Uuid;

use crate::dispatch_tool::DispatchJobTool;
use crate::error::JobError;
use crate::get_job_result_tool::GetJobResultTool;
use crate::list_subagents_tool::ListSubagentsTool;
use crate::sse_broadcaster::SseBroadcaster;
use crate::types::{JobDispatchArgs, JobDispatchResult, JobResult};

/// Manages job dispatch and lifecycle.
pub struct JobManager {
    pool: SqlitePool,
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
    jobs: Arc<RwLock<std::collections::HashMap<String, JobState>>>,
    broadcaster: Arc<SseBroadcaster>,
}

impl fmt::Debug for JobManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobManager")
            .field("jobs", &self.jobs)
            .field("broadcaster", &self.broadcaster)
            .finish()
    }
}

#[derive(Debug, Clone)]
struct JobState {
    status: String,
    result: Option<JobResult>,
}

const JOB_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const JOB_WAIT_TIMEOUT: Duration = Duration::from_secs(120);

impl JobManager {
    /// Create a new JobManager.
    pub fn new(
        pool: SqlitePool,
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
    ) -> Self {
        Self {
            pool,
            template_manager,
            provider_resolver,
            tool_manager,
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

    /// Create a ListSubagentsTool for this manager.
    pub fn create_list_subagents_tool(self: Arc<Self>) -> ListSubagentsTool {
        ListSubagentsTool::new(Arc::clone(&self.template_manager))
    }

    fn spawn_background_execution(&self, job_id: String, args: JobDispatchArgs) {
        let jobs = Arc::clone(&self.jobs);
        let broadcaster = Arc::clone(&self.broadcaster);
        let pool = self.pool.clone();
        let template_manager = Arc::clone(&self.template_manager);
        let provider_resolver = Arc::clone(&self.provider_resolver);
        let tool_manager = Arc::clone(&self.tool_manager);

        tokio::spawn(async move {
            {
                let mut guard = jobs.write().await;
                if let Some(state) = guard.get_mut(&job_id) {
                    state.status = "running".to_string();
                }
            }

            let (final_status, final_result) = match Self::execute_job(
                pool,
                &template_manager,
                &provider_resolver,
                &tool_manager,
                args,
            )
            .await
            {
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

    async fn execute_job(
        _pool: SqlitePool,
        template_manager: &Arc<TemplateManager>,
        provider_resolver: &Arc<dyn ProviderResolver>,
        tool_manager: &Arc<ToolManager>,
        args: JobDispatchArgs,
    ) -> Result<JobResult, JobError> {
        if args.prompt.trim().is_empty() {
            return Err(JobError::ExecutionFailed(
                "prompt cannot be empty".to_string(),
            ));
        }

        // 1. Look up the agent record
        let agent_record = template_manager
            .get(args.agent_id)
            .await
            .map_err(|e| JobError::ExecutionFailed(format!("failed to load agent: {e}")))?
            .ok_or(JobError::AgentNotFound(args.agent_id.inner()))?;

        // 2. Resolve the LLM provider
        let provider = match agent_record.provider_id {
            Some(provider_id) => provider_resolver.resolve(provider_id).await.map_err(|e| {
                JobError::ExecutionFailed(format!("failed to resolve provider: {e}"))
            })?,
            None => {
                // Use default provider
                provider_resolver.default_provider().await.map_err(|e| {
                    JobError::ExecutionFailed(format!("no provider configured: {e}"))
                })?
            }
        };

        Self::execute_turn_for_provider(args.prompt, agent_record, provider, tool_manager).await
    }

    async fn execute_turn_for_provider(
        prompt: String,
        agent_record: AgentRecord,
        provider: Arc<dyn LlmProvider>,
        tool_manager: &Arc<ToolManager>,
    ) -> Result<JobResult, JobError> {
        let thread_id = format!("job-{}", Uuid::new_v4());
        let turn_number = 1u32;

        // Build the initial message list: user prompt
        let messages = vec![ChatMessage::user(&prompt)];

        // Collect tools filtered by agent_record.tool_names
        let enabled_tool_names: HashSet<_> = agent_record.tool_names.iter().collect();
        let tools: Vec<Arc<dyn NamedTool>> = tool_manager
            .list_ids()
            .iter()
            .filter(|name| enabled_tool_names.contains(*name))
            .filter_map(|name| tool_manager.get(name))
            .collect();

        // Build TurnConfig with job-appropriate limits
        let config = TurnConfig::new();

        // Create broadcast channels for events (drop receivers after construction)
        let (stream_tx, _stream_rx) = broadcast::channel(256);
        let (thread_event_tx, _thread_event_rx) = broadcast::channel(256);

        // Build and execute the Turn
        let turn = TurnBuilder::default()
            .turn_number(turn_number)
            .thread_id(thread_id)
            .messages(messages)
            .provider(provider)
            .tools(tools)
            .hooks(Vec::new())
            .config(config)
            .agent_record(Arc::new(agent_record))
            .stream_tx(stream_tx)
            .thread_event_tx(thread_event_tx)
            .build()
            .map_err(|e| JobError::ExecutionFailed(format!("failed to build turn: {e}")))?;

        match turn.execute().await {
            Ok(output) => Ok(JobResult {
                success: true,
                message: Self::summarize_output(&output),
                token_usage: Some(output.token_usage),
            }),
            Err(err) => Err(JobError::TurnResult(err.to_string())),
        }
    }

    /// Summarize turn output into a brief result message.
    fn summarize_output(output: &TurnOutput) -> String {
        for msg in output.messages.iter().rev() {
            if let ChatMessage {
                role: Role::Assistant,
                content,
                ..
            } = msg
                && !content.is_empty()
            {
                if content.len() > 500 {
                    return format!("{}...", &content[..500]);
                }
                return content.clone();
            }
        }
        format!("job completed, {} messages in turn", output.messages.len())
    }
}
