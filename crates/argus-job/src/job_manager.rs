//! JobManager for dispatching and managing background jobs.
//!
//! Each dispatched job runs as a lightweight Turn (via TurnBuilder).
//! Results are sent back through the unified pipe as ThreadEvent::JobResult.

use std::any::Any;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex as StdMutex};

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
use crate::get_job_result_tool::GetJobResultTool;
use crate::list_subagents_tool::ListSubagentsTool;

#[derive(Debug, Clone)]
enum TrackedJobState {
    Pending,
    Completed(ThreadJobResult),
    Consumed(ThreadJobResult),
}

#[derive(Debug, Clone)]
struct TrackedJob {
    thread_id: ThreadId,
    state: TrackedJobState,
}

/// Result of looking up a background job for a specific thread.
#[derive(Debug, Clone)]
pub enum JobLookup {
    /// Job was never seen for this thread.
    NotFound,
    /// Job was dispatched but has not completed yet.
    Pending,
    /// Job completed and the result is still available for consumption.
    Completed(ThreadJobResult),
    /// Job result was already consumed proactively.
    Consumed(ThreadJobResult),
}

/// Manages job dispatch and lifecycle.
pub struct JobManager {
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
    tracked_jobs: Arc<StdMutex<HashMap<String, TrackedJob>>>,
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
            tracked_jobs: Arc::new(StdMutex::new(HashMap::new())),
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

    /// Create a GetJobResultTool for this manager.
    pub fn create_get_job_result_tool(self: Arc<Self>) -> GetJobResultTool {
        GetJobResultTool::new(self)
    }

    /// Record that a job was dispatched for a thread.
    pub fn record_dispatched_job(&self, thread_id: ThreadId, job_id: String) {
        Self::record_dispatched_job_in_store(&self.tracked_jobs, thread_id, job_id);
    }

    /// Record the completed result for a job.
    pub fn record_completed_job_result(&self, thread_id: ThreadId, result: ThreadJobResult) {
        Self::record_completed_job_result_in_store(&self.tracked_jobs, thread_id, result);
    }

    /// Get the current tracked status for a job scoped to its originating thread.
    pub fn get_job_result_status(
        &self,
        thread_id: ThreadId,
        job_id: &str,
        consume: bool,
    ) -> JobLookup {
        let mut tracked_jobs = self
            .tracked_jobs
            .lock()
            .expect("job tracking mutex poisoned");
        let Some(tracked_job) = tracked_jobs.get_mut(job_id) else {
            return JobLookup::NotFound;
        };

        if tracked_job.thread_id != thread_id {
            return JobLookup::NotFound;
        }

        match &tracked_job.state {
            TrackedJobState::Pending => JobLookup::Pending,
            TrackedJobState::Completed(result) => {
                let result = result.clone();
                if consume {
                    tracked_job.state = TrackedJobState::Consumed(result.clone());
                }
                JobLookup::Completed(result)
            }
            TrackedJobState::Consumed(result) => JobLookup::Consumed(result.clone()),
        }
    }

    /// Spawn a background job executor.
    ///
    /// Resolves the agent, builds a Turn, executes it, and sends
    /// ThreadEvent::JobResult into the pipe when done.
    #[allow(clippy::too_many_arguments)]
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

        self.record_dispatched_job(originating_thread_id, job_id.clone());

        // ThreadId is Copy — captured into async block directly
        let template_manager = Arc::clone(&self.template_manager);
        let provider_resolver = Arc::clone(&self.provider_resolver);
        let tool_manager = Arc::clone(&self.tool_manager);
        let tracked_jobs = Arc::clone(&self.tracked_jobs);
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

