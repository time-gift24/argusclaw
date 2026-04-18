use std::path::PathBuf;
use std::sync::Arc;

use argus_agent::thread_trace_store::{
    chat_thread_base_dir, persist_thread_metadata, ThreadTraceKind, ThreadTraceMetadata,
};
use argus_agent::tool_context::current_agent_id;
use argus_agent::turn_log_store::{recover_thread_log_state, RecoveredThreadLogState};
use argus_job::{JobLookup, JobManager, ThreadPool};
use argus_protocol::{
    llm::{ChatMessage, CompletionRequest, CompletionResponse, LlmError, LlmEventStream},
    AgentId, ArgusError, LlmProviderId, MailboxMessage, MailboxMessageType, McpToolResolver,
    ProviderId, Result, SessionId, ThreadEvent, ThreadId, ThreadMessage, ThreadPoolRuntimeKind,
    ToolError,
};
use argus_repository::traits::{LlmProviderRepository, SessionRepository, ThreadRepository};
use argus_template::TemplateManager;
use argus_tool::{
    SchedulerBackend, SchedulerDispatchRequest, SchedulerJobLookup, SchedulerJobResult,
    SchedulerLookupRequest, SchedulerSubagent, SchedulerTool, SendMessageRequest,
    SendMessageResponse, ToolManager, MAX_DISPATCH_DEPTH,
};
use async_trait::async_trait;
use chrono::Utc;
use dashmap::DashMap;
use rust_decimal::Decimal;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::session::{Session, SessionSummary, ThreadSummary};
use argus_protocol::ProviderResolver;

#[derive(Debug)]
struct RecoveredThreadState {
    messages: Vec<ChatMessage>,
    turn_count: u32,
    token_count: u32,
}

#[derive(Debug)]
struct UnconfiguredProvider {
    reason: String,
}

impl UnconfiguredProvider {
    fn new(reason: String) -> Self {
        Self { reason }
    }

    fn llm_error(&self) -> LlmError {
        LlmError::RequestFailed {
            provider: "unconfigured-default".to_string(),
            reason: self.reason.clone(),
        }
    }
}

#[async_trait]
impl argus_protocol::LlmProvider for UnconfiguredProvider {
    fn model_name(&self) -> &str {
        "unconfigured-default"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> std::result::Result<CompletionResponse, LlmError> {
        Err(self.llm_error())
    }

    async fn stream_complete(
        &self,
        _request: CompletionRequest,
    ) -> std::result::Result<LlmEventStream, LlmError> {
        Err(self.llm_error())
    }
}

#[derive(Clone)]
struct SessionSchedulerBackend {
    template_manager: Arc<TemplateManager>,
    job_manager: Arc<JobManager>,
    sessions: Arc<DashMap<SessionId, Arc<Session>>>,
}

impl std::fmt::Debug for SessionSchedulerBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionSchedulerBackend").finish()
    }
}

impl SessionSchedulerBackend {
    fn new(
        template_manager: Arc<TemplateManager>,
        job_manager: Arc<JobManager>,
        sessions: Arc<DashMap<SessionId, Arc<Session>>>,
    ) -> Self {
        Self {
            template_manager,
            job_manager,
            sessions,
        }
    }

    fn scheduler_error(reason: impl Into<String>) -> ToolError {
        ToolError::ExecutionFailed {
            tool_name: "scheduler".to_string(),
            reason: reason.into(),
        }
    }

    fn thread_pool(&self) -> Arc<ThreadPool> {
        self.job_manager.thread_pool()
    }

    fn map_job_lookup(lookup: JobLookup) -> SchedulerJobLookup {
        match lookup {
            JobLookup::NotFound => SchedulerJobLookup::NotFound,
            JobLookup::Pending => SchedulerJobLookup::Pending,
            JobLookup::Completed(result) => SchedulerJobLookup::Completed(SchedulerJobResult {
                success: result.success,
                message: result.message,
                token_usage: result.token_usage,
                agent_id: result.agent_id,
                agent_display_name: result.agent_display_name,
                agent_description: result.agent_description,
            }),
            JobLookup::Consumed(result) => SchedulerJobLookup::Consumed(SchedulerJobResult {
                success: result.success,
                message: result.message,
                token_usage: result.token_usage,
                agent_id: result.agent_id,
                agent_display_name: result.agent_display_name,
                agent_description: result.agent_description,
            }),
        }
    }

    async fn claim_queued_runtime_result(
        &self,
        thread_id: ThreadId,
        job_id: &str,
    ) -> std::result::Result<(), ToolError> {
        let thread_pool = self.thread_pool();
        if thread_pool.runtime_summary(&thread_id).is_none() {
            tracing::warn!(job_id, thread_id = %thread_id, "failed to claim queued job result: thread not registered");
            return Ok(());
        }

        if thread_pool
            .claim_queued_job_result(thread_id, job_id)
            .is_none()
        {
            tracing::debug!(
                job_id,
                thread_id = %thread_id,
                "claim queued job result missed because the mailbox item was already absent"
            );
        }

        Ok(())
    }

    async fn source_label(&self, thread_id: ThreadId) -> String {
        let thread_pool = self.thread_pool();
        let Some(thread) = thread_pool.loaded_thread(&thread_id) else {
            return format!("Thread {}", thread_id);
        };

        let guard = thread.read().await;
        guard.agent_record().display_name.clone()
    }

    async fn current_scheduler_agent(
        &self,
    ) -> std::result::Result<argus_protocol::AgentRecord, ToolError> {
        let agent_id =
            current_agent_id().ok_or_else(|| Self::scheduler_error("no current agent context"))?;
        self.template_manager
            .get(agent_id)
            .await
            .map_err(|error| Self::scheduler_error(error.to_string()))?
            .ok_or_else(|| Self::scheduler_error("agent not found"))
    }

    async fn dispatch_depth_for_thread(
        &self,
        thread_id: ThreadId,
    ) -> std::result::Result<u32, ToolError> {
        let thread_pool = self.thread_pool();
        let mut depth = 0;
        let mut cursor = thread_id;

        while let Some(parent_thread_id) = thread_pool.parent_thread_id(&cursor).or(thread_pool
            .recover_parent_thread_id(&cursor)
            .await
            .map_err(|error| Self::scheduler_error(error.to_string()))?)
        {
            depth += 1;
            cursor = parent_thread_id;
        }

        Ok(depth)
    }

    async fn active_child_thread_ids(
        &self,
        thread_id: ThreadId,
    ) -> std::result::Result<Vec<ThreadId>, ToolError> {
        let thread_pool = self.thread_pool();
        let children = thread_pool
            .recover_child_jobs(thread_id)
            .await
            .map_err(|error| Self::scheduler_error(error.to_string()))?;
        let mut active = Vec::new();
        for child in children {
            if self
                .job_manager
                .is_job_pending_persisted(&child.job_id)
                .await
                .map_err(|error| Self::scheduler_error(error.to_string()))?
            {
                active.push(child.thread_id);
            }
        }
        Ok(active)
    }

    async fn mailbox_ready_child_thread_ids(
        &self,
        thread_id: ThreadId,
    ) -> std::result::Result<Vec<ThreadId>, ToolError> {
        let thread_pool = self.thread_pool();
        Ok(self
            .active_child_thread_ids(thread_id)
            .await?
            .into_iter()
            .filter(|child_thread_id| thread_pool.loaded_thread(child_thread_id).is_some())
            .collect())
    }

    async fn is_thread_target_reachable(
        &self,
        source_thread_id: ThreadId,
        target_thread_id: ThreadId,
    ) -> std::result::Result<bool, ToolError> {
        if source_thread_id == target_thread_id {
            return Ok(true);
        }

        let thread_pool = self.thread_pool();
        let parent = match thread_pool.parent_thread_id(&source_thread_id) {
            Some(parent) => Some(parent),
            None => thread_pool
                .recover_parent_thread_id(&source_thread_id)
                .await
                .map_err(|error| Self::scheduler_error(error.to_string()))?,
        };
        if parent == Some(target_thread_id) {
            return Ok(true);
        }

        Ok(thread_pool
            .recover_child_jobs(source_thread_id)
            .await
            .map_err(|error| Self::scheduler_error(error.to_string()))?
            .into_iter()
            .any(|child| child.thread_id == target_thread_id))
    }

    async fn validate_mailbox_target_ready(
        &self,
        target_thread_id: ThreadId,
    ) -> std::result::Result<(), ToolError> {
        let thread_pool = self.thread_pool();
        let Some(summary) = thread_pool.runtime_summary(&target_thread_id) else {
            if thread_pool
                .recover_parent_thread_id(&target_thread_id)
                .await
                .map_err(|error| Self::scheduler_error(error.to_string()))?
                .is_some()
            {
                return Err(Self::scheduler_error(format!(
                    "thread {target_thread_id} is not ready to receive mailbox messages"
                )));
            }
            return Err(Self::scheduler_error(format!(
                "thread {target_thread_id} is not registered"
            )));
        };

        if summary.kind == ThreadPoolRuntimeKind::Job
            && thread_pool.loaded_thread(&target_thread_id).is_none()
        {
            return Err(Self::scheduler_error(format!(
                "thread {target_thread_id} is not ready to receive mailbox messages"
            )));
        }

        Ok(())
    }

