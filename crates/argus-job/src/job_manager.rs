//! JobManager for dispatching and managing background jobs.
//!
//! Each dispatched job runs as a lightweight Turn (via TurnBuilder).
//! Results are sent back through the unified pipe as ThreadEvent::JobResult.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use argus_protocol::llm::{ChatMessage, Role};
use argus_protocol::tool::NamedTool;
use argus_protocol::{AgentId, ProviderResolver, ThreadEvent, TokenUsage};
use argus_template::TemplateManager;
use argus_tool::ToolManager;
use argus_turn::{TurnBuilder, TurnConfig, TurnOutput};
use tokio::sync::{RwLock, broadcast};

use crate::dispatch_tool::DispatchJobTool;
use crate::error::JobError;
use crate::list_subagents_tool::ListSubagentsTool;

/// Job result (internal, not exported).
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct JobResult {
    success: bool,
    message: String,
    token_usage: Option<TokenUsage>,
}

/// Manages job dispatch and lifecycle.
pub struct JobManager {
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
    jobs: Arc<RwLock<std::collections::HashMap<String, JobState>>>,
}

impl fmt::Debug for JobManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobManager")
            .field("jobs", &self.jobs)
            .finish()
    }
}

#[derive(Debug, Clone)]
struct JobState {
    status: String,
    result: Option<JobResult>,
}