            Self::forward_job_result_to_runtime(&control_tx_clone, result.clone());
            Self::record_completed_job_result_in_store(
                &tracked_jobs,
                originating_thread_id,
                result.clone(),
            );
            Self::broadcast_job_result(&pipe_tx_clone, originating_thread_id, result);
        });

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
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
                && !content.is_empty()
            {
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

    fn record_dispatched_job_in_store(
        tracked_jobs: &Arc<StdMutex<HashMap<String, TrackedJob>>>,
        thread_id: ThreadId,
        job_id: String,
    ) {
        tracked_jobs
            .lock()
            .expect("job tracking mutex poisoned")
            .insert(
                job_id,
                TrackedJob {
                    thread_id,
                    state: TrackedJobState::Pending,
                },
            );
    }

    fn record_completed_job_result_in_store(
        tracked_jobs: &Arc<StdMutex<HashMap<String, TrackedJob>>>,
        thread_id: ThreadId,
        result: ThreadJobResult,
    ) {
        tracked_jobs
            .lock()
            .expect("job tracking mutex poisoned")
            .insert(
                result.job_id.clone(),
                TrackedJob {
                    thread_id,
                    state: TrackedJobState::Completed(result),
                },
            );
    }

    fn forward_job_result_to_runtime(
        control_tx: &mpsc::UnboundedSender<ThreadControlEvent>,
        result: ThreadJobResult,
    ) {
        let _ = control_tx.send(ThreadControlEvent::JobResult(result));
    }

    fn broadcast_job_result(
        pipe_tx: &broadcast::Sender<ThreadEvent>,
        originating_thread_id: ThreadId,
        result: ThreadJobResult,
    ) {
        let _ = pipe_tx.send(ThreadEvent::JobResult {
            thread_id: originating_thread_id,
            job_id: result.job_id,
            success: result.success,
            message: result.message,
            token_usage: result.token_usage,
            agent_id: result.agent_id,
            agent_display_name: result.agent_display_name,
            agent_description: result.agent_description,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use argus_protocol::{LlmProvider, ProviderId};
    use argus_template::TemplateManager;
    use argus_repository::traits::AgentRepository;
    use argus_repository::ArgusSqlite;
    use async_trait::async_trait;
    use sqlx::SqlitePool;

    use argus_protocol::TokenUsage;
    use argus_tool::ToolManager;

    use super::*;

    #[derive(Debug)]
    struct DummyProviderResolver;

    #[async_trait]
    impl ProviderResolver for DummyProviderResolver {
        async fn resolve(&self, _id: ProviderId) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            unreachable!("resolver should not be called in tracking tests");
        }

        async fn default_provider(&self) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            unreachable!("resolver should not be called in tracking tests");
        }

        async fn resolve_with_model(
            &self,
            _id: ProviderId,
            _model: &str,
        ) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            unreachable!("resolver should not be called in tracking tests");
        }
    }

    fn test_job_manager() -> JobManager {
        let pool = SqlitePool::connect_lazy("sqlite::memory:")
            .expect("lazy sqlite pool should build for tests");
        let sqlite = Arc::new(ArgusSqlite::new(pool));
        JobManager::new(
            Arc::new(TemplateManager::new(
                sqlite.clone() as Arc<dyn AgentRepository>,
                sqlite.clone(),
            )),
            Arc::new(DummyProviderResolver),
            Arc::new(ToolManager::new()),
        )
    }

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
        assert_eq!(
            summary.chars().count(),
            JobManager::JOB_RESULT_SUMMARY_CHAR_LIMIT + 3
        );
        assert!(summary.contains('数'));
    }

    #[test]
    fn summarize_output_keeps_reports_longer_than_legacy_limit() {
        let content = "x".repeat(800);

        let summary = JobManager::summarize_output(&assistant_output(&content));

        assert_eq!(summary, content);
    }

    #[tokio::test]
    async fn tracked_job_status_moves_from_pending_to_completed_to_consumed() {
        let manager = test_job_manager();
        let thread_id = ThreadId::new();
        let result = ThreadJobResult {
            job_id: "job-42".to_string(),
            success: true,
            message: "all done".to_string(),
            token_usage: None,
            agent_id: AgentId::new(9),
            agent_display_name: "Researcher".to_string(),
            agent_description: "Looks things up".to_string(),
        };

        manager.record_dispatched_job(thread_id, result.job_id.clone());
        assert!(matches!(
            manager.get_job_result_status(thread_id, &result.job_id, false),
            JobLookup::Pending
        ));

        manager.record_completed_job_result(thread_id, result.clone());
        assert!(matches!(
            manager.get_job_result_status(thread_id, &result.job_id, false),
            JobLookup::Completed(found) if found.job_id == result.job_id
        ));

        assert!(matches!(
            manager.get_job_result_status(thread_id, &result.job_id, true),
            JobLookup::Completed(found) if found.job_id == result.job_id
        ));

        assert!(matches!(
            manager.get_job_result_status(thread_id, &result.job_id, false),
            JobLookup::Consumed(found) if found.job_id == result.job_id
        ));
    }
}