    async fn resolve_agent_name_target(
        &self,
        thread_id: ThreadId,
        agent_name: &str,
    ) -> std::result::Result<ThreadId, ToolError> {
        let thread_pool = self.thread_pool();
        let mut matches = Vec::new();

        for child_thread_id in self.mailbox_ready_child_thread_ids(thread_id).await? {
            let Some(thread) = thread_pool.loaded_thread(&child_thread_id) else {
                continue;
            };
            let display_name = {
                let guard = thread.read().await;
                guard.agent_record().display_name.clone()
            };
            if display_name == agent_name {
                matches.push(child_thread_id);
            }
        }

        match matches.as_slice() {
            [thread_id] => Ok(*thread_id),
            [] => Err(Self::scheduler_error(format!(
                "no active direct child agent named {agent_name}"
            ))),
            _ => Err(Self::scheduler_error(format!(
                "agent name {agent_name} is ambiguous; use job:<job_id> or thread:<thread_id>"
            ))),
        }
    }

    async fn resolve_message_targets(
        &self,
        thread_id: ThreadId,
        to: &str,
    ) -> std::result::Result<Vec<ThreadId>, ToolError> {
        let thread_pool = self.thread_pool();

        if let Some(job_id) = to.strip_prefix("job:") {
            if !matches!(
                self.job_manager
                    .get_job_result_status_persisted(thread_id, job_id, false)
                    .await
                    .map_err(|error| Self::scheduler_error(error.to_string()))?,
                JobLookup::Pending
            ) {
                return Err(Self::scheduler_error(format!("job {job_id} is not active")));
            }
            let target = thread_pool
                .recover_thread_binding(job_id)
                .await
                .map_err(|error| Self::scheduler_error(error.to_string()))?
                .ok_or_else(|| {
                    Self::scheduler_error(format!("job {job_id} is not bound to a thread"))
                })?;
            self.validate_mailbox_target_ready(target)
                .await
                .map_err(|_| {
                    Self::scheduler_error(format!(
                        "job {job_id} is not ready to receive mailbox messages"
                    ))
                })?;
            return Ok(vec![target]);
        }

        if let Some(thread_id_str) = to.strip_prefix("thread:") {
            let target = ThreadId::parse(thread_id_str).map_err(|error| {
                Self::scheduler_error(format!("invalid thread target {thread_id_str}: {error}"))
            })?;
            if !self.is_thread_target_reachable(thread_id, target).await? {
                return Err(Self::scheduler_error(format!(
                    "thread {target} is not reachable from the current thread"
                )));
            }
            self.validate_mailbox_target_ready(target).await?;
            return Ok(vec![target]);
        }

        if to == "parent" {
            let parent = match thread_pool.parent_thread_id(&thread_id) {
                Some(parent) => Some(parent),
                None => thread_pool
                    .recover_parent_thread_id(&thread_id)
                    .await
                    .map_err(|error| Self::scheduler_error(error.to_string()))?,
            }
            .ok_or_else(|| Self::scheduler_error("current thread does not have a direct parent"))?;
            self.validate_mailbox_target_ready(parent).await?;
            return Ok(vec![parent]);
        }

        if to == "*" {
            let children = self.mailbox_ready_child_thread_ids(thread_id).await?;
            if children.is_empty() {
                return Err(Self::scheduler_error(
                    "current thread does not have any mailbox-ready direct child jobs",
                ));
            }
            return Ok(children);
        }

        Ok(vec![self.resolve_agent_name_target(thread_id, to).await?])
    }
}

#[async_trait]
impl SchedulerBackend for SessionSchedulerBackend {
    async fn dispatch_job(
        &self,
        request: SchedulerDispatchRequest,
    ) -> std::result::Result<String, ToolError> {
        let agent = self.current_scheduler_agent().await?;
        if agent.subagent_names.is_empty() {
            return Err(Self::scheduler_error(
                "this agent has no subagents configured",
            ));
        }

        let dispatch_depth = self.dispatch_depth_for_thread(request.thread_id).await?;
        if dispatch_depth >= MAX_DISPATCH_DEPTH {
            return Err(Self::scheduler_error(format!(
                "maximum dispatch depth ({}) exceeded",
                MAX_DISPATCH_DEPTH
            )));
        }

        let job_id = Uuid::new_v4().to_string();

        let dispatch_event = ThreadEvent::JobDispatched {
            thread_id: request.thread_id,
            job_id: job_id.clone(),
            agent_id: request.agent_id,
            prompt: request.prompt.clone(),
            context: request.context.clone(),
        };
        if let Err(error) = request.pipe_tx.send(dispatch_event) {
            tracing::warn!("failed to send JobDispatched event: {error}");
        }

        self.job_manager
            .dispatch_job(
                request.thread_id,
                job_id.clone(),
                request.agent_id,
                request.prompt,
                request.context,
                request.pipe_tx,
            )
            .await
            .map_err(|error| ToolError::ExecutionFailed {
                tool_name: "scheduler".to_string(),
                reason: error.to_string(),
            })?;

        Ok(job_id)
    }

    async fn list_subagents(&self) -> std::result::Result<Vec<SchedulerSubagent>, ToolError> {
        let agent = self.current_scheduler_agent().await?;
        let records = self
            .template_manager
            .list_subagents_by_names(&agent.subagent_names)
            .await
            .map_err(|error| Self::scheduler_error(error.to_string()))?;

        Ok(records
            .into_iter()
            .map(|record| SchedulerSubagent {
                agent_id: record.id,
                display_name: record.display_name,
                description: record.description,
            })
            .collect())
    }

    async fn get_job_result(
        &self,
        request: SchedulerLookupRequest,
    ) -> std::result::Result<SchedulerJobLookup, ToolError> {
        let lookup = self
            .job_manager
            .get_job_result_status_persisted(request.thread_id, &request.job_id, false)
            .await
            .map_err(|error| Self::scheduler_error(error.to_string()))?;

        if request.consume && matches!(lookup, JobLookup::Completed(_)) {
            self.claim_queued_runtime_result(request.thread_id, &request.job_id)
                .await?;
            let consumed_lookup = self
                .job_manager
                .get_job_result_status_persisted(request.thread_id, &request.job_id, true)
                .await
                .map_err(|error| Self::scheduler_error(error.to_string()))?;
            return Ok(Self::map_job_lookup(consumed_lookup));
        }

        Ok(Self::map_job_lookup(lookup))
    }

    async fn send_message(
        &self,
        request: SendMessageRequest,
    ) -> std::result::Result<SendMessageResponse, ToolError> {
        let targets = self
            .resolve_message_targets(request.thread_id, &request.to)
            .await?;
        let source_label = self.source_label(request.thread_id).await;
        let thread_pool = self.thread_pool();

        for target in &targets {
            let message = MailboxMessage {
                id: Uuid::new_v4().to_string(),
                from_thread_id: request.thread_id,
                to_thread_id: *target,
                from_label: source_label.clone(),
                message_type: MailboxMessageType::Plain,
                text: request.message.clone(),
                timestamp: Utc::now().to_rfc3339(),
                read: false,
                summary: request.summary.clone(),
            };

            match thread_pool.runtime_summary(target) {
                Some(summary) if summary.kind == ThreadPoolRuntimeKind::Chat => {
                    let session_id = summary.session_id.ok_or_else(|| {
                        Self::scheduler_error(format!(
                            "chat thread {} is missing a session binding",
                            target
                        ))
                    })?;
                    let session = self
                        .sessions
                        .get(&session_id)
                        .map(|entry| entry.value().clone())
                        .ok_or_else(|| {
                            Self::scheduler_error(format!(
                                "session {} is not loaded for thread {}",
                                session_id, target
                            ))
                        })?;
                    thread_pool.register_chat_thread(session_id, *target);
                    let thread = thread_pool
                        .ensure_chat_runtime(session_id, *target)
                        .await
                        .map_err(|error| Self::scheduler_error(error.to_string()))?;
                    session.add_thread(thread);
                    thread_pool
                        .deliver_mailbox_message(*target, message)
                        .await
                        .map_err(|error| Self::scheduler_error(error.to_string()))?;
                }
                _ => {
                    thread_pool
                        .deliver_mailbox_message(*target, message)
                        .await
                        .map_err(|error| Self::scheduler_error(error.to_string()))?;
                }
            }
        }

        Ok(SendMessageResponse {
            delivered: targets.len(),
            thread_ids: targets,
        })
    }
}

/// Manages sessions and their threads.
#[derive(Clone)]
pub struct SessionManager {
    session_repo: Arc<dyn SessionRepository>,
    thread_repo: Arc<dyn ThreadRepository>,
    llm_provider_repo: Arc<dyn LlmProviderRepository>,
    sessions: Arc<DashMap<SessionId, Arc<Session>>>,
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    mcp_tool_resolver: Arc<dyn McpToolResolver>,
    trace_dir: PathBuf,
    thread_pool: Arc<ThreadPool>,
}