impl JobManager {
    /// Create a new JobManager.
    pub fn new(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
    ) -> Self {
        Self {
            template_manager,
            provider_resolver,
            tool_manager,
            jobs: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Create a DispatchJobTool for this manager.
    pub fn create_dispatch_tool(self: Arc<Self>) -> DispatchJobTool {
        DispatchJobTool::new(self)
    }

    /// Create a ListSubagentsTool for this manager.
    pub fn create_list_subagents_tool(self: Arc<Self>) -> ListSubagentsTool {
        ListSubagentsTool::new(Arc::clone(&self.template_manager))
    }

    /// Dispatch a job (legacy API — use spawn_job_executor instead).
    ///
    /// This method is a shim that returns a placeholder job ID.
    /// The caller (dispatch_tool) should migrate to `spawn_job_executor`.
    #[allow(dead_code)]
    pub async fn dispatch(&self, _args: crate::types::JobDispatchArgs) -> Result<String, JobError> {
        Err(JobError::ExecutionFailed(
            "dispatch() is deprecated — use spawn_job_executor instead".to_string(),
        ))
    }

    /// Spawn a background job executor.
    ///
    /// Resolves the agent, builds a Turn, executes it, and sends
    /// ThreadEvent::JobResult into the pipe when done.
    pub async fn spawn_job_executor(
        &self,
        job_id: String,
        agent_id: AgentId,
        prompt: String,
        _context: Option<serde_json::Value>,
        pipe_tx: broadcast::Sender<ThreadEvent>,
    ) -> Result<(), JobError> {
        if prompt.trim().is_empty() {
            return Err(JobError::ExecutionFailed(
                "prompt cannot be empty".to_string(),
            ));
        }

        // Store initial job state
        {
            let mut jobs = self.jobs.write().await;
            jobs.insert(
                job_id.clone(),
                JobState {
                    status: "running".to_string(),
                    result: None,
                },
            );
        }

        // Capture clones for the background task
        let jobs = Arc::clone(&self.jobs);
        let template_manager = Arc::clone(&self.template_manager);
        let provider_resolver = Arc::clone(&self.provider_resolver);
        let tool_manager = Arc::clone(&self.tool_manager);
        let pipe_tx_clone = pipe_tx.clone();

        tokio::spawn(async move {
            let thread_id = format!("job-{}", job_id);

            // Resolve agent_record
            let agent_record = match template_manager.get(agent_id).await {
                Ok(Some(record)) => record,
                Ok(None) => {
                    let msg = format!("agent {} not found", agent_id.inner());
                    Self::mark_failed(&jobs, &job_id, &msg).await;
                    let _ = pipe_tx_clone.send(ThreadEvent::JobResult {
                        job_id,
                        success: false,
                        message: msg,
                        token_usage: None,
                    });
                    return;
                }
                Err(e) => {
                    let msg = format!("failed to load agent: {}", e);
                    Self::mark_failed(&jobs, &job_id, &msg).await;
                    let _ = pipe_tx_clone.send(ThreadEvent::JobResult {
                        job_id,
                        success: false,
                        message: msg,
                        token_usage: None,
                    });
                    return;
                }
            };

            // Resolve provider
            let provider = match agent_record.provider_id {
                Some(pid) => match provider_resolver.resolve(pid).await {
                    Ok(p) => p,
                    Err(e) => {
                        let msg = format!("failed to resolve provider: {}", e);
                        Self::mark_failed(&jobs, &job_id, &msg).await;
                        let _ = pipe_tx_clone.send(ThreadEvent::JobResult {
                            job_id,
                            success: false,
                            message: msg,
                            token_usage: None,
                        });
                        return;
                    }
                },
                None => match provider_resolver.default_provider().await {
                    Ok(p) => p,
                    Err(e) => {
                        let msg = format!("no provider configured: {}", e);
                        Self::mark_failed(&jobs, &job_id, &msg).await;
                        let _ = pipe_tx_clone.send(ThreadEvent::JobResult {
                            job_id,
                            success: false,
                            message: msg,
                            token_usage: None,
                        });
                        return;
                    }
                },
            };

            // Collect tools filtered by agent_record.tool_names
            let enabled_tool_names: HashSet<_> = agent_record.tool_names.iter().collect();
            let tools: Vec<Arc<dyn NamedTool>> = tool_manager
                .list_ids()
                .iter()
                .filter(|name| enabled_tool_names.contains(*name))
                .filter_map(|name| tool_manager.get(name))
                .collect();

            // Create internal stream channel for the Turn
            let (stream_tx, _stream_rx) = broadcast::channel(256);

            // Build and execute the Turn
            let turn_result = TurnBuilder::default()
                .turn_number(1)
                .thread_id(thread_id.clone())
                .messages(vec![ChatMessage::user(&prompt)])
                .provider(provider)
                .tools(tools)
                .hooks(Vec::new())
                .config(TurnConfig::new())
                .agent_record(Arc::new(agent_record))
                .stream_tx(stream_tx)
                .thread_event_tx(pipe_tx_clone.clone())
                .build()
                .map_err(|e| e.to_string());

            let output = match turn_result {
                Ok(turn) => turn.execute().await,
                Err(e) => {
                    let msg = format!("failed to build turn: {}", e);
                    Self::mark_failed(&jobs, &job_id, &msg).await;
                    let _ = pipe_tx_clone.send(ThreadEvent::JobResult {
                        job_id,
                        success: false,
                        message: msg,
                        token_usage: None,
                    });
                    return;
                }
            };

            match output {
                Ok(o) => {
                    let message = Self::summarize_output(&o);
                    if let Some(state) = jobs.write().await.get_mut(&job_id) {
                        state.status = "completed".to_string();
                        state.result = Some(JobResult {
                            success: true,
                            message: message.clone(),
                            token_usage: Some(o.token_usage.clone()),
                        });
                    }
                    let _ = pipe_tx_clone.send(ThreadEvent::JobResult {
                        job_id,
                        success: true,
                        message,
                        token_usage: Some(o.token_usage),
                    });
                }
                Err(e) => {
                    let msg = e.to_string();
                    Self::mark_failed(&jobs, &job_id, &msg).await;
                    let _ = pipe_tx_clone.send(ThreadEvent::JobResult {
                        job_id,
                        success: false,
                        message: msg,
                        token_usage: None,
                    });
                }
            }
        });

        Ok(())
    }

    async fn mark_failed(
        jobs: &Arc<RwLock<std::collections::HashMap<String, JobState>>>,
        job_id: &str,
        message: &str,
    ) {
        let mut guard = jobs.write().await;
        if let Some(state) = guard.get_mut(job_id) {
            state.status = "failed".to_string();
            state.result = Some(JobResult {
                success: false,
                message: message.to_string(),
                token_usage: None,
            });
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
            {
                if !content.is_empty() {
                    if content.len() > 500 {
                        return format!("{}...", &content[..500]);
                    }
                    return content.clone();
                }
            }
        }
        format!("job completed, {} messages in turn", output.messages.len())
    }
}
