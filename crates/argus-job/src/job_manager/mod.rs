//! JobManager for dispatching and managing background jobs.
//!
//! Each dispatched job is tracked through a ThreadPool-managed execution thread.
//! Results are sent back through the unified pipe as ThreadEvent::JobResult.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex, Weak};

#[cfg(test)]
use argus_agent::TurnRecord;
use argus_agent::thread_bootstrap::{
    build_thread_config, cleanup_trace_dir, hydrate_turn_log_state, recover_and_validate_metadata,
};
use argus_agent::thread_trace_store::{
    ThreadTraceKind, ThreadTraceMetadata, child_thread_base_dir, find_job_thread_base_dir,
    list_direct_child_threads, persist_thread_metadata, recover_thread_metadata,
};
use argus_agent::{
    FilePlanStore, LlmThreadCompactor, Thread, ThreadBuilder, ThreadHandle, TurnCancellation,
};
use argus_protocol::llm::{ChatMessage, LlmProvider, Role};
use argus_protocol::{
    AgentId, JobRuntimeSnapshot, JobRuntimeState, JobRuntimeSummary, MailboxMessage,
    MailboxMessageType, McpToolResolver, ProviderId, ProviderResolver, SessionId, ThreadEvent,
    ThreadId, ThreadJobResult, ThreadMessage, ThreadPoolEventReason, ThreadRuntimeStatus,
};
use argus_repository::traits::{JobRepository, LlmProviderRepository, ThreadRepository};
use argus_repository::types::{
    AgentId as RepoAgentId, JobId, JobRecord, JobResult, JobStatus, JobType, ThreadRecord,
};
use argus_template::TemplateManager;
use argus_thread_pool::{RuntimeLifecycleChange, ThreadPool, ThreadPoolError};
use argus_tool::ToolManager;
use chrono::Utc;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::error::JobError;
use crate::types::{JobExecutionRequest, RecoveredChildJob};

mod binding_recovery;
mod execution;
mod mailbox_result;
mod persistence;
mod runtime_state;
mod support;
mod tracking;

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
#[derive(Clone)]
pub struct JobManager {
    thread_pool: Arc<ThreadPool>,
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
    trace_dir: PathBuf,
    mcp_tool_resolver: Arc<StdMutex<Option<Arc<dyn McpToolResolver>>>>,
    thread_repository: Option<Arc<dyn ThreadRepository>>,
    provider_repository: Option<Arc<dyn LlmProviderRepository>>,
    tracked_jobs: Arc<StdMutex<tracking::TrackedJobsStore>>,
    job_runtime_store: Arc<StdMutex<runtime_state::JobRuntimeStore>>,
    chat_mailbox_forwarder: Arc<StdMutex<Option<Arc<ChatMailboxForwarder>>>>,
    job_repository: Option<Arc<dyn JobRepository>>,
}

type ChatMailboxForwarderFuture = Pin<Box<dyn Future<Output = bool> + Send>>;
type ChatMailboxForwarder =
    dyn Fn(ThreadId, MailboxMessage) -> ChatMailboxForwarderFuture + Send + Sync;

impl fmt::Debug for JobManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobManager").finish()
    }
}

impl JobManager {
    const TERMINAL_JOB_RETENTION_LIMIT: usize = 1024;
    #[cfg(test)]
    const JOB_RESULT_SUMMARY_CHAR_LIMIT: usize = 4000;

    /// Recover direct child job execution threads dispatched by a parent thread.
    pub async fn recover_child_jobs_for_thread(
        &self,
        parent_thread_id: ThreadId,
    ) -> Result<Vec<RecoveredChildJob>, JobError> {
        self.recover_child_jobs_for_thread_inner(parent_thread_id)
            .await
    }

    /// Create a new JobManager.
    pub fn new(
        thread_pool: Arc<ThreadPool>,
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        trace_dir: PathBuf,
    ) -> Self {
        Self::new_with_repositories(
            thread_pool,
            template_manager,
            provider_resolver,
            tool_manager,
            trace_dir,
            None,
            None,
            None,
        )
    }

    /// Create a new JobManager with optional repository backing.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_persistence(
        thread_pool: Arc<ThreadPool>,
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        trace_dir: PathBuf,
        job_repository: Option<Arc<dyn JobRepository>>,
        thread_repository: Option<Arc<dyn ThreadRepository>>,
        provider_repository: Option<Arc<dyn LlmProviderRepository>>,
    ) -> Self {
        let manager = Self {
            thread_pool,
            template_manager,
            provider_resolver,
            tool_manager,
            trace_dir,
            mcp_tool_resolver: Arc::new(StdMutex::new(None)),
            thread_repository,
            provider_repository,
            tracked_jobs: Arc::new(StdMutex::new(tracking::TrackedJobsStore::default())),
            job_runtime_store: Arc::new(StdMutex::new(runtime_state::JobRuntimeStore::default())),
            chat_mailbox_forwarder: Arc::new(StdMutex::new(None)),
            job_repository,
        };
        manager.install_runtime_lifecycle_bridge();
        manager
    }

    /// Create a new JobManager wired with repository-backed persistence.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_repositories(
        thread_pool: Arc<ThreadPool>,
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        trace_dir: PathBuf,
        job_repository: Option<Arc<dyn JobRepository>>,
        thread_repository: Option<Arc<dyn ThreadRepository>>,
        provider_repository: Option<Arc<dyn LlmProviderRepository>>,
    ) -> Self {
        Self::new_with_persistence(
            thread_pool,
            template_manager,
            provider_resolver,
            tool_manager,
            trace_dir,
            job_repository,
            thread_repository,
            provider_repository,
        )
    }

    /// Return the shared unified thread pool.
    pub fn thread_pool(&self) -> Arc<ThreadPool> {
        Arc::clone(&self.thread_pool)
    }

    pub fn set_mcp_tool_resolver(&self, resolver: Option<Arc<dyn McpToolResolver>>) {
        *self
            .mcp_tool_resolver
            .lock()
            .expect("mcp resolver mutex poisoned") = resolver;
    }

    fn current_mcp_tool_resolver(&self) -> Option<Arc<dyn McpToolResolver>> {
        self.mcp_tool_resolver
            .lock()
            .expect("mcp resolver mutex poisoned")
            .clone()
    }

    fn thread_repository(&self) -> Option<Arc<dyn ThreadRepository>> {
        self.thread_repository.clone()
    }

    fn provider_repository(&self) -> Option<Arc<dyn LlmProviderRepository>> {
        self.provider_repository.clone()
    }
    /// Summarize turn output into a brief result message.
    #[cfg(test)]
    fn summarize_output(output: &TurnRecord) -> String {
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

    #[cfg(test)]
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
}

#[cfg(test)]
mod tests;
