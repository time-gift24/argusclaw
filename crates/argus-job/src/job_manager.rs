//! JobManager for dispatching and managing background jobs.
//!
//! Each dispatched job runs as a lightweight Turn (via TurnBuilder).
//! Results are sent back through the unified pipe as ThreadEvent::JobResult.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use argus_protocol::llm::{ChatMessage, Role};
use argus_protocol::tool::NamedTool;
use argus_protocol::{
    AgentId, ProviderResolver, ThreadControlEvent, ThreadEvent, ThreadId, ThreadJobResult,
    ThreadMailbox,
};
use argus_template::TemplateManager;
use argus_tool::ToolManager;
use argus_turn::{TurnBuilder, TurnConfig, TurnOutput};
use tokio::sync::{Mutex, broadcast, mpsc};

use crate::dispatch_tool::DispatchJobTool;
use crate::error::JobError;
use crate::list_subagents_tool::ListSubagentsTool;

/// Manages job dispatch and lifecycle.
pub struct JobManager {
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
}

impl fmt::Debug for JobManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobManager").finish()
    }
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

    /// Spawn a background job executor.
    ///
    /// Resolves the agent, builds a Turn, executes it, and sends
    /// ThreadEvent::JobResult into the pipe when done.
    pub async fn spawn_job_executor(
        &self,
        originating_thread_id: ThreadId,
        job_id: String,
        agent_id: AgentId,
        prompt: String,
        _context: Option<serde_json::Value>,
        pipe_tx: broadcast::Sender<ThreadEvent>,
        control_tx: mpsc::UnboundedSender<ThreadControlEvent>,
    ) -> Result<(), JobError> {
        if prompt.trim().is_empty() {
            return Err(JobError::ExecutionFailed(
                "prompt cannot be empty".to_string(),
            ));
        }

        // ThreadId is Copy — captured into async block directly
        let template_manager = Arc::clone(&self.template_manager);
        let provider_resolver = Arc::clone(&self.provider_resolver);
        let tool_manager = Arc::clone(&self.tool_manager);
        let pipe_tx_clone = pipe_tx.clone();
        let control_tx_clone = control_tx.clone();

        tokio::spawn(async move {
            let thread_id = format!("job-{}", job_id);
            let default_display_name = format!("Agent {}", agent_id.inner());

            // Resolve agent_record
            let agent_record = match template_manager.get(agent_id).await {
                Ok(Some(record)) => record,
                Ok(None) => {
                    let msg = format!("agent {} not found", agent_id.inner());
                    Self::emit_job_result(
                        &pipe_tx_clone,
                        &control_tx_clone,
                        originating_thread_id,
                        ThreadJobResult {
                            job_id,
                            success: false,
                            message: msg,
                            token_usage: None,
                            agent_id,
                            agent_display_name: default_display_name,
                            agent_description: String::new(),
                        },
                    );
                    return;
                }
                Err(e) => {
                    let msg = format!("failed to load agent: {}", e);
                    Self::emit_job_result(
                        &pipe_tx_clone,
                        &control_tx_clone,
                        originating_thread_id,
                        ThreadJobResult {
                            job_id,
                            success: false,
                            message: msg,
                            token_usage: None,
                            agent_id,
                            agent_display_name: default_display_name,
                            agent_description: String::new(),
                        },
                    );
                    return;
                }
            };
            let agent_display_name = agent_record.display_name.clone();
            let agent_description = agent_record.description.clone();

            // Resolve provider
            let provider = match agent_record.provider_id {
                Some(pid) => match provider_resolver.resolve(pid).await {
                    Ok(p) => p,
                    Err(e) => {
                        let msg = format!("failed to resolve provider: {}", e);
                        Self::emit_job_result(
                            &pipe_tx_clone,
                            &control_tx_clone,
                            originating_thread_id,
                            ThreadJobResult {
                                job_id,
                                success: false,
                                message: msg,
                                token_usage: None,
                                agent_id,
                                agent_display_name: agent_display_name.clone(),
                                agent_description: agent_description.clone(),
                            },
                        );
                        return;
                    }
                },
                None => match provider_resolver.default_provider().await {
                    Ok(p) => p,
                    Err(e) => {
                        let msg = format!("no provider configured: {}", e);
                        Self::emit_job_result(
                            &pipe_tx_clone,
                            &control_tx_clone,
                            originating_thread_id,
                            ThreadJobResult {
                                job_id,
                                success: false,
                                message: msg,
                                token_usage: None,
                                agent_id,
                                agent_display_name: agent_display_name.clone(),
                                agent_description: agent_description.clone(),
                            },
                        );
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
                .originating_thread_id(originating_thread_id)
                .control_tx(control_tx_clone.clone())
                .mailbox(Arc::new(Mutex::new(ThreadMailbox::default())))
                .build()
                .map_err(|e| e.to_string());

            let output = match turn_result {
                Ok(turn) => turn.execute().await,
                Err(e) => {
                    let msg = format!("failed to build turn: {}", e);
                    Self::emit_job_result(
                        &pipe_tx_clone,
                        &control_tx_clone,
                        originating_thread_id,
                        ThreadJobResult {
                            job_id,
                            success: false,
                            message: msg,
                            token_usage: None,
                            agent_id,
                            agent_display_name: agent_display_name.clone(),
                            agent_description: agent_description.clone(),
                        },
                    );
                    return;
                }
            };

            match output {
                Ok(o) => {
                    let message = Self::summarize_output(&o);
                    Self::emit_job_result(
                        &pipe_tx_clone,
                        &control_tx_clone,
                        originating_thread_id,
                        ThreadJobResult {
                            job_id,
                            success: true,
                            message,
                            token_usage: Some(o.token_usage),
                            agent_id,
                            agent_display_name: agent_display_name,
                            agent_description,
                        },
                    );
                }
                Err(e) => {
                    let msg = e.to_string();
                    Self::emit_job_result(
                        &pipe_tx_clone,
                        &control_tx_clone,
                        originating_thread_id,
                        ThreadJobResult {
                            job_id,
                            success: false,
                            message: msg,
                            token_usage: None,
                            agent_id,
                            agent_display_name,
                            agent_description,
                        },
                    );
                }
            }
        });

        Ok(())
    }

    /// Summarize turn output into a brief result message.
    fn summarize_output(output: &TurnOutput) -> String {
        for msg in output.messages.iter().rev() {
            if let ChatMessage {
                role: Role::Assistant,
                content,
                ..
            } = msg
                && !content.is_empty() {
                    if content.len() > 500 {
                        return format!("{}...", &content[..500]);
                    }
                    return content.clone();
                }
        }
        format!("job completed, {} messages in turn", output.messages.len())
    }

    fn emit_job_result(
        pipe_tx: &broadcast::Sender<ThreadEvent>,
        control_tx: &mpsc::UnboundedSender<ThreadControlEvent>,
        originating_thread_id: ThreadId,
        result: ThreadJobResult,
    ) {
        let public_result = result.clone();
        let _ = pipe_tx.send(ThreadEvent::JobResult {
            thread_id: originating_thread_id,
            job_id: public_result.job_id,
            success: public_result.success,
            message: public_result.message,
            token_usage: public_result.token_usage,
            agent_id: public_result.agent_id,
            agent_display_name: public_result.agent_display_name,
            agent_description: public_result.agent_description,
        });
        let _ = control_tx.send(ThreadControlEvent::JobResult(result));
    }
}