impl SessionManager {
    /// Create a new SessionManager.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_repo: Arc<dyn SessionRepository>,
        thread_repo: Arc<dyn ThreadRepository>,
        llm_provider_repo: Arc<dyn LlmProviderRepository>,
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        mcp_tool_resolver: Arc<dyn McpToolResolver>,
        tool_manager: Arc<ToolManager>,
        trace_dir: PathBuf,
        thread_pool: Arc<ThreadPool>,
        job_manager: Arc<JobManager>,
    ) -> Self {
        let sessions = Arc::new(DashMap::new());
        let scheduler_backend = Arc::new(SessionSchedulerBackend::new(
            template_manager.clone(),
            job_manager.clone(),
            Arc::clone(&sessions),
        ));
        tool_manager.register(Arc::new(SchedulerTool::new(scheduler_backend.clone())));
        {
            let sessions = Arc::clone(&sessions);
            let thread_pool = Arc::clone(&thread_pool);
            job_manager.set_chat_mailbox_forwarder(move |thread_id, message| {
                let sessions = Arc::clone(&sessions);
                let thread_pool = Arc::clone(&thread_pool);
                async move {
                    let Some(summary) = thread_pool.runtime_summary(&thread_id) else {
                        return false;
                    };
                    if summary.kind != ThreadPoolRuntimeKind::Chat {
                        return false;
                    }
                    let Some(session_id) = summary.session_id else {
                        return false;
                    };
                    let Some(session) =
                        sessions.get(&session_id).map(|entry| entry.value().clone())
                    else {
                        return false;
                    };
                    thread_pool.register_chat_thread(session_id, thread_id);
                    let Ok(thread) = thread_pool.ensure_chat_runtime(session_id, thread_id).await
                    else {
                        return false;
                    };
                    session.add_thread(thread);
                    thread_pool
                        .deliver_mailbox_message(thread_id, message)
                        .await
                        .is_ok()
                }
            });
        }

        Self {
            session_repo,
            thread_repo,
            llm_provider_repo,
            sessions,
            template_manager,
            provider_resolver,
            mcp_tool_resolver,
            trace_dir,
            thread_pool,
        }
    }

    async fn ensure_thread_runtime_with_mcp(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<Arc<tokio::sync::RwLock<argus_agent::Thread>>> {
        let thread = self
            .thread_pool
            .ensure_chat_runtime(session_id, thread_id)
            .await
            .map_err(|error| ArgusError::LlmError {
                reason: error.to_string(),
            })?;
        thread
            .write()
            .await
            .set_mcp_tool_resolver(Some(Arc::clone(&self.mcp_tool_resolver)));
        Ok(thread)
    }

    /// List all sessions (from DB).
    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>> {
        let sessions =
            self.session_repo
                .list_with_counts()
                .await
                .map_err(|e| ArgusError::DatabaseError {
                    reason: e.to_string(),
                })?;

        let sessions = sessions
            .into_iter()
            .map(|swc| {
                let updated_at = chrono::DateTime::parse_from_rfc3339(&swc.session.updated_at)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now());

                SessionSummary {
                    id: swc.session.id,
                    name: swc.session.name,
                    thread_count: swc.thread_count,
                    updated_at,
                }
            })
            .collect();

        Ok(sessions)
    }

    /// Load a session into memory.
    pub async fn load(&self, session_id: SessionId) -> Result<Arc<Session>> {
        if let Some(existing) = self.sessions.get(&session_id) {
            return Ok(existing.clone());
        }

        let session_record =
            self.session_repo
                .get(&session_id)
                .await
                .map_err(|e| ArgusError::DatabaseError {
                    reason: e.to_string(),
                })?;

        let session = match session_record {
            Some(record) => Arc::new(Session::new(session_id, record.name)),
            None => return Err(ArgusError::SessionNotFound(session_id)),
        };
        let thread_records = self
            .thread_repo
            .list_threads_in_session(&session_id)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        self.sessions.insert(session_id, session.clone());
        for thread_record in thread_records {
            self.thread_pool
                .register_chat_thread(session_id, thread_record.id);
            match self
                .ensure_thread_runtime_with_mcp(session_id, thread_record.id)
                .await
            {
                Ok(thread) => session.add_thread(thread),
                Err(error) => {
                    tracing::warn!(
                        session_id = %session_id,
                        thread_id = %thread_record.id,
                        error = %error,
                        "Failed to load thread runtime into session"
                    );
                }
            }
        }

        Ok(session)
    }

    /// Unload a session from memory.
    pub async fn unload(&self, session_id: SessionId) -> Result<()> {
        if let Some((_, session)) = self.sessions.remove(&session_id) {
            for thread_id in session.thread_ids() {
                self.thread_pool.remove_runtime(&thread_id);
            }
        }
        Ok(())
    }

    /// Create a new session.
    pub async fn create(&self, name: String) -> Result<SessionId> {
        let session_id = SessionId::new();
        self.session_repo
            .create(&session_id, &name)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        Ok(session_id)
    }

    /// Delete a session and all its threads.
    pub async fn delete(&self, session_id: SessionId) -> Result<()> {
        let thread_ids = self
            .thread_repo
            .list_threads_in_session(&session_id)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?
            .into_iter()
            .map(|thread| thread.id)
            .collect::<Vec<_>>();

        // Delete threads belonging to this session (no CASCADE on session_id FK)
        self.thread_repo
            .delete_threads_in_session(&session_id)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        // Delete the session row
        self.session_repo
            .delete(&session_id)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        // Remove from memory if loaded
        self.sessions.remove(&session_id);
        for thread_id in thread_ids {
            self.thread_pool.remove_runtime(&thread_id);
        }

        // Clean up session trace directory
        let session_dir = self.trace_dir.join(session_id.to_string());
        if session_dir.exists() {
            if let Err(e) = tokio::fs::remove_dir_all(&session_dir).await {
                tracing::warn!(session_id = %session_id, error = %e, "Failed to remove session trace directory");
            }
        }

        Ok(())
    }

    /// Create a new thread in a session.
    ///
    /// Provider selection logic:
    /// 1. Use `provider_id` if specified
    /// 2. Use `agent_record.provider_id` if set
    /// 3. Use default provider
    pub async fn create_thread(
        &self,
        session_id: SessionId,
        template_id: AgentId,
        explicit_provider_id: Option<ProviderId>,
        model_override: Option<&str>,
    ) -> Result<ThreadId> {
        let session = self.load(session_id).await?;

        // Get agent record (template)
        let agent_record = self
            .template_manager
            .get(template_id)
            .await?
            .ok_or(ArgusError::TemplateNotFound(template_id.inner()))?;

        // Model resolution: explicit override > agent default model > provider default
        let requested_model = model_override.or(agent_record.model_id.as_deref());

        // Resolve provider using priority: explicit > agent_record > default
        let (provider_id, provider) = match explicit_provider_id.or(agent_record.provider_id) {
            Some(provider_id) => {
                let provider = match requested_model {
                    Some(model) => {
                        self.provider_resolver
                            .resolve_with_model(provider_id, model)
                            .await?
                    }
                    None => self.provider_resolver.resolve(provider_id).await?,
                };
                (provider_id, provider)
            }
            None => {
                let default_llm_provider_id = self
                    .llm_provider_repo
                    .get_default_provider_id()
                    .await
                    .map_err(|e| ArgusError::DatabaseError {
                        reason: e.to_string(),
                    })?
                    .ok_or(ArgusError::DefaultProviderNotConfigured)?;
                let default_provider_id = ProviderId::new(default_llm_provider_id.into_inner());

                let provider = match requested_model {
                    Some(model) => match self
                        .provider_resolver
                        .resolve_with_model(default_provider_id, model)
                        .await
                    {
                        Ok(provider) => provider,
                        Err(model_error) => {
                            tracing::warn!(
                                session_id = %session_id,
                                template_id = %template_id,
                                provider_id = %default_provider_id,
                                model_override = %model,
                                error = %model_error,
                                "Failed to resolve default provider with model override, falling back to default model"
                            );
                            match self.provider_resolver.resolve(default_provider_id).await {
                                Ok(provider) => provider,
                                Err(error) => {
                                    Arc::new(UnconfiguredProvider::new(error.to_string()))
                                }
                            }
                        }
                    },
                    None => match self.provider_resolver.resolve(default_provider_id).await {
                        Ok(provider) => provider,
                        Err(error) => Arc::new(UnconfiguredProvider::new(error.to_string())),
                    },
                };

                (default_provider_id, provider)
            }
        };

        let effective_model = provider.model_name().to_string();

        // Generate thread ID (UUID)
        let thread_id = ThreadId::new();
        let thread_trace_dir = chat_thread_base_dir(&self.trace_dir, session_id, thread_id);
        persist_thread_metadata(
            &thread_trace_dir,
            &ThreadTraceMetadata {
                thread_id,
                kind: ThreadTraceKind::ChatRoot,
                root_session_id: Some(session_id),
                parent_thread_id: None,
                job_id: None,
                agent_snapshot: agent_record.clone(),
            },
        )
        .await
        .map_err(|error| ArgusError::DatabaseError {
            reason: error.to_string(),
        })?;

        // Insert into DB
        use argus_repository::types::ThreadRecord;
        let thread_record = ThreadRecord {
            id: thread_id,
            provider_id: LlmProviderId::new(provider_id.inner()),
            title: None,
            token_count: 0,
            turn_count: 0,
            session_id: Some(session_id),
            template_id: Some(template_id),
            model_override: Some(effective_model.clone()),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        self.thread_repo
            .upsert_thread(&thread_record)
            .await
            .map_err(|e| {
                let thread_trace_dir = thread_trace_dir.clone();
                tokio::spawn(async move {
                    let _ = tokio::fs::remove_dir_all(thread_trace_dir).await;
                });
                ArgusError::DatabaseError {
                    reason: e.to_string(),
                }
            })?;
        self.thread_pool.register_chat_thread(session_id, thread_id);
        let thread = match self
            .ensure_thread_runtime_with_mcp(session_id, thread_id)
            .await
        {
            Ok(thread) => thread,
            Err(error) => {
                self.thread_pool.remove_runtime(&thread_id);
                let _ = self.thread_repo.delete_thread(&thread_id).await;
                let _ = tokio::fs::remove_dir_all(&thread_trace_dir).await;
                return Err(ArgusError::LlmError {
                    reason: error.to_string(),
                });
            }
        };
        session.add_thread(thread);

        Ok(thread_id)
    }

    /// Delete a thread from a session.
    pub async fn delete_thread(&self, session_id: SessionId, thread_id: &ThreadId) -> Result<()> {
        // Delete from DB
        self.thread_repo
            .delete_thread(thread_id)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;
        self.thread_pool.remove_runtime(thread_id);

        // Remove from in-memory session if loaded
        if let Some(session) = self.sessions.get(&session_id) {
            session.remove_thread(thread_id);
        }

        Ok(())
    }

    /// Rename a persisted session.
    pub async fn rename_session(&self, session_id: SessionId, name: String) -> Result<()> {
        let found = self
            .session_repo
            .rename(&session_id, name.trim())
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        if !found {
            return Err(ArgusError::SessionNotFound(session_id));
        }

        Ok(())
    }

    /// Rename a thread title and keep loaded runtime state in sync.
    pub async fn rename_thread(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
        title: String,
    ) -> Result<()> {
        let normalized = title.trim().to_string();
        let persisted_title: Option<&str> = if normalized.is_empty() {
            None
        } else {
            Some(&normalized)
        };
        let in_memory_title: Option<String> = if normalized.is_empty() {
            None
        } else {
            Some(normalized.clone())
        };
        let found = self
            .thread_repo
            .rename_thread(thread_id, &session_id, persisted_title)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        if !found {
            return Err(ArgusError::ThreadNotFound(thread_id.inner().to_string()));
        }

        if let Some(thread) = self.thread_pool.loaded_chat_thread(thread_id) {
            let mut thread = thread.write().await;
            thread.set_title(in_memory_title);
        }

        Ok(())
    }

    /// Update the bound provider/model for an existing thread.
    pub async fn update_thread_model(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
        provider_id: ProviderId,
        model: &str,
    ) -> Result<(ProviderId, String)> {
        let provider = self
            .provider_resolver
            .resolve_with_model(provider_id, model)
            .await?;
        let effective_model = provider.model_name().to_string();

        let found = self
            .thread_repo
            .update_thread_model(
                thread_id,
                &session_id,
                LlmProviderId::new(provider_id.inner()),
                Some(&effective_model),
            )
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        if !found {
            return Err(ArgusError::ThreadNotFound(thread_id.inner().to_string()));
        }

        if let Some(thread) = self.thread_pool.loaded_chat_thread(thread_id) {
            let mut thread = thread.write().await;
            thread.set_provider(provider);
        }

        Ok((provider_id, effective_model))
    }

    /// Get threads for a session (metadata only, from DB).
    pub async fn list_threads(&self, session_id: SessionId) -> Result<Vec<ThreadSummary>> {
        let thread_records = self
            .thread_repo
            .list_threads_in_session(&session_id)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        let threads = thread_records
            .into_iter()
            .map(|record| {
                let updated_at = chrono::DateTime::parse_from_rfc3339(&record.updated_at)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now());

                ThreadSummary {
                    id: record.id,
                    title: record.title,
                    token_count: record.token_count as i64,
                    turn_count: record.turn_count as i64,
                    updated_at,
                }
            })
            .collect();

        Ok(threads)
    }

    /// Send a message to a thread via the unified pipe.
    pub async fn send_message(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
        message: String,
    ) -> Result<()> {
        let session = self.load(session_id).await?;
        self.ensure_thread_in_session(session_id, thread_id).await?;
        self.ensure_thread_runtime_with_mcp(session_id, *thread_id)
            .await?;
        let thread = self
            .thread_pool
            .ensure_chat_runtime(session_id, *thread_id)
            .await
            .map_err(|error| ArgusError::LlmError {
                reason: error.to_string(),
            })?;
        session.add_thread(thread);
        self.thread_pool
            .send_chat_message(session_id, *thread_id, message)
            .await
            .map_err(|error| ArgusError::LlmError {
                reason: error.to_string(),
            })
    }

    /// Send a cancel/interrupt signal to a specific thread's active turn.
    pub async fn cancel_thread(&self, session_id: SessionId, thread_id: &ThreadId) -> Result<()> {
        let session = self.load(session_id).await?;
        self.ensure_thread_in_session(session_id, thread_id).await?;

        if session.interrupt_thread(thread_id).await {
            Ok(())
        } else if let Some(thread) = self.thread_pool.loaded_chat_thread(thread_id) {
            thread
                .read()
                .await
                .send_message(ThreadMessage::Interrupt)
                .map_err(|error| ArgusError::LlmError {
                    reason: error.to_string(),
                })
        } else {
            Ok(())
        }
    }

    /// Get the thread message history, falling back to turn trace recovery when
    /// the in-memory history is empty after reloading a persisted session.
    pub async fn get_thread_messages(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
    ) -> Result<Vec<ChatMessage>> {
        self.load(session_id).await?;
        if let Some(thread) = self.thread_pool.loaded_chat_thread(thread_id) {
            let thread = thread.read().await;
            if thread.has_non_system_history() || thread.turn_count() > 0 {
                return Ok(thread.history_iter().cloned().collect());
            }
            let recovered =
                recover_messages_from_trace(&self.trace_dir, &session_id, thread_id).await?;
            if !recovered.is_empty() {
                return Ok(recovered);
            }
            return Ok(thread.history_iter().cloned().collect());
        }
        let _thread_record = self
            .thread_repo
            .get_thread_in_session(thread_id, &session_id)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?
            .ok_or_else(|| ArgusError::ThreadNotFound(thread_id.inner().to_string()))?;
        recover_messages_from_trace(&self.trace_dir, &session_id, thread_id).await
    }

    /// Get a thread snapshot without forcing the runtime to become resident.
    pub async fn get_thread_snapshot(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
    ) -> Result<(Vec<ChatMessage>, u32, u32, u32)> {
        self.load(session_id).await?;
        if let Some(thread) = self.thread_pool.loaded_chat_thread(thread_id) {
            let thread = thread.read().await;
            let (messages, turn_count, token_count) =
                if thread.has_non_system_history() || thread.turn_count() > 0 {
                    (
                        thread.history_iter().cloned().collect(),
                        thread.turn_count(),
                        thread.token_count(),
                    )
                } else {
                    let recovered =
                        recover_thread_state_from_trace(&self.trace_dir, &session_id, thread_id)
                            .await?;
                    if recovered.turn_count > 0 {
                        (
                            recovered.messages,
                            recovered.turn_count,
                            recovered.token_count,
                        )
                    } else {
                        (
                            thread.history_iter().cloned().collect(),
                            thread.turn_count(),
                            thread.token_count(),
                        )
                    }
                };
            return Ok((
                messages,
                turn_count,
                token_count,
                thread.plan().len() as u32,
            ));
        }

        let thread_record = self
            .thread_repo
            .get_thread_in_session(thread_id, &session_id)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?
            .ok_or_else(|| ArgusError::ThreadNotFound(thread_id.inner().to_string()))?;
        let recovered =
            recover_thread_state_from_trace(&self.trace_dir, &session_id, thread_id).await?;

        Ok(if recovered.turn_count > 0 {
            (
                recovered.messages,
                recovered.turn_count,
                recovered.token_count,
                0,
            )
        } else {
            (
                recovered.messages,
                thread_record.turn_count,
                thread_record.token_count,
                0,
            )
        })
    }

    /// Activate a historical thread so it can continue as a live in-memory thread.
    pub async fn activate_thread(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
    ) -> Result<(AgentId, Option<ProviderId>, Option<String>)> {
        let thread_record = self
            .thread_repo
            .get_thread_in_session(thread_id, &session_id)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?
            .ok_or_else(|| ArgusError::ThreadNotFound(thread_id.inner().to_string()))?;

        let template_id = thread_record.template_id.unwrap_or_else(|| {
            tracing::warn!(thread_id = %thread_id, "No template_id for thread, using AgentId(0)");
            AgentId::new(0)
        });
        let provider_id = Some(ProviderId::new(thread_record.provider_id.into_inner()));
        self.load(session_id).await?;
        self.thread_pool
            .register_chat_thread(session_id, *thread_id);
        let effective_model = if let Some(thread) = self.thread_pool.loaded_chat_thread(thread_id) {
            Some(thread.read().await.provider().model_name().to_string())
        } else {
            thread_record.model_override.clone()
        };
        Ok((template_id, provider_id, effective_model))
    }

    /// Subscribe to thread events.
    pub async fn subscribe(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
    ) -> Option<broadcast::Receiver<ThreadEvent>> {
        let _ = self.load(session_id).await.ok()?;
        self.thread_repo
            .get_thread_in_session(thread_id, &session_id)
            .await
            .ok()
            .flatten()?;
        self.thread_pool.subscribe(thread_id).or_else(|| {
            Some(
                self.thread_pool
                    .register_chat_thread(session_id, *thread_id),
            )
        })
    }

    async fn ensure_thread_in_session(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
    ) -> Result<()> {
        let thread_record = self
            .thread_repo
            .get_thread_in_session(thread_id, &session_id)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;
        if thread_record.is_none() {
            return Err(ArgusError::ThreadNotFound(thread_id.inner().to_string()));
        }
        Ok(())
    }
}

