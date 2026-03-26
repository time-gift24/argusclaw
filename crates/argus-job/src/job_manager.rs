//! JobManager for dispatching and managing background jobs.
//!
//! Each dispatched job runs as a lightweight Turn (via TurnBuilder).
//! Results are sent back through the unified pipe as ThreadEvent::JobResult.

use std::any::Any;
use std::collections::HashSet;
use std::fmt;
use std::panic::AssertUnwindSafe;
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
use futures_util::FutureExt;
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
    const JOB_RESULT_SUMMARY_CHAR_LIMIT: usize = 4000;

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
            let fallback_job_id = job_id.clone();
            let fallback_display_name = format!("Agent {}", agent_id.inner());
            let result = AssertUnwindSafe(Self::execute_job(
                template_manager,
                provider_resolver,
                tool_manager,
                originating_thread_id,
                job_id,
                agent_id,
                prompt,
                pipe_tx_clone.clone(),
                control_tx_clone.clone(),
            ))
            .catch_unwind()
            .await;

            let result = match result {
                Ok(result) => result,
                Err(payload) => Self::failure_result(
                    fallback_job_id,
                    agent_id,
                    fallback_display_name,
                    String::new(),
                    Self::panic_message(payload),
                ),
            };

            Self::emit_job_result(
                &pipe_tx_clone,
                &control_tx_clone,
                originating_thread_id,
                result,
            );
        });

        Ok(())
    }

    async fn execute_job(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        originating_thread_id: ThreadId,
        job_id: String,
        agent_id: AgentId,
        prompt: String,
        pipe_tx: broadcast::Sender<ThreadEvent>,
        control_tx: mpsc::UnboundedSender<ThreadControlEvent>,
    ) -> ThreadJobResult {
        let thread_id = format!("job-{}", job_id);
        let default_display_name = format!("Agent {}", agent_id.inner());

        let agent_record = match template_manager.get(agent_id).await {
            Ok(Some(record)) => record,
            Ok(None) => {
                return Self::failure_result(
                    job_id,
                    agent_id,
                    default_display_name,
                    String::new(),
                    format!("agent {} not found", agent_id.inner()),
                );
            }
            Err(e) => {
                return Self::failure_result(
                    job_id,
                    agent_id,
                    default_display_name,
                    String::new(),
                    format!("failed to load agent: {}", e),
                );
            }
        };
        let agent_display_name = agent_record.display_name.clone();
        let agent_description = agent_record.description.clone();

        let provider = match agent_record.provider_id {
            Some(pid) => match provider_resolver.resolve(pid).await {
                Ok(p) => p,
                Err(e) => {
                    return Self::failure_result(
                        job_id,
                        agent_id,
                        agent_display_name.clone(),
                        agent_description.clone(),
                        format!("failed to resolve provider: {}", e),
                    );
                }
            },
            None => match provider_resolver.default_provider().await {
                Ok(p) => p,
                Err(e) => {
                    return Self::failure_result(
                        job_id,
                        agent_id,
                        agent_display_name.clone(),
                        agent_description.clone(),
                        format!("no provider configured: {}", e),
                    );
                }
            },
        };

        let enabled_tool_names: HashSet<_> = agent_record.tool_names.iter().collect();
        let tools: Vec<Arc<dyn NamedTool>> = tool_manager
            .list_ids()
            .iter()
            .filter(|name| enabled_tool_names.contains(*name))
            .filter_map(|name| tool_manager.get(name))
            .collect();

        let (stream_tx, _stream_rx) = broadcast::channel(256);

        let turn = match TurnBuilder::default()
            .turn_number(1)
            .thread_id(thread_id)
            .messages(vec![ChatMessage::user(&prompt)])
            .provider(provider)
            .tools(tools)
            .hooks(Vec::new())
            .config(TurnConfig::new())
            .agent_record(Arc::new(agent_record))
            .stream_tx(stream_tx)
            .thread_event_tx(pipe_tx)
            .originating_thread_id(originating_thread_id)
            .control_tx(control_tx)
            .mailbox(Arc::new(Mutex::new(ThreadMailbox::default())))
            .build()
        {
            Ok(turn) => turn,
            Err(e) => {
                return Self::failure_result(
                    job_id,
                    agent_id,
                    agent_display_name.clone(),
                    agent_description.clone(),
                    format!("failed to build turn: {}", e),
                );
            }
        };

        match turn.execute().await {
            Ok(output) => ThreadJobResult {
                job_id,
                success: true,
                message: Self::summarize_output(&output),
                token_usage: Some(output.token_usage),
                agent_id,
                agent_display_name,
                agent_description,
            },
            Err(e) => Self::failure_result(
                job_id,
                agent_id,
                agent_display_name,
                agent_description,
                e.to_string(),
            ),
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
                && !content.is_empty() {
                    return Self::truncate_summary(content);
                }
        }
        format!("job completed, {} messages in turn", output.messages.len())
    }

    fn truncate_summary(content: &str) -> String {
        let mut chars = content.chars();
        let summary: String = chars
            .by_ref()
            .take(Self::JOB_RESULT_SUMMARY_CHAR_LIMIT)
            .collect();
        if chars.next().is_some() {
            format!("{summary}...")
        } else {
            content.to_string()
        }
    }

    fn failure_result(
        job_id: String,
        agent_id: AgentId,
        agent_display_name: String,
        agent_description: String,
        message: String,
    ) -> ThreadJobResult {
        ThreadJobResult {
            job_id,
            success: false,
            message,
            token_usage: None,
            agent_id,
            agent_display_name,
            agent_description,
        }
    }

    fn panic_message(payload: Box<dyn Any + Send>) -> String {
        let payload = payload.as_ref();
        let detail = payload
            .downcast_ref::<&'static str>()
            .map(|msg| (*msg).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic payload".to_string());
        format!("job executor panicked: {detail}")
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

#[cfg(test)]
mod tests {
    use argus_protocol::TokenUsage;

    use super::*;

    fn assistant_output(content: &str) -> TurnOutput {
        TurnOutput {
            messages: vec![ChatMessage::assistant(content)],
            token_usage: TokenUsage::default(),
        }
    }

    #[test]
    fn summarize_output_handles_unicode_boundaries() {
        let content = format!("{}数{}", "a".repeat(498), "b".repeat(5000));

        let summary = JobManager::summarize_output(&assistant_output(&content));

        assert!(summary.ends_with("..."));
        assert_eq!(summary.chars().count(), JobManager::JOB_RESULT_SUMMARY_CHAR_LIMIT + 3);
        assert!(summary.contains('数'));
    }

    #[test]
    fn summarize_output_keeps_reports_longer_than_legacy_limit() {
        let content = "x".repeat(800);

        let summary = JobManager::summarize_output(&assistant_output(&content));

        assert_eq!(summary, content);
    }
}