async fn recover_messages_from_trace(
    trace_dir: &std::path::Path,
    session_id: &SessionId,
    thread_id: &ThreadId,
) -> Result<Vec<ChatMessage>> {
    Ok(
        recover_thread_state_from_trace(trace_dir, session_id, thread_id)
            .await?
            .messages,
    )
}

async fn recover_thread_state_from_trace(
    trace_dir: &std::path::Path,
    session_id: &SessionId,
    thread_id: &ThreadId,
) -> Result<RecoveredThreadState> {
    let base_dir = chat_thread_base_dir(trace_dir, *session_id, *thread_id);
    let recovered =
        recover_thread_log_state(&base_dir)
            .await
            .map_err(|error| ArgusError::DatabaseError {
                reason: format_turn_log_recovery_error(thread_id, &error),
            })?;

    Ok(flatten_recovered_thread_state(recovered))
}

fn flatten_recovered_thread_state(recovered: RecoveredThreadLogState) -> RecoveredThreadState {
    RecoveredThreadState {
        messages: recovered.committed_messages(),
        turn_count: recovered.turn_count(),
        token_count: recovered.token_count(),
    }
}

fn format_turn_log_recovery_error(
    thread_id: &ThreadId,
    error: &argus_agent::TurnLogError,
) -> String {
    match error {
        argus_agent::TurnLogError::TurnNotFound(path) => {
            let turn_number = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.split('.').next())
                .and_then(|stem| stem.parse::<u32>().ok());
            match turn_number {
                Some(turn_number) => format!("missing turn trace file {turn_number}"),
                None => {
                    format!("failed to recover committed turn log for thread {thread_id}: {error}")
                }
            }
        }
        _ => format!("failed to recover committed turn log for thread {thread_id}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    struct NoopMcpResolver;

    #[async_trait]
    impl McpToolResolver for NoopMcpResolver {
        async fn resolve_for_agent(
            &self,
            _agent_id: AgentId,
        ) -> argus_protocol::Result<ResolvedMcpTools> {
            Ok(ResolvedMcpTools::default())
        }
    }

    use super::{
        recover_messages_from_trace, recover_thread_state_from_trace, SessionManager,
        SessionSchedulerBackend,
    };
    use argus_agent::history::{TurnRecord, TurnRecordKind};
    use argus_agent::thread_trace_store::{
        chat_thread_base_dir, persist_thread_metadata, ThreadTraceKind, ThreadTraceMetadata,
    };
    use argus_agent::tool_context::{clear_current_agent_id, set_current_agent_id};
    use argus_agent::turn_log_store::append_turn_record;
    use argus_agent::turn_log_store::RecoveredThreadLogState;
    use argus_protocol::llm::ChatMessage;
    use argus_protocol::llm::{
        CompletionRequest, CompletionResponse, LlmError, LlmProvider, LlmProviderRepository,
    };
    use argus_protocol::{
        AgentId, AgentRecord, MailboxMessage, MailboxMessageType, McpToolResolver, ProviderId,
        ResolvedMcpTools, SessionId, ThinkingConfig, ThreadId, TokenUsage,
    };
    use argus_repository::migrate;
    use argus_repository::traits::JobRepository;
    use argus_repository::traits::{AgentRepository, SessionRepository, ThreadRepository};
    use argus_repository::types::{
        AgentId as RepoAgentId, JobRecord, JobResult, JobStatus, JobType,
    };
    use argus_repository::ArgusSqlite;
    use argus_template::TemplateManager;
    use argus_tool::{SchedulerBackend, SchedulerDispatchRequest, SchedulerLookupRequest};
    use async_trait::async_trait;
    use chrono::Utc;
    use dashmap::DashMap;
    use futures_util::stream;
    use sqlx::SqlitePool;
    use std::path::PathBuf;
    use tempfile::TempDir;
    use tokio::sync::broadcast;
    use tokio::time::{sleep, timeout, Duration};
    use uuid::Uuid;

    #[derive(Debug)]
    struct FixedProvider {
        model_name: String,
    }

    #[async_trait]
    impl LlmProvider for FixedProvider {
        fn model_name(&self) -> &str {
            &self.model_name
        }

        fn cost_per_token(&self) -> (rust_decimal::Decimal, rust_decimal::Decimal) {
            (rust_decimal::Decimal::ZERO, rust_decimal::Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<CompletionResponse, LlmError> {
            Err(LlmError::RequestFailed {
                provider: self.model_name.clone(),
                reason: "not used in routing tests".to_string(),
            })
        }

        async fn stream_complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<argus_protocol::llm::LlmEventStream, LlmError> {
            Err(LlmError::UnsupportedCapability {
                provider: self.model_name.clone(),
                capability: "stream_complete".to_string(),
            })
        }
    }

    #[derive(Debug)]
    struct HangingStreamingProvider {
        model_name: String,
    }

    #[async_trait]
    impl LlmProvider for HangingStreamingProvider {
        fn model_name(&self) -> &str {
            &self.model_name
        }

        fn cost_per_token(&self) -> (rust_decimal::Decimal, rust_decimal::Decimal) {
            (rust_decimal::Decimal::ZERO, rust_decimal::Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<CompletionResponse, LlmError> {
            Err(LlmError::UnsupportedCapability {
                provider: self.model_name.clone(),
                capability: "complete".to_string(),
            })
        }

        async fn stream_complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<argus_protocol::llm::LlmEventStream, LlmError> {
            Ok(Box::pin(stream::pending()))
        }
    }

    struct FixedProviderResolver {
        provider: Arc<dyn LlmProvider>,
    }

    impl std::fmt::Debug for FixedProviderResolver {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("FixedProviderResolver").finish()
        }
    }

    impl FixedProviderResolver {
        fn new(provider: Arc<dyn LlmProvider>) -> Self {
            Self { provider }
        }
    }

    struct SessionManagerHarness {
        manager: SessionManager,
        job_manager: Arc<argus_job::JobManager>,
        temp_dir: TempDir,
        sqlite: Arc<ArgusSqlite>,
        template_manager: Arc<TemplateManager>,
        session_id: SessionId,
        thread_id: ThreadId,
        agent_id: AgentId,
    }

    #[async_trait]
    impl argus_protocol::ProviderResolver for FixedProviderResolver {
        async fn resolve(&self, _id: ProviderId) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            Ok(Arc::clone(&self.provider))
        }

        async fn default_provider(&self) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            Ok(Arc::clone(&self.provider))
        }

        async fn resolve_with_model(
            &self,
            _id: ProviderId,
            _model: &str,
        ) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            Ok(Arc::clone(&self.provider))
        }
    }

    async fn test_session_manager_harness_with_provider(
        provider: Arc<dyn LlmProvider>,
    ) -> SessionManagerHarness {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite memory pool should connect");
        migrate(&pool).await.expect("migration should succeed");
        let sqlite = Arc::new(ArgusSqlite::new(pool));

        let template_manager = Arc::new(TemplateManager::new(
            sqlite.clone() as Arc<dyn AgentRepository>,
            sqlite.clone(),
        ));
        let agent_id = AgentId::new(11);
        template_manager
            .upsert(AgentRecord {
                id: agent_id,
                display_name: "Routing Test Agent".to_string(),
                description: "Used to verify session mailbox routing".to_string(),
                version: "1.0.0".to_string(),
                provider_id: Some(ProviderId::new(1)),
                model_id: Some("routing-test".to_string()),
                system_prompt: "You are a routing test agent.".to_string(),
                tool_names: vec![],
                subagent_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::disabled()),
            })
            .await
            .expect("agent upsert should succeed");

        let provider_resolver = Arc::new(FixedProviderResolver::new(provider));
        let tool_manager = Arc::new(argus_tool::ToolManager::new());
        let job_manager = Arc::new(argus_job::JobManager::new_with_repositories(
            Arc::clone(&template_manager),
            provider_resolver.clone(),
            Arc::clone(&tool_manager),
            temp_dir.path().join("trace"),
            sqlite.clone() as Arc<dyn argus_repository::traits::JobRepository>,
            sqlite.clone() as Arc<dyn ThreadRepository>,
            sqlite.clone() as Arc<dyn LlmProviderRepository>,
        ));
        let session_manager = SessionManager::new(
            sqlite.clone() as Arc<dyn SessionRepository>,
            sqlite.clone() as Arc<dyn ThreadRepository>,
            sqlite.clone() as Arc<dyn LlmProviderRepository>,
            Arc::clone(&template_manager),
            provider_resolver,
            Arc::new(NoopMcpResolver),
            tool_manager,
            temp_dir.path().join("trace"),
            job_manager.thread_pool(),
            Arc::clone(&job_manager),
        );

        let session_id: SessionId = session_manager
            .create("routing session".to_string())
            .await
            .expect("session should create");
        let thread_id: ThreadId = session_manager
            .create_thread(session_id, agent_id, Some(ProviderId::new(1)), None)
            .await
            .expect("thread should create");

        SessionManagerHarness {
            manager: session_manager,
            job_manager,
            temp_dir,
            sqlite,
            template_manager,
            session_id,
            thread_id,
            agent_id,
        }
    }

    async fn test_session_manager_with_provider(
        provider: Arc<dyn LlmProvider>,
    ) -> (SessionManager, TempDir, SessionId, ThreadId) {
        let harness = test_session_manager_harness_with_provider(provider).await;
        (
            harness.manager,
            harness.temp_dir,
            harness.session_id,
            harness.thread_id,
        )
    }

    async fn test_session_manager() -> (SessionManager, TempDir, SessionId, ThreadId) {
        test_session_manager_with_provider(Arc::new(FixedProvider {
            model_name: "routing-test".to_string(),
        }))
        .await
    }

    async fn wait_until_thread_running(thread: &Arc<tokio::sync::RwLock<argus_agent::Thread>>) {
        timeout(Duration::from_secs(5), async {
            loop {
                if thread.read().await.is_turn_running() {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runtime should start the queued turn");
    }

    async fn wait_until_runtime_status(
        manager: &SessionManager,
        thread_id: ThreadId,
        expected_status: argus_protocol::ThreadRuntimeStatus,
    ) {
        timeout(Duration::from_secs(5), async {
            loop {
                let Some(summary) = manager.thread_pool.runtime_summary(&thread_id) else {
                    panic!(
                        "thread {thread_id} should stay registered while waiting for runtime status"
                    );
                };
                if summary.status == expected_status {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runtime should reach the expected thread-pool status");
    }

    fn usage(total_tokens: u32) -> TokenUsage {
        TokenUsage {
            input_tokens: total_tokens.saturating_sub(1),
            output_tokens: 1,
            total_tokens,
        }
    }

    fn restarted_scheduler_backend(
        sqlite: Arc<ArgusSqlite>,
        template_manager: Arc<TemplateManager>,
        trace_dir: PathBuf,
        provider: Arc<dyn LlmProvider>,
    ) -> SessionSchedulerBackend {
        let provider_resolver = Arc::new(FixedProviderResolver::new(provider));
        let job_manager = Arc::new(argus_job::JobManager::new_with_repositories(
            template_manager.clone(),
            provider_resolver,
            Arc::new(argus_tool::ToolManager::new()),
            trace_dir,
            sqlite.clone() as Arc<dyn JobRepository>,
            sqlite.clone() as Arc<dyn ThreadRepository>,
            sqlite as Arc<dyn LlmProviderRepository>,
        ));
        SessionSchedulerBackend::new(template_manager, job_manager, Arc::new(DashMap::new()))
    }

    #[tokio::test]
    async fn cancel_thread_when_idle_is_a_noop() {
        let (manager, _temp_dir, session_id, thread_id) = test_session_manager().await;

        manager
            .cancel_thread(session_id, &thread_id)
            .await
            .expect("cancel should succeed");

        let session = manager.load(session_id).await.expect("session should load");
        let thread = session
            .get_thread(&thread_id)
            .expect("session should keep the thread handle");
        let guard = thread.read().await;
        assert!(
            !guard.is_turn_running(),
            "cancel_thread should leave an idle thread idle"
        );
        assert_eq!(guard.turn_count(), 0);
    }

    #[tokio::test]
    async fn cancel_thread_when_chat_runtime_is_evicted_is_a_successful_noop() {
        let (manager, _temp_dir, session_id, thread_id) = test_session_manager().await;

        assert!(
            manager.thread_pool.remove_runtime(&thread_id),
            "chat runtime should be evictable for the no-op cancel regression"
        );

        manager
            .cancel_thread(session_id, &thread_id)
            .await
            .expect("cancel should stay a successful no-op after runtime eviction");

        assert!(
            manager.thread_pool.loaded_chat_thread(&thread_id).is_none(),
            "cancel should not eagerly reload an evicted idle runtime"
        );
    }

    #[tokio::test]
    async fn send_message_wakes_existing_runtime_loop() {
        let (manager, _temp_dir, session_id, thread_id) =
            test_session_manager_with_provider(Arc::new(HangingStreamingProvider {
                model_name: "routing-hanging".to_string(),
            }))
            .await;

        manager
            .send_message(session_id, &thread_id, "hello".to_string())
            .await
            .expect("send_message should succeed");

        let session = manager.load(session_id).await.expect("session should load");
        let thread = session
            .get_thread(&thread_id)
            .expect("session should keep the runtime thread handle");

        wait_until_thread_running(&thread).await;
    }

    #[tokio::test]
    async fn send_message_rehydrates_evicted_chat_runtime_before_enqueueing() {
        let (manager, _temp_dir, session_id, thread_id) =
            test_session_manager_with_provider(Arc::new(HangingStreamingProvider {
                model_name: "routing-hanging".to_string(),
            }))
            .await;

        let session = manager.load(session_id).await.expect("session should load");
        assert!(
            manager.thread_pool.remove_runtime(&thread_id),
            "chat runtime should be removable for rehydrate coverage"
        );

        manager
            .send_message(session_id, &thread_id, "hello".to_string())
            .await
            .expect("send_message should reload the evicted runtime");

        let thread = session
            .get_thread(&thread_id)
            .expect("session should refresh the thread handle after reload");
        wait_until_thread_running(&thread).await;

        assert!(
            session.get_thread(&thread_id).is_some(),
            "session should refresh its thread handle after runtime reload"
        );
    }

    #[tokio::test]
    async fn chat_thread_rehydrates_from_trace_snapshot_instead_of_latest_template() {
        let (manager, temp_dir, session_id, thread_id) = test_session_manager().await;
        let snapshot_path = temp_dir
            .path()
            .join("trace")
            .join(session_id.to_string())
            .join(thread_id.to_string())
            .join("thread.json");

        let snapshot_content =
            fs::read_to_string(&snapshot_path).expect("thread snapshot should be persisted");
        let snapshot: ThreadTraceMetadata =
            serde_json::from_str(&snapshot_content).expect("thread snapshot should deserialize");
        assert_eq!(snapshot.kind, ThreadTraceKind::ChatRoot);
        assert_eq!(snapshot.root_session_id, Some(session_id));
        assert_eq!(
            snapshot.agent_snapshot.system_prompt,
            "You are a routing test agent."
        );

        manager
            .template_manager
            .upsert(AgentRecord {
                id: AgentId::new(11),
                display_name: "Routing Test Agent Updated".to_string(),
                description: "Updated template".to_string(),
                version: "2.0.0".to_string(),
                provider_id: Some(ProviderId::new(1)),
                model_id: Some("routing-test-v2".to_string()),
                system_prompt: "You are a mutated routing test agent.".to_string(),
                tool_names: vec![],
                subagent_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::disabled()),
            })
            .await
            .expect("template upsert should succeed");

        assert!(
            manager.thread_pool.remove_runtime(&thread_id),
            "existing runtime should be evicted before rehydration"
        );

        let thread = manager
            .thread_pool
            .ensure_chat_runtime(session_id, thread_id)
            .await
            .expect("chat runtime should rebuild from trace snapshot");
        let guard = thread.read().await;
        assert_eq!(
            guard.agent_record().system_prompt,
            "You are a routing test agent."
        );
        assert_eq!(guard.agent_record().display_name, "Routing Test Agent");
    }

    #[tokio::test]
    async fn unload_evicts_loaded_chat_runtimes() {
        let (manager, _temp_dir, session_id, thread_id) = test_session_manager().await;

        assert!(
            manager.thread_pool.loaded_chat_thread(&thread_id).is_some(),
            "test setup should leave the chat runtime loaded"
        );

        manager
            .unload(session_id)
            .await
            .expect("session unload should succeed");

        assert!(
            manager.thread_pool.loaded_chat_thread(&thread_id).is_none(),
            "unload should evict loaded chat runtimes alongside the session"
        );
    }

    #[tokio::test]
    async fn cancel_thread_wakes_running_turn_and_reaches_idle() {
        let (manager, _temp_dir, session_id, thread_id) =
            test_session_manager_with_provider(Arc::new(HangingStreamingProvider {
                model_name: "routing-hanging".to_string(),
            }))
            .await;

        manager
            .send_message(session_id, &thread_id, "hello".to_string())
            .await
            .expect("send_message should succeed");

        let session = manager.load(session_id).await.expect("session should load");
        let thread = session
            .get_thread(&thread_id)
            .expect("thread should be present in session");

        timeout(Duration::from_secs(5), async {
            loop {
                if thread.read().await.is_turn_running() {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runtime should start a turn before cancellation");

        manager
            .cancel_thread(session_id, &thread_id)
            .await
            .expect("cancel should succeed");

        timeout(Duration::from_secs(5), async {
            loop {
                if !thread.read().await.is_turn_running() {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runtime should return to idle after cancellation");
    }

    #[tokio::test]
    async fn deliver_mailbox_message_keeps_thread_runtime_registered() {
        let (manager, _temp_dir, session_id, thread_id) =
            test_session_manager_with_provider(Arc::new(HangingStreamingProvider {
                model_name: "routing-hanging".to_string(),
            }))
            .await;
        let session = manager.load(session_id).await.expect("session should load");

        manager
            .send_message(session_id, &thread_id, "hello".to_string())
            .await
            .expect("send_message should succeed");

        let thread = session
            .get_thread(&thread_id)
            .expect("thread should be present in session");
        timeout(Duration::from_secs(5), async {
            loop {
                if thread.read().await.is_turn_running() {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runtime should start before mailbox delivery");

        let message = MailboxMessage {
            id: Uuid::new_v4().to_string(),
            from_thread_id: ThreadId::new(),
            to_thread_id: thread_id,
            from_label: "sender".to_string(),
            message_type: MailboxMessageType::Plain,
            text: "hello from the mailbox".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            read: false,
            summary: Some("routing test".to_string()),
        };

        manager
            .thread_pool
            .deliver_mailbox_message(thread_id, message.clone())
            .await
            .expect("thread pool should deliver mailbox messages through thread routing");

        assert_eq!(
            manager
                .thread_pool
                .runtime_summary(&thread_id)
                .expect("thread summary should exist")
                .runtime
                .thread_id,
            thread_id,
            "mailbox delivery should keep the thread runtime registered"
        );
    }

    #[tokio::test]
    async fn consume_job_result_clears_evicted_runtime_inbox_copy() {
        let harness = test_session_manager_harness_with_provider(Arc::new(FixedProvider {
            model_name: "routing-test".to_string(),
        }))
        .await;
        let _session = harness
            .manager
            .load(harness.session_id)
            .await
            .expect("session should load");
        let job_id = "job-evicted-claim".to_string();
        let message = MailboxMessage {
            id: Uuid::new_v4().to_string(),
            from_thread_id: ThreadId::new(),
            to_thread_id: harness.thread_id,
            from_label: "subagent".to_string(),
            message_type: MailboxMessageType::JobResult {
                job_id: job_id.clone(),
                success: true,
                token_usage: None,
                agent_id: harness.agent_id,
                agent_display_name: "Routing Test Agent".to_string(),
                agent_description: "Used to verify session mailbox routing".to_string(),
            },
            text: "finished work".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            read: false,
            summary: Some("job finished".to_string()),
        };

        harness
            .manager
            .thread_pool
            .deliver_mailbox_message(harness.thread_id, message)
            .await
            .expect("thread pool should enqueue the job-result mailbox message");

        wait_until_runtime_status(
            &harness.manager,
            harness.thread_id,
            argus_protocol::ThreadRuntimeStatus::Cooling,
        )
        .await;

        harness
            .manager
            .thread_pool
            .evict_chat_if_idle(&harness.thread_id)
            .expect("chat runtime should evict once cooling");
        assert!(
            harness
                .manager
                .thread_pool
                .loaded_thread(&harness.thread_id)
                .is_none(),
            "evicted runtime should no longer be loaded"
        );

        let backend = SessionSchedulerBackend::new(
            Arc::clone(&harness.template_manager),
            Arc::clone(&harness.job_manager),
            Arc::clone(&harness.manager.sessions),
        );
        backend
            .claim_queued_runtime_result(harness.thread_id, &job_id)
            .await
            .expect("consume path should clear queued job results even after eviction");

        assert!(
            harness
                .manager
                .thread_pool
                .claim_queued_job_result(harness.thread_id, &job_id)
                .is_none(),
            "job-result inbox copy should already be claimed from the evicted runtime entry"
        );
    }

    #[tokio::test]
    async fn recover_messages_from_trace_flattens_typed_turn_records() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let base_dir = temp_dir
            .path()
            .join(session_id.to_string())
            .join(thread_id.to_string());
        fs::create_dir_all(base_dir.join("turns")).expect("turns dir should exist");

        append_turn_record(
            &base_dir,
            &TurnRecord::user_turn(
                1,
                vec![
                    ChatMessage::system("sys"),
                    ChatMessage::user("hi"),
                    ChatMessage::assistant("hello"),
                ],
                usage(3),
            ),
        )
        .await
        .expect("turn1 should append");
        append_turn_record(
            &base_dir,
            &TurnRecord::checkpoint(vec![ChatMessage::assistant("summary")], usage(7)),
        )
        .await
        .expect("checkpoint should append");
        append_turn_record(
            &base_dir,
            &TurnRecord::user_turn(
                2,
                vec![ChatMessage::user("next"), ChatMessage::assistant("reply")],
                usage(5),
            ),
        )
        .await
        .expect("turn2 should append");

        let messages = recover_messages_from_trace(temp_dir.path(), &session_id, &thread_id)
            .await
            .expect("messages should recover");
        let contents: Vec<_> = messages
            .into_iter()
            .map(|message| message.content)
            .collect();
        assert_eq!(contents, vec!["sys", "hi", "hello", "next", "reply"]);
    }

    #[tokio::test]
    async fn recover_thread_state_from_trace_uses_last_record_usage_and_turn_count() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let base_dir = temp_dir
            .path()
            .join(session_id.to_string())
            .join(thread_id.to_string());
        fs::create_dir_all(base_dir.join("turns")).expect("turns dir should exist");

        append_turn_record(
            &base_dir,
            &TurnRecord::user_turn(
                1,
                vec![
                    ChatMessage::system("sys"),
                    ChatMessage::user("turn1"),
                    ChatMessage::assistant("answer1"),
                ],
                usage(15),
            ),
        )
        .await
        .expect("turn1 should append");
        append_turn_record(
            &base_dir,
            &TurnRecord::checkpoint(vec![ChatMessage::assistant("summary")], usage(9)),
        )
        .await
        .expect("checkpoint should append");
        append_turn_record(
            &base_dir,
            &TurnRecord::user_turn(
                2,
                vec![
                    ChatMessage::user("turn2"),
                    ChatMessage::assistant("answer2"),
                ],
                usage(28),
            ),
        )
        .await
        .expect("turn2 should append");

        let recovered = recover_thread_state_from_trace(temp_dir.path(), &session_id, &thread_id)
            .await
            .expect("thread state should recover");

        assert_eq!(recovered.turn_count, 2);
        assert_eq!(recovered.token_count, 28);
        assert_eq!(recovered.messages.len(), 5);
    }

    #[tokio::test]
    async fn recover_thread_state_from_trace_reports_meta_jsonl_errors() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let base_dir = chat_thread_base_dir(temp_dir.path(), session_id, thread_id);
        fs::create_dir_all(&base_dir).expect("thread dir should exist");
        fs::write(
            base_dir.join("turns.jsonl"),
            "{\"kind\":\"Checkpoint\",\"turn_number\":0}\n",
        )
        .expect("invalid meta should write");

        let error = recover_thread_state_from_trace(temp_dir.path(), &session_id, &thread_id)
            .await
            .expect_err("invalid first checkpoint should fail");
        let message = error.to_string();
        assert!(message.contains("failed to recover committed turn log"));
    }

    #[test]
    fn flatten_recovered_thread_state_includes_turn_checkpoint_messages() {
        let recovered = RecoveredThreadLogState {
            turns: vec![TurnRecord::turn_checkpoint(
                1,
                vec![
                    ChatMessage::user("compressed user intent"),
                    ChatMessage::assistant("compressed assistant state"),
                ],
                TokenUsage {
                    input_tokens: 2,
                    output_tokens: 1,
                    total_tokens: 3,
                },
            )],
        };

        let flattened = super::flatten_recovered_thread_state(recovered);

        assert_eq!(flattened.turn_count, 1);
        assert_eq!(flattened.token_count, 3);
        assert_eq!(flattened.messages.len(), 2);
        assert_eq!(flattened.messages[0].content, "compressed user intent");
        assert_eq!(flattened.messages[1].content, "compressed assistant state");
        assert!(matches!(
            TurnRecordKind::TurnCheckpoint,
            TurnRecordKind::TurnCheckpoint
        ));
    }

    #[tokio::test]
    async fn scheduler_job_target_uses_persisted_binding_after_restart() {
        let harness = test_session_manager_harness_with_provider(Arc::new(FixedProvider {
            model_name: "routing-test".to_string(),
        }))
        .await;
        let child_thread_id = ThreadId::new();
        let job_id = "job-restart-routing".to_string();
        let child_base_dir = chat_thread_base_dir(
            harness.temp_dir.path().join("trace").as_path(),
            harness.session_id,
            harness.thread_id,
        )
        .join(child_thread_id.to_string());
        persist_thread_metadata(
            &child_base_dir,
            &ThreadTraceMetadata {
                thread_id: child_thread_id,
                kind: ThreadTraceKind::Job,
                root_session_id: Some(harness.session_id),
                parent_thread_id: Some(harness.thread_id),
                job_id: Some(job_id.clone()),
                agent_snapshot: harness
                    .template_manager
                    .get(harness.agent_id)
                    .await
                    .expect("template lookup should succeed")
                    .expect("agent snapshot should exist"),
            },
        )
        .await
        .expect("child metadata should persist");
        JobRepository::create(
            harness.sqlite.as_ref(),
            &JobRecord {
                id: argus_repository::types::JobId::new(job_id.clone()),
                job_type: JobType::Standalone,
                name: format!("job:{job_id}"),
                status: JobStatus::Pending,
                agent_id: RepoAgentId::new(harness.agent_id.inner()),
                context: None,
                prompt: "persisted child".to_string(),
                thread_id: Some(child_thread_id),
                group_id: None,
                depends_on: Vec::new(),
                cron_expr: None,
                scheduled_at: None,
                started_at: None,
                finished_at: None,
                parent_job_id: None,
                result: None,
            },
        )
        .await
        .expect("job record should persist");

        let backend = restarted_scheduler_backend(
            Arc::clone(&harness.sqlite),
            Arc::clone(&harness.template_manager),
            harness.temp_dir.path().join("trace"),
            Arc::new(FixedProvider {
                model_name: "routing-test".to_string(),
            }),
        );
        let error = backend
            .resolve_message_targets(harness.thread_id, &format!("job:{job_id}"))
            .await
            .expect_err("persisted but unloaded job target should not be mailbox ready");
        assert!(
            error
                .to_string()
                .contains("not ready to receive mailbox messages"),
            "unexpected scheduler error: {error}"
        );
    }

    #[tokio::test]
    async fn scheduler_get_job_result_recovers_completed_job_after_restart() {
        let harness = test_session_manager_harness_with_provider(Arc::new(FixedProvider {
            model_name: "routing-test".to_string(),
        }))
        .await;
        let child_thread_id = ThreadId::new();
        let job_id = "job-restart-result".to_string();
        let child_base_dir = chat_thread_base_dir(
            harness.temp_dir.path().join("trace").as_path(),
            harness.session_id,
            harness.thread_id,
        )
        .join(child_thread_id.to_string());
        persist_thread_metadata(
            &child_base_dir,
            &ThreadTraceMetadata {
                thread_id: child_thread_id,
                kind: ThreadTraceKind::Job,
                root_session_id: Some(harness.session_id),
                parent_thread_id: Some(harness.thread_id),
                job_id: Some(job_id.clone()),
                agent_snapshot: harness
                    .template_manager
                    .get(harness.agent_id)
                    .await
                    .expect("template lookup should succeed")
                    .expect("agent snapshot should exist"),
            },
        )
        .await
        .expect("child metadata should persist");
        JobRepository::create(
            harness.sqlite.as_ref(),
            &JobRecord {
                id: argus_repository::types::JobId::new(job_id.clone()),
                job_type: JobType::Standalone,
                name: format!("job:{job_id}"),
                status: JobStatus::Pending,
                agent_id: RepoAgentId::new(harness.agent_id.inner()),
                context: None,
                prompt: "persisted result".to_string(),
                thread_id: Some(child_thread_id),
                group_id: None,
                depends_on: Vec::new(),
                cron_expr: None,
                scheduled_at: None,
                started_at: None,
                finished_at: None,
                parent_job_id: None,
                result: None,
            },
        )
        .await
        .expect("completed job record should persist");
        JobRepository::update_result(
            harness.sqlite.as_ref(),
            &argus_repository::types::JobId::new(job_id.clone()),
            &JobResult {
                success: true,
                message: "persisted answer".to_string(),
                token_usage: None,
                agent_id: RepoAgentId::new(harness.agent_id.inner()),
                agent_display_name: "Routing Test Agent".to_string(),
                agent_description: "Used to verify session mailbox routing".to_string(),
            },
        )
        .await
        .expect("job result should persist");
        let finished_at = Utc::now().to_rfc3339();
        JobRepository::update_status(
            harness.sqlite.as_ref(),
            &argus_repository::types::JobId::new(job_id.clone()),
            JobStatus::Succeeded,
            None,
            Some(finished_at.as_str()),
        )
        .await
        .expect("job status should persist");

        let backend = restarted_scheduler_backend(
            Arc::clone(&harness.sqlite),
            Arc::clone(&harness.template_manager),
            harness.temp_dir.path().join("trace"),
            Arc::new(FixedProvider {
                model_name: "routing-test".to_string(),
            }),
        );
        let lookup = backend
            .get_job_result(SchedulerLookupRequest {
                thread_id: harness.thread_id,
                job_id: job_id.clone(),
                consume: false,
            })
            .await
            .expect("persisted job result should load");

        match lookup {
            argus_tool::SchedulerJobLookup::Completed(result) => {
                assert_eq!(result.message, "persisted answer");
                assert!(result.success);
            }
            other => panic!("expected completed lookup, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn scheduler_thread_target_recovers_direct_child_relationship_after_restart() {
        let harness = test_session_manager_harness_with_provider(Arc::new(FixedProvider {
            model_name: "routing-test".to_string(),
        }))
        .await;
        let child_thread_id = ThreadId::new();
        let child_base_dir = chat_thread_base_dir(
            harness.temp_dir.path().join("trace").as_path(),
            harness.session_id,
            harness.thread_id,
        )
        .join(child_thread_id.to_string());
        persist_thread_metadata(
            &child_base_dir,
            &ThreadTraceMetadata {
                thread_id: child_thread_id,
                kind: ThreadTraceKind::Job,
                root_session_id: Some(harness.session_id),
                parent_thread_id: Some(harness.thread_id),
                job_id: Some("job-thread-target".to_string()),
                agent_snapshot: harness
                    .template_manager
                    .get(harness.agent_id)
                    .await
                    .expect("template lookup should succeed")
                    .expect("agent snapshot should exist"),
            },
        )
        .await
        .expect("child metadata should persist");

        let backend = restarted_scheduler_backend(
            Arc::clone(&harness.sqlite),
            Arc::clone(&harness.template_manager),
            harness.temp_dir.path().join("trace"),
            Arc::new(FixedProvider {
                model_name: "routing-test".to_string(),
            }),
        );
        let error = backend
            .resolve_message_targets(harness.thread_id, &format!("thread:{child_thread_id}"))
            .await
            .expect_err("persisted child thread should be recognized but not mailbox ready");
        assert!(
            error
                .to_string()
                .contains("not ready to receive mailbox messages"),
            "unexpected scheduler error: {error}"
        );
    }

    #[tokio::test]
    async fn scheduler_list_subagents_resolves_names_from_current_agent() {
        let harness = test_session_manager_harness_with_provider(Arc::new(FixedProvider {
            model_name: "routing-test".to_string(),
        }))
        .await;

        harness
            .template_manager
            .upsert(AgentRecord {
                id: AgentId::new(12),
                display_name: "Researcher".to_string(),
                description: "Researches things".to_string(),
                version: "1.0.0".to_string(),
                provider_id: None,
                model_id: None,
                system_prompt: "Research.".to_string(),
                tool_names: vec![],
                subagent_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: None,
            })
            .await
            .expect("subagent should upsert");
        harness
            .template_manager
            .upsert(AgentRecord {
                id: harness.agent_id,
                display_name: "Routing Test Agent".to_string(),
                description: "Used to verify session mailbox routing".to_string(),
                version: "1.0.0".to_string(),
                provider_id: Some(ProviderId::new(1)),
                model_id: Some("routing-test".to_string()),
                system_prompt: "You are a routing test agent.".to_string(),
                tool_names: vec![],
                subagent_names: vec!["Researcher".to_string(), "Missing".to_string()],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::disabled()),
            })
            .await
            .expect("parent should upsert");

        let backend = restarted_scheduler_backend(
            Arc::clone(&harness.sqlite),
            Arc::clone(&harness.template_manager),
            harness.temp_dir.path().join("trace"),
            Arc::new(FixedProvider {
                model_name: "routing-test".to_string(),
            }),
        );

        set_current_agent_id(harness.agent_id);
        let subagents = backend
            .list_subagents()
            .await
            .expect("subagent lookup should succeed");
        clear_current_agent_id();

        assert_eq!(subagents.len(), 1);
        assert_eq!(subagents[0].display_name, "Researcher");
    }

    #[tokio::test]
    async fn scheduler_dispatch_job_rejects_requests_at_maximum_depth() {
        let harness = test_session_manager_harness_with_provider(Arc::new(FixedProvider {
            model_name: "routing-test".to_string(),
        }))
        .await;
        let level_one = ThreadId::new();
        let level_two = ThreadId::new();
        let level_three = ThreadId::new();
        let trace_root = harness.temp_dir.path().join("trace");
        let root_base_dir =
            chat_thread_base_dir(trace_root.as_path(), harness.session_id, harness.thread_id);
        let level_one_dir = root_base_dir.join(level_one.to_string());
        let level_two_dir = level_one_dir.join(level_two.to_string());
        let level_three_dir = level_two_dir.join(level_three.to_string());
        let agent_snapshot = harness
            .template_manager
            .get(harness.agent_id)
            .await
            .expect("template lookup should succeed")
            .expect("agent snapshot should exist");

        for (thread_id, parent_thread_id, base_dir) in [
            (level_one, Some(harness.thread_id), level_one_dir.as_path()),
            (level_two, Some(level_one), level_two_dir.as_path()),
            (level_three, Some(level_two), level_three_dir.as_path()),
        ] {
            persist_thread_metadata(
                base_dir,
                &ThreadTraceMetadata {
                    thread_id,
                    kind: ThreadTraceKind::Job,
                    root_session_id: Some(harness.session_id),
                    parent_thread_id,
                    job_id: Some(format!("job-{thread_id}")),
                    agent_snapshot: agent_snapshot.clone(),
                },
            )
            .await
            .expect("child metadata should persist");
        }

        harness
            .template_manager
            .upsert(AgentRecord {
                id: harness.agent_id,
                display_name: "Routing Test Agent".to_string(),
                description: "Used to verify session mailbox routing".to_string(),
                version: "1.0.0".to_string(),
                provider_id: Some(ProviderId::new(1)),
                model_id: Some("routing-test".to_string()),
                system_prompt: "You are a routing test agent.".to_string(),
                tool_names: vec![],
                subagent_names: vec!["Researcher".to_string()],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::disabled()),
            })
            .await
            .expect("parent should upsert");

        let backend = restarted_scheduler_backend(
            Arc::clone(&harness.sqlite),
            Arc::clone(&harness.template_manager),
            trace_root,
            Arc::new(FixedProvider {
                model_name: "routing-test".to_string(),
            }),
        );
        let (pipe_tx, _) = broadcast::channel(8);

        set_current_agent_id(harness.agent_id);
        let error = backend
            .dispatch_job(SchedulerDispatchRequest {
                thread_id: level_three,
                prompt: "nested work".to_string(),
                agent_id: harness.agent_id,
                context: None,
                pipe_tx,
            })
            .await
            .expect_err("depth limit should reject dispatch");
        clear_current_agent_id();

        assert!(
            error
                .to_string()
                .contains("maximum dispatch depth (3) exceeded"),
            "unexpected scheduler error: {error}"
        );
    }
}
