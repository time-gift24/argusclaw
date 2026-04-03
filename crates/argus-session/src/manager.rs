use std::path::PathBuf;
use std::sync::Arc;

use argus_agent::tool_context::current_agent_id;
use argus_agent::turn_log_store::{recover_thread_log_state, RecoveredThreadLogState};
use argus_job::{JobLookup, JobManager, ThreadPool};
use argus_protocol::{
    llm::{ChatMessage, CompletionRequest, CompletionResponse, LlmError, LlmEventStream},
    AgentId, ArgusError, LlmProviderId, MailboxMessage, MailboxMessageType, ProviderId, Result,
    SessionId, ThreadControlEvent, ThreadEvent, ThreadId, ThreadPoolRuntimeKind, ToolError,
};
use argus_repository::traits::{LlmProviderRepository, SessionRepository, ThreadRepository};
use argus_template::TemplateManager;
use argus_tool::{
    CheckInboxRequest, MarkReadRequest, SchedulerBackend, SchedulerDispatchRequest,
    SchedulerJobLookup, SchedulerJobResult, SchedulerLookupRequest, SchedulerSubagent,
    SchedulerTool, SendMessageRequest, SendMessageResponse, ToolManager,
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
}

impl std::fmt::Debug for SessionSchedulerBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionSchedulerBackend").finish()
    }
}

impl SessionSchedulerBackend {
    fn new(template_manager: Arc<TemplateManager>, job_manager: Arc<JobManager>) -> Self {
        Self {
            template_manager,
            job_manager,
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
        control_tx: &tokio::sync::mpsc::UnboundedSender<ThreadControlEvent>,
        job_id: &str,
    ) -> std::result::Result<(), ToolError> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        if let Err(error) = control_tx.send(ThreadControlEvent::ClaimQueuedJobResult {
            job_id: job_id.to_string(),
            reply_tx,
        }) {
            tracing::warn!(job_id, "failed to enqueue queued-job claim: {error}");
            return Ok(());
        }

        if let Err(error) = reply_rx.await {
            tracing::warn!(job_id, "queued-job claim reply dropped: {error}");
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

    fn active_child_thread_ids(&self, thread_id: ThreadId) -> Vec<ThreadId> {
        let thread_pool = self.thread_pool();
        thread_pool
            .child_thread_ids(&thread_id)
            .into_iter()
            .filter(|child_thread_id| {
                thread_pool
                    .runtime_summary(child_thread_id)
                    .is_some_and(|summary| {
                        summary.runtime.kind == ThreadPoolRuntimeKind::Job
                            && summary
                                .runtime
                                .job_id
                                .as_deref()
                                .is_some_and(|job_id| self.job_manager.is_job_pending(job_id))
                    })
            })
            .collect()
    }

    fn mailbox_ready_child_thread_ids(&self, thread_id: ThreadId) -> Vec<ThreadId> {
        let thread_pool = self.thread_pool();
        self.active_child_thread_ids(thread_id)
            .into_iter()
            .filter(|child_thread_id| thread_pool.loaded_thread(child_thread_id).is_some())
            .collect()
    }

    fn is_thread_target_reachable(
        &self,
        source_thread_id: ThreadId,
        target_thread_id: ThreadId,
    ) -> bool {
        if source_thread_id == target_thread_id {
            return true;
        }

        let thread_pool = self.thread_pool();
        if thread_pool.parent_thread_id(&source_thread_id) == Some(target_thread_id) {
            return true;
        }

        thread_pool
            .child_thread_ids(&source_thread_id)
            .into_iter()
            .any(|child_thread_id| child_thread_id == target_thread_id)
    }

    fn validate_mailbox_target_ready(
        &self,
        target_thread_id: ThreadId,
    ) -> std::result::Result<(), ToolError> {
        let thread_pool = self.thread_pool();
        let summary = thread_pool
            .runtime_summary(&target_thread_id)
            .ok_or_else(|| {
                Self::scheduler_error(format!("thread {target_thread_id} is not registered"))
            })?;

        if summary.runtime.kind == ThreadPoolRuntimeKind::Job
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

        for child_thread_id in self.mailbox_ready_child_thread_ids(thread_id) {
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
            if !self.job_manager.is_job_pending(job_id) {
                return Err(Self::scheduler_error(format!("job {job_id} is not active")));
            }
            let target = thread_pool.get_thread_binding(job_id).ok_or_else(|| {
                Self::scheduler_error(format!("job {job_id} is not bound to a thread"))
            })?;
            self.validate_mailbox_target_ready(target).map_err(|_| {
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
            if !self.is_thread_target_reachable(thread_id, target) {
                return Err(Self::scheduler_error(format!(
                    "thread {target} is not reachable from the current thread"
                )));
            }
            self.validate_mailbox_target_ready(target)?;
            return Ok(vec![target]);
        }

        if to == "parent" {
            let parent = thread_pool.parent_thread_id(&thread_id).ok_or_else(|| {
                Self::scheduler_error("current thread does not have a direct parent")
            })?;
            self.validate_mailbox_target_ready(parent)?;
            return Ok(vec![parent]);
        }

        if to == "*" {
            let children = self.mailbox_ready_child_thread_ids(thread_id);
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
                request.control_tx,
            )
            .await
            .map_err(|error| ToolError::ExecutionFailed {
                tool_name: "scheduler".to_string(),
                reason: error.to_string(),
            })?;

        Ok(job_id)
    }

    async fn list_subagents(&self) -> std::result::Result<Vec<SchedulerSubagent>, ToolError> {
        let agent_id = current_agent_id().ok_or_else(|| ToolError::ExecutionFailed {
            tool_name: "list_subagents".to_string(),
            reason: "current agent_id not available".to_string(),
        })?;
        let records = self
            .template_manager
            .list_subagents(agent_id)
            .await
            .map_err(|error| ToolError::ExecutionFailed {
                tool_name: "scheduler".to_string(),
                reason: error.to_string(),
            })?;

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
        let lookup =
            self.job_manager
                .get_job_result_status(request.thread_id, &request.job_id, false);

        if request.consume && matches!(lookup, JobLookup::Completed(_)) {
            self.claim_queued_runtime_result(&request.control_tx, &request.job_id)
                .await?;
            let consumed_lookup =
                self.job_manager
                    .get_job_result_status(request.thread_id, &request.job_id, true);
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

            thread_pool
                .deliver_mailbox_message(*target, message)
                .await
                .map_err(|error| Self::scheduler_error(error.to_string()))?;
        }

        Ok(SendMessageResponse {
            delivered: targets.len(),
            thread_ids: targets,
        })
    }

    async fn check_inbox(
        &self,
        request: CheckInboxRequest,
    ) -> std::result::Result<Vec<MailboxMessage>, ToolError> {
        self.thread_pool()
            .unread_mailbox_messages(request.thread_id)
            .await
            .map_err(|error| Self::scheduler_error(error.to_string()))
    }

    async fn mark_read(&self, request: MarkReadRequest) -> std::result::Result<(), ToolError> {
        let marked = self
            .thread_pool()
            .mark_mailbox_message_read(request.thread_id, &request.message_id)
            .await
            .map_err(|error| Self::scheduler_error(error.to_string()))?;
        if !marked {
            return Err(Self::scheduler_error(format!(
                "message {} was not found in the current inbox",
                request.message_id
            )));
        }

        Ok(())
    }
}

/// Manages sessions and their threads.
#[derive(Clone)]
pub struct SessionManager {
    session_repo: Arc<dyn SessionRepository>,
    thread_repo: Arc<dyn ThreadRepository>,
    llm_provider_repo: Arc<dyn LlmProviderRepository>,
    sessions: DashMap<SessionId, Arc<Session>>,
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
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
        tool_manager: Arc<ToolManager>,
        trace_dir: PathBuf,
        thread_pool: Arc<ThreadPool>,
        job_manager: Arc<JobManager>,
    ) -> Self {
        let scheduler_backend = Arc::new(SessionSchedulerBackend::new(
            template_manager.clone(),
            job_manager.clone(),
        ));
        tool_manager.register(Arc::new(SchedulerTool::new(scheduler_backend.clone())));

        Self {
            session_repo,
            thread_repo,
            llm_provider_repo,
            sessions: DashMap::new(),
            template_manager,
            provider_resolver,
            trace_dir,
            thread_pool,
        }
    }

    /// Broadcast a ThreadEvent to all active sessions.
    pub fn broadcast_event(&self, event: ThreadEvent) {
        for session in self.sessions.iter() {
            session.value().broadcast(event.clone());
        }
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

        if let Err(e) = self.ensure_session_dir(session_id).await {
            tracing::warn!(session_id = %session_id, error = %e, "Failed to ensure session directory");
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
                .thread_pool
                .ensure_chat_runtime(session_id, thread_record.id)
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
        self.sessions.remove(&session_id);
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

        // Create session trace directory with meta.json
        if let Err(e) = self.ensure_session_dir(session_id).await {
            tracing::warn!(session_id = %session_id, error = %e, "Failed to create session directory");
        }

        Ok(session_id)
    }

    /// Ensure the session trace directory exists with meta.json.
    /// Idempotent: safe to call multiple times.
    pub async fn ensure_session_dir(&self, session_id: SessionId) -> std::io::Result<()> {
        let session_dir = self.trace_dir.join(session_id.to_string());
        tokio::fs::create_dir_all(&session_dir).await?;
        let meta_path = session_dir.join("meta.json");

        // Only create meta.json if it doesn't exist
        if !meta_path.exists() {
            let meta = serde_json::json!({
                "session_id": session_id.to_string(),
                "current_turn": 0,
            });
            tokio::fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        }

        Ok(())
    }

    /// Update the current_turn in meta.json after a turn completes.
    pub async fn update_session_turn(
        &self,
        session_id: SessionId,
        turn_number: u32,
    ) -> std::io::Result<()> {
        let meta_path = self
            .trace_dir
            .join(session_id.to_string())
            .join("meta.json");

        let meta = if meta_path.exists() {
            let content = tokio::fs::read_to_string(&meta_path).await?;
            serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| {
                serde_json::json!({
                    "session_id": session_id.to_string(),
                    "current_turn": 0,
                })
            })
        } else {
            serde_json::json!({
                "session_id": session_id.to_string(),
                "current_turn": 0,
            })
        };

        let updated = serde_json::json!({
            "session_id": meta.get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or(&session_id.to_string()),
            "current_turn": turn_number,
        });
        tokio::fs::write(&meta_path, serde_json::to_string_pretty(&updated)?).await?;
        Ok(())
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
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;
        self.thread_pool.register_chat_thread(session_id, thread_id);
        let thread = match self
            .thread_pool
            .ensure_chat_runtime(session_id, thread_id)
            .await
        {
            Ok(thread) => thread,
            Err(error) => {
                self.thread_pool.remove_runtime(&thread_id);
                let _ = self.thread_repo.delete_thread(&thread_id).await;
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
        self.load(session_id).await?;
        self.ensure_thread_in_session(session_id, thread_id).await?;
        self.thread_pool
            .send_chat_message(session_id, *thread_id, message)
            .await
            .map_err(|e| ArgusError::LlmError {
                reason: e.to_string(),
            })
    }

    /// Send a cancel/interrupt signal to a specific thread's active turn.
    pub async fn cancel_thread(&self, session_id: SessionId, thread_id: &ThreadId) -> Result<()> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or(ArgusError::SessionNotFound(session_id))?;

        let thread = session
            .get_thread(thread_id)
            .or_else(|| self.thread_pool.loaded_chat_thread(thread_id))
            .ok_or(ArgusError::ThreadNotFound(thread_id.to_string()))?;

        let result = thread
            .read()
            .await
            .send_control_event(ThreadControlEvent::UserInterrupt {
                content: "stop".to_string(),
            });
        result.map_err(|e| ArgusError::LlmError {
            reason: e.to_string(),
        })
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
                return Ok(thread.history().to_vec());
            }
            let recovered = recover_messages_from_trace(
                &self.trace_dir,
                &session_id,
                thread_id,
                thread.turn_count(),
            )
            .await?;
            if !recovered.is_empty() {
                return Ok(recovered);
            }
            return Ok(thread.history().to_vec());
        }
        let thread_record = self
            .thread_repo
            .get_thread_in_session(thread_id, &session_id)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?
            .ok_or_else(|| ArgusError::ThreadNotFound(thread_id.inner().to_string()))?;
        recover_messages_from_trace(
            &self.trace_dir,
            &session_id,
            thread_id,
            thread_record.turn_count as u32,
        )
        .await
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
                        thread.history().to_vec(),
                        thread.turn_count(),
                        thread.token_count(),
                    )
                } else {
                    let recovered = recover_thread_state_from_trace(
                        &self.trace_dir,
                        &session_id,
                        thread_id,
                        Some(thread.turn_count()),
                    )
                    .await?;
                    if recovered.turn_count > 0 {
                        (
                            recovered.messages,
                            recovered.turn_count,
                            recovered.token_count,
                        )
                    } else {
                        (
                            thread.history().to_vec(),
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
        let recovered = recover_thread_state_from_trace(
            &self.trace_dir,
            &session_id,
            thread_id,
            (thread_record.turn_count > 0).then_some(thread_record.turn_count),
        )
        .await?;

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
    turn_count: u32,
) -> Result<Vec<ChatMessage>> {
    Ok(
        recover_thread_state_from_trace(trace_dir, session_id, thread_id, Some(turn_count))
            .await?
            .messages,
    )
}

async fn recover_thread_state_from_trace(
    trace_dir: &std::path::Path,
    session_id: &SessionId,
    thread_id: &ThreadId,
    turn_count_hint: Option<u32>,
) -> Result<RecoveredThreadState> {
    let base_dir = trace_dir
        .join(session_id.to_string())
        .join(thread_id.to_string());
    let recovered = recover_thread_log_state(&base_dir, turn_count_hint)
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
    use std::collections::HashMap;
    use std::fs;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use argus_agent::history::TurnState;
    use argus_agent::turn_log_store::{
        turn_messages_path, turn_meta_path, turns_dir, write_turn_messages, write_turn_meta,
        TurnLogMeta,
    };
    use argus_agent::{CompactResult, Compactor, ThreadBuilder};
    use argus_protocol::llm::{
        ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError,
    };
    use argus_protocol::{
        AgentId, AgentRecord, AgentType, LlmProviderKind, LlmProviderRecord, MailboxMessage,
        MailboxMessageType, ProviderId, ProviderResolver, ProviderSecretStatus, Role, SecretString,
        SessionId, ThinkingConfig, ThreadControlEvent, ThreadEvent, ThreadId,
    };
    use argus_repository::traits::{
        AgentRepository, JobRepository, LlmProviderRepository, SessionRepository, ThreadRepository,
    };
    use argus_repository::{migrate, ArgusSqlite};
    use argus_template::TemplateManager;
    use argus_tool::{
        CheckInboxRequest, MarkReadRequest, SchedulerBackend, SendMessageRequest, ToolManager,
    };
    use async_trait::async_trait;
    use chrono::Utc;
    use rust_decimal::Decimal;
    use sqlx::SqlitePool;
    use tokio::time::{sleep, timeout};

    use super::{
        recover_messages_from_trace, recover_thread_state_from_trace, Session, SessionManager,
        SessionSchedulerBackend,
    };

    struct NoopCompactor;

    #[async_trait]
    impl Compactor for NoopCompactor {
        async fn compact(
            &self,
            _provider: &dyn argus_protocol::LlmProvider,
            _messages: &[ChatMessage],
            _token_count: u32,
        ) -> Result<Option<CompactResult>, argus_agent::CompactError> {
            Ok(None)
        }

        fn name(&self) -> &'static str {
            "noop"
        }
    }

    #[derive(Debug)]
    struct CapturingProvider {
        response: String,
        delay: Duration,
        captured_user_inputs: Arc<Mutex<Vec<String>>>,
    }

    impl CapturingProvider {
        fn new(response: &str, delay: Duration) -> Self {
            Self {
                response: response.to_string(),
                delay,
                captured_user_inputs: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn captured_user_inputs(&self) -> Vec<String> {
            self.captured_user_inputs.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl argus_protocol::LlmProvider for CapturingProvider {
        fn model_name(&self) -> &str {
            "capturing"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> std::result::Result<CompletionResponse, LlmError> {
            let last_user_input = request
                .messages
                .iter()
                .rev()
                .find(|message| message.role == argus_protocol::Role::User)
                .map(|message| message.content.clone())
                .unwrap_or_default();
            self.captured_user_inputs
                .lock()
                .unwrap()
                .push(last_user_input);

            sleep(self.delay).await;

            Ok(CompletionResponse {
                content: Some(self.response.clone()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 12,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }
    }

    struct FixedProviderResolver {
        provider: Arc<dyn argus_protocol::LlmProvider>,
    }

    impl FixedProviderResolver {
        fn new(provider: Arc<dyn argus_protocol::LlmProvider>) -> Self {
            Self { provider }
        }
    }

    #[async_trait]
    impl ProviderResolver for FixedProviderResolver {
        async fn resolve(
            &self,
            _id: ProviderId,
        ) -> argus_protocol::Result<Arc<dyn argus_protocol::LlmProvider>> {
            Ok(Arc::clone(&self.provider))
        }

        async fn default_provider(
            &self,
        ) -> argus_protocol::Result<Arc<dyn argus_protocol::LlmProvider>> {
            Ok(Arc::clone(&self.provider))
        }

        async fn resolve_with_model(
            &self,
            _id: ProviderId,
            _model: &str,
        ) -> argus_protocol::Result<Arc<dyn argus_protocol::LlmProvider>> {
            Ok(Arc::clone(&self.provider))
        }
    }

    fn sample_provider_record(is_default: bool) -> LlmProviderRecord {
        LlmProviderRecord {
            id: argus_protocol::LlmProviderId::new(0),
            display_name: "session-manager-provider".to_string(),
            kind: LlmProviderKind::OpenAiCompatible,
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: SecretString::new("test-key"),
            models: vec!["capturing".to_string()],
            model_config: HashMap::new(),
            default_model: "capturing".to_string(),
            is_default,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::Ready,
            meta_data: HashMap::new(),
        }
    }

    async fn test_session_manager() -> SessionManager {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite memory pool should connect");
        migrate(&pool).await.expect("migration should succeed");
        let sqlite = Arc::new(ArgusSqlite::new(pool));
        let provider_id =
            LlmProviderRepository::upsert_provider(sqlite.as_ref(), &sample_provider_record(true))
                .await
                .expect("provider upsert should succeed");

        let template_manager = Arc::new(TemplateManager::new(
            sqlite.clone() as Arc<dyn AgentRepository>,
            sqlite.clone(),
        ));
        template_manager
            .upsert(AgentRecord {
                id: AgentId::new(7),
                display_name: "Session Test Agent".to_string(),
                description: "Used to verify session loading".to_string(),
                version: "1.0.0".to_string(),
                provider_id: Some(ProviderId::new(provider_id.into_inner())),
                model_id: Some("capturing".to_string()),
                system_prompt: "You are a test session agent.".to_string(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::enabled()),
                parent_agent_id: None,
                agent_type: AgentType::Standard,
            })
            .await
            .expect("agent upsert should succeed");

        let provider = Arc::new(CapturingProvider::new("hello", Duration::from_millis(5)));
        let provider_resolver =
            Arc::new(FixedProviderResolver::new(provider)) as Arc<dyn ProviderResolver>;
        let tool_manager = Arc::new(ToolManager::new());
        let trace_dir =
            std::env::temp_dir().join(format!("argus-session-manager-tests-{}", SessionId::new()));
        let job_manager = Arc::new(argus_job::JobManager::new_with_repositories(
            template_manager.clone(),
            Arc::clone(&provider_resolver),
            tool_manager.clone(),
            Arc::new(NoopCompactor) as Arc<dyn Compactor>,
            trace_dir.clone(),
            sqlite.clone() as Arc<dyn JobRepository>,
            sqlite.clone() as Arc<dyn ThreadRepository>,
            sqlite.clone() as Arc<dyn LlmProviderRepository>,
        ));

        SessionManager::new(
            sqlite.clone() as Arc<dyn SessionRepository>,
            sqlite.clone() as Arc<dyn ThreadRepository>,
            sqlite as Arc<dyn LlmProviderRepository>,
            template_manager,
            provider_resolver,
            tool_manager,
            trace_dir,
            job_manager.thread_pool(),
            job_manager,
        )
    }

    async fn test_session_manager_with_tool_manager() -> (SessionManager, Arc<ToolManager>) {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite memory pool should connect");
        migrate(&pool).await.expect("migration should succeed");
        let sqlite = Arc::new(ArgusSqlite::new(pool));
        let provider_id =
            LlmProviderRepository::upsert_provider(sqlite.as_ref(), &sample_provider_record(true))
                .await
                .expect("provider upsert should succeed");

        let template_manager = Arc::new(TemplateManager::new(
            sqlite.clone() as Arc<dyn AgentRepository>,
            sqlite.clone(),
        ));
        template_manager
            .upsert(AgentRecord {
                id: AgentId::new(7),
                display_name: "Session Test Agent".to_string(),
                description: "Used to verify session loading".to_string(),
                version: "1.0.0".to_string(),
                provider_id: Some(ProviderId::new(provider_id.into_inner())),
                model_id: Some("capturing".to_string()),
                system_prompt: "You are a test session agent.".to_string(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::enabled()),
                parent_agent_id: None,
                agent_type: AgentType::Standard,
            })
            .await
            .expect("agent upsert should succeed");

        let provider = Arc::new(CapturingProvider::new("hello", Duration::from_millis(5)));
        let provider_resolver =
            Arc::new(FixedProviderResolver::new(provider)) as Arc<dyn ProviderResolver>;
        let tool_manager = Arc::new(ToolManager::new());
        let trace_dir =
            std::env::temp_dir().join(format!("argus-session-manager-tests-{}", SessionId::new()));
        let job_manager = Arc::new(argus_job::JobManager::new_with_repositories(
            template_manager.clone(),
            Arc::clone(&provider_resolver),
            tool_manager.clone(),
            Arc::new(NoopCompactor) as Arc<dyn Compactor>,
            trace_dir.clone(),
            sqlite.clone() as Arc<dyn JobRepository>,
            sqlite.clone() as Arc<dyn ThreadRepository>,
            sqlite.clone() as Arc<dyn LlmProviderRepository>,
        ));

        (
            SessionManager::new(
                sqlite.clone() as Arc<dyn SessionRepository>,
                sqlite.clone() as Arc<dyn ThreadRepository>,
                sqlite as Arc<dyn LlmProviderRepository>,
                template_manager,
                provider_resolver,
                tool_manager.clone(),
                trace_dir,
                job_manager.thread_pool(),
                job_manager,
            ),
            tool_manager,
        )
    }

    async fn test_session_manager_with_provider_delay(
        delay: Duration,
    ) -> (SessionManager, SessionSchedulerBackend) {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite memory pool should connect");
        migrate(&pool).await.expect("migration should succeed");
        let sqlite = Arc::new(ArgusSqlite::new(pool));
        let provider_id =
            LlmProviderRepository::upsert_provider(sqlite.as_ref(), &sample_provider_record(true))
                .await
                .expect("provider upsert should succeed");

        let template_manager = Arc::new(TemplateManager::new(
            sqlite.clone() as Arc<dyn AgentRepository>,
            sqlite.clone(),
        ));
        template_manager
            .upsert(AgentRecord {
                id: AgentId::new(7),
                display_name: "Session Test Agent".to_string(),
                description: "Used to verify session loading".to_string(),
                version: "1.0.0".to_string(),
                provider_id: Some(ProviderId::new(provider_id.into_inner())),
                model_id: Some("capturing".to_string()),
                system_prompt: "You are a test session agent.".to_string(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::enabled()),
                parent_agent_id: None,
                agent_type: AgentType::Standard,
            })
            .await
            .expect("agent upsert should succeed");

        let provider = Arc::new(CapturingProvider::new("hello", delay));
        let provider_resolver =
            Arc::new(FixedProviderResolver::new(provider)) as Arc<dyn ProviderResolver>;
        let tool_manager = Arc::new(ToolManager::new());
        let trace_dir =
            std::env::temp_dir().join(format!("argus-session-manager-tests-{}", SessionId::new()));
        let job_manager = Arc::new(argus_job::JobManager::new_with_repositories(
            template_manager.clone(),
            Arc::clone(&provider_resolver),
            tool_manager.clone(),
            Arc::new(NoopCompactor) as Arc<dyn Compactor>,
            trace_dir.clone(),
            sqlite.clone() as Arc<dyn JobRepository>,
            sqlite.clone() as Arc<dyn ThreadRepository>,
            sqlite.clone() as Arc<dyn LlmProviderRepository>,
        ));

        let backend =
            SessionSchedulerBackend::new(template_manager.clone(), Arc::clone(&job_manager));
        let manager = SessionManager::new(
            sqlite.clone() as Arc<dyn SessionRepository>,
            sqlite.clone() as Arc<dyn ThreadRepository>,
            sqlite as Arc<dyn LlmProviderRepository>,
            template_manager,
            provider_resolver,
            tool_manager,
            trace_dir,
            job_manager.thread_pool(),
            job_manager,
        );

        (manager, backend)
    }

    fn test_agent_record() -> Arc<AgentRecord> {
        Arc::new(AgentRecord {
            id: AgentId::new(1),
            display_name: "Main Agent".to_string(),
            description: "Main orchestration agent".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(ProviderId::new(1)),
            model_id: None,
            system_prompt: "You are a test orchestrator.".to_string(),
            tool_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: None,
            parent_agent_id: None,
            agent_type: AgentType::Standard,
        })
    }

    fn build_test_thread(
        session_id: SessionId,
        provider: Arc<CapturingProvider>,
    ) -> Arc<tokio::sync::RwLock<argus_agent::Thread>> {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        Arc::new(tokio::sync::RwLock::new(
            ThreadBuilder::new()
                .provider(provider)
                .compactor(compactor)
                .agent_record(test_agent_record())
                .session_id(session_id)
                .build()
                .expect("thread should build"),
        ))
    }

    async fn wait_for_idle(
        thread: &Arc<tokio::sync::RwLock<argus_agent::Thread>>,
        expected_count: usize,
    ) {
        let mut rx = {
            let guard = thread.read().await;
            guard.subscribe()
        };
        let mut idle_count = 0usize;
        timeout(Duration::from_secs(5), async {
            loop {
                match rx.recv().await {
                    Ok(ThreadEvent::Idle { .. }) => {
                        idle_count += 1;
                        if idle_count >= expected_count {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => {}
                }
            }
        })
        .await
        .expect("thread should emit idle");
    }

    #[tokio::test]
    async fn recover_turns_from_messages_jsonl() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let turn_logs_dir = turns_dir(
            &temp_dir
                .path()
                .join(session_id.to_string())
                .join(thread_id.to_string()),
        );
        fs::create_dir_all(&turn_logs_dir).expect("turns dir should exist");

        write_turn_messages(
            &turn_messages_path(&turn_logs_dir, 1),
            &[ChatMessage::user("hi"), ChatMessage::assistant("hello")],
        )
        .await
        .expect("messages should write");
        write_turn_meta(
            &turn_meta_path(&turn_logs_dir, 1),
            &TurnLogMeta {
                turn_number: 1,
                state: TurnState::Completed,
                token_usage: Some(argus_protocol::TokenUsage {
                    input_tokens: 1,
                    output_tokens: 1,
                    total_tokens: 2,
                }),
                started_at: Utc::now(),
                finished_at: Some(Utc::now()),
                model: Some("test-model".to_string()),
                error: None,
            },
        )
        .await
        .expect("meta should write");

        let recovered =
            recover_thread_state_from_trace(temp_dir.path(), &session_id, &thread_id, Some(1))
                .await
                .expect("new-format turn logs should recover");

        assert_eq!(recovered.turn_count, 1);
        assert_eq!(recovered.token_count, 2);
        assert_eq!(recovered.messages.len(), 2);
        assert_eq!(recovered.messages[0].content, "hi");
        assert_eq!(recovered.messages[1].content, "hello");
    }

    #[tokio::test]
    async fn get_thread_snapshot_flattens_turns_for_existing_callers() {
        let manager = test_session_manager().await;
        let session_id = manager
            .create("snapshot compatibility".to_string())
            .await
            .expect("session should create");
        let thread_id = manager
            .create_thread(session_id, AgentId::new(7), None, None)
            .await
            .expect("thread should create");
        let trace_turns_dir = turns_dir(
            &manager
                .trace_dir
                .join(session_id.to_string())
                .join(thread_id.to_string()),
        );
        fs::create_dir_all(&trace_turns_dir).expect("turns dir should exist");

        write_turn_messages(
            &turn_messages_path(&trace_turns_dir, 1),
            &[ChatMessage::user("hi"), ChatMessage::assistant("hello")],
        )
        .await
        .expect("first turn messages should write");
        write_turn_meta(
            &turn_meta_path(&trace_turns_dir, 1),
            &TurnLogMeta {
                turn_number: 1,
                state: TurnState::Completed,
                token_usage: Some(argus_protocol::TokenUsage {
                    input_tokens: 1,
                    output_tokens: 1,
                    total_tokens: 2,
                }),
                started_at: Utc::now(),
                finished_at: Some(Utc::now()),
                model: Some("test-model".to_string()),
                error: None,
            },
        )
        .await
        .expect("first turn meta should write");
        write_turn_messages(
            &turn_messages_path(&trace_turns_dir, 2),
            &[ChatMessage::user("again"), ChatMessage::assistant("done")],
        )
        .await
        .expect("second turn messages should write");
        write_turn_meta(
            &turn_meta_path(&trace_turns_dir, 2),
            &TurnLogMeta {
                turn_number: 2,
                state: TurnState::Completed,
                token_usage: Some(argus_protocol::TokenUsage {
                    input_tokens: 2,
                    output_tokens: 4,
                    total_tokens: 6,
                }),
                started_at: Utc::now(),
                finished_at: Some(Utc::now()),
                model: Some("test-model".to_string()),
                error: None,
            },
        )
        .await
        .expect("second turn meta should write");

        manager
            .unload(session_id)
            .await
            .expect("session should unload");

        let (messages, turn_count, token_count, _) = manager
            .get_thread_snapshot(session_id, &thread_id)
            .await
            .expect("snapshot should recover");

        assert_eq!(turn_count, 2);
        assert_eq!(token_count, 6);
        assert_eq!(messages.len(), 4);
        assert_eq!(
            messages.last().map(|message| message.content.as_str()),
            Some("done")
        );
    }

    #[tokio::test]
    async fn recover_messages_from_trace_restores_full_turn_history() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let turn_logs_dir = turns_dir(
            &temp_dir
                .path()
                .join(session_id.to_string())
                .join(thread_id.to_string()),
        );
        fs::create_dir_all(&turn_logs_dir).expect("turns dir should exist");

        write_turn_messages(
            &turn_messages_path(&turn_logs_dir, 1),
            &[
                ChatMessage::user("用户问题一"),
                ChatMessage::assistant("总结一"),
                ChatMessage::tool_result("call_1", "bash", "'/tmp'"),
            ],
        )
        .await
        .expect("turn one messages should write");
        write_turn_meta(
            &turn_meta_path(&turn_logs_dir, 1),
            &TurnLogMeta {
                turn_number: 1,
                state: TurnState::Completed,
                token_usage: Some(argus_protocol::TokenUsage {
                    input_tokens: 10,
                    output_tokens: 5,
                    total_tokens: 15,
                }),
                started_at: Utc::now(),
                finished_at: Some(Utc::now()),
                model: Some("test-model".to_string()),
                error: None,
            },
        )
        .await
        .expect("turn one meta should write");
        write_turn_messages(
            &turn_messages_path(&turn_logs_dir, 2),
            &[
                ChatMessage::user("用户问题二"),
                ChatMessage::assistant("总结二"),
            ],
        )
        .await
        .expect("turn two messages should write");
        write_turn_meta(
            &turn_meta_path(&turn_logs_dir, 2),
            &TurnLogMeta {
                turn_number: 2,
                state: TurnState::Completed,
                token_usage: Some(argus_protocol::TokenUsage {
                    input_tokens: 20,
                    output_tokens: 8,
                    total_tokens: 28,
                }),
                started_at: Utc::now(),
                finished_at: Some(Utc::now()),
                model: Some("test-model".to_string()),
                error: None,
            },
        )
        .await
        .expect("turn two meta should write");

        let messages = recover_messages_from_trace(temp_dir.path(), &session_id, &thread_id, 2)
            .await
            .expect("committed log recovery should succeed");

        assert_eq!(messages.len(), 5);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].content, "用户问题一");
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(messages[1].content, "总结一");
        assert_eq!(messages[2].role, Role::Tool);
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(messages[2].name.as_deref(), Some("bash"));
        assert_eq!(messages[3].role, Role::User);
        assert_eq!(messages[3].content, "用户问题二");
        assert_eq!(messages[4].role, Role::Assistant);
        assert_eq!(messages[4].content, "总结二");
    }

    #[tokio::test]
    async fn recover_messages_from_trace_fails_when_turn_file_is_missing() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let turn_logs_dir = turns_dir(
            &temp_dir
                .path()
                .join(session_id.to_string())
                .join(thread_id.to_string()),
        );
        fs::create_dir_all(&turn_logs_dir).expect("turns dir should exist");

        write_turn_messages(
            &turn_messages_path(&turn_logs_dir, 1),
            &[ChatMessage::user("hi"), ChatMessage::assistant("hello")],
        )
        .await
        .expect("turn one messages should write");
        write_turn_meta(
            &turn_meta_path(&turn_logs_dir, 1),
            &TurnLogMeta {
                turn_number: 1,
                state: TurnState::Completed,
                token_usage: Some(argus_protocol::TokenUsage {
                    input_tokens: 10,
                    output_tokens: 5,
                    total_tokens: 15,
                }),
                started_at: Utc::now(),
                finished_at: Some(Utc::now()),
                model: Some("test-model".to_string()),
                error: None,
            },
        )
        .await
        .expect("turn one meta should write");

        let error = recover_messages_from_trace(temp_dir.path(), &session_id, &thread_id, 2)
            .await
            .expect_err("missing turn file should fail");

        assert!(error.to_string().contains("missing turn trace file 2"));
    }

    #[tokio::test]
    async fn recover_messages_from_trace_uses_turns_beyond_persisted_count_hint() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let turn_logs_dir = turns_dir(
            &temp_dir
                .path()
                .join(session_id.to_string())
                .join(thread_id.to_string()),
        );
        fs::create_dir_all(&turn_logs_dir).expect("turns dir should exist");

        for (turn_number, user, assistant, total_tokens) in [
            (1, "用户问题一", "回答一", 15),
            (2, "用户问题二", "回答二", 28),
        ] {
            write_turn_messages(
                &turn_messages_path(&turn_logs_dir, turn_number),
                &[ChatMessage::user(user), ChatMessage::assistant(assistant)],
            )
            .await
            .expect("turn messages should write");
            write_turn_meta(
                &turn_meta_path(&turn_logs_dir, turn_number),
                &TurnLogMeta {
                    turn_number,
                    state: TurnState::Completed,
                    token_usage: Some(argus_protocol::TokenUsage {
                        input_tokens: total_tokens / 2,
                        output_tokens: total_tokens - (total_tokens / 2),
                        total_tokens,
                    }),
                    started_at: Utc::now(),
                    finished_at: Some(Utc::now()),
                    model: Some("test-model".to_string()),
                    error: None,
                },
            )
            .await
            .expect("turn meta should write");
        }

        let messages = recover_messages_from_trace(temp_dir.path(), &session_id, &thread_id, 1)
            .await
            .expect("recovery should discover committed turns beyond the persisted count hint");

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].content, "用户问题一");
        assert_eq!(messages[1].content, "回答一");
        assert_eq!(messages[2].content, "用户问题二");
        assert_eq!(messages[3].content, "回答二");
    }

    #[tokio::test]
    async fn recover_thread_state_from_trace_infers_counts_from_files_when_db_counts_are_missing() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let turn_logs_dir = turns_dir(
            &temp_dir
                .path()
                .join(session_id.to_string())
                .join(thread_id.to_string()),
        );
        fs::create_dir_all(&turn_logs_dir).expect("turns dir should exist");

        for (turn_number, user, assistant, total_tokens) in
            [(1, "hi", "hello", 15), (2, "again", "welcome back", 28)]
        {
            write_turn_messages(
                &turn_messages_path(&turn_logs_dir, turn_number),
                &[ChatMessage::user(user), ChatMessage::assistant(assistant)],
            )
            .await
            .expect("turn messages should write");
            write_turn_meta(
                &turn_meta_path(&turn_logs_dir, turn_number),
                &TurnLogMeta {
                    turn_number,
                    state: TurnState::Completed,
                    token_usage: Some(argus_protocol::TokenUsage {
                        input_tokens: total_tokens / 2,
                        output_tokens: total_tokens - (total_tokens / 2),
                        total_tokens,
                    }),
                    started_at: Utc::now(),
                    finished_at: Some(Utc::now()),
                    model: Some("test-model".to_string()),
                    error: None,
                },
            )
            .await
            .expect("turn meta should write");
        }

        let recovered =
            recover_thread_state_from_trace(temp_dir.path(), &session_id, &thread_id, None)
                .await
                .expect("committed log recovery should succeed");

        assert_eq!(recovered.turn_count, 2);
        assert_eq!(recovered.token_count, 28);
        assert_eq!(recovered.messages.len(), 4);
        assert_eq!(recovered.messages[0].content, "hi");
        assert_eq!(recovered.messages[3].content, "welcome back");
    }

    #[tokio::test]
    async fn recover_thread_state_from_trace_returns_empty_when_turns_dir_is_missing() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();

        let recovered =
            recover_thread_state_from_trace(temp_dir.path(), &session_id, &thread_id, None)
                .await
                .expect("missing turns dir should be treated as empty history");

        assert_eq!(recovered.turn_count, 0);
        assert_eq!(recovered.token_count, 0);
        assert!(recovered.messages.is_empty());
    }

    #[tokio::test]
    async fn busy_thread_remains_visible_while_orchestrator_runs_turn() {
        let session_id = SessionId::new();
        let session = Arc::new(Session::new(session_id, "Test".to_string()));
        let provider = Arc::new(CapturingProvider::new("done", Duration::from_millis(150)));
        let thread = build_test_thread(session_id, Arc::clone(&provider));
        let thread_id = thread.read().await.id();

        argus_agent::Thread::spawn_reactor(Arc::clone(&thread));
        session.add_thread(Arc::clone(&thread));

        {
            let guard = thread.read().await;
            guard
                .send_user_message("hello".to_string(), None)
                .expect("message should queue");
        }

        sleep(Duration::from_millis(30)).await;

        let summaries = session.list_threads().await;
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, thread_id);

        wait_for_idle(&thread, 1).await;
    }

    #[tokio::test]
    async fn idle_job_result_triggers_new_turn_with_synthetic_user_message() {
        let session_id = SessionId::new();
        let provider = Arc::new(CapturingProvider::new(
            "job consumed",
            Duration::from_millis(10),
        ));
        let thread = build_test_thread(session_id, Arc::clone(&provider));
        let thread_id = thread.read().await.id();

        argus_agent::Thread::spawn_reactor(Arc::clone(&thread));

        {
            let guard = thread.read().await;
            guard
                .send_control_event(ThreadControlEvent::DeliverMailboxMessage(MailboxMessage {
                    id: "msg-job-42".to_string(),
                    from_thread_id: ThreadId::new(),
                    to_thread_id: thread_id,
                    from_label: "Researcher".to_string(),
                    message_type: MailboxMessageType::JobResult {
                        job_id: "job-42".to_string(),
                        success: true,
                        token_usage: None,
                        agent_id: AgentId::new(99),
                        agent_display_name: "Researcher".to_string(),
                        agent_description: "Investigates background context".to_string(),
                    },
                    text: "completed successfully".to_string(),
                    timestamp: "2026-04-01T00:00:00Z".to_string(),
                    read: false,
                    summary: None,
                }))
                .expect("job result should queue");
        }

        wait_for_idle(&thread, 1).await;

        let captured = provider.captured_user_inputs();
        assert_eq!(captured.len(), 1);
        assert!(captured[0].contains("Job: job-42"));
        assert!(captured[0].contains("Subagent: Researcher"));
        assert!(captured[0].contains("Description: Investigates background context"));
        assert!(captured[0].contains("Result: completed successfully"));
    }

    #[tokio::test]
    async fn running_turn_consumes_job_result_after_idle_if_no_next_iteration_happens() {
        let session_id = SessionId::new();
        let provider = Arc::new(CapturingProvider::new(
            "turn complete",
            Duration::from_millis(120),
        ));
        let thread = build_test_thread(session_id, Arc::clone(&provider));
        let thread_id = thread.read().await.id();

        argus_agent::Thread::spawn_reactor(Arc::clone(&thread));

        {
            let guard = thread.read().await;
            guard
                .send_user_message("initial request".to_string(), None)
                .expect("message should queue");
        }

        sleep(Duration::from_millis(20)).await;

        {
            let guard = thread.read().await;
            guard
                .send_control_event(ThreadControlEvent::DeliverMailboxMessage(MailboxMessage {
                    id: "msg-job-late".to_string(),
                    from_thread_id: ThreadId::new(),
                    to_thread_id: thread_id,
                    from_label: "Builder".to_string(),
                    message_type: MailboxMessageType::JobResult {
                        job_id: "job-late".to_string(),
                        success: true,
                        token_usage: None,
                        agent_id: AgentId::new(100),
                        agent_display_name: "Builder".to_string(),
                        agent_description: "Builds follow-up plans".to_string(),
                    },
                    text: "late background answer".to_string(),
                    timestamp: "2026-04-01T00:00:01Z".to_string(),
                    read: false,
                    summary: None,
                }))
                .expect("job result should queue");
        }

        wait_for_idle(&thread, 1).await;

        let captured = provider.captured_user_inputs();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0], "initial request");
        assert!(captured[1].contains("Job: job-late"));
        assert!(captured[1].contains("Subagent: Builder"));
    }

    #[tokio::test]
    async fn scheduler_backend_resolves_parent_route_to_direct_parent_thread() {
        let (manager, backend) =
            test_session_manager_with_provider_delay(Duration::from_millis(5)).await;
        let session_id = manager
            .create("parent-route".to_string())
            .await
            .expect("session should create");
        let parent_thread_id = manager
            .create_thread(session_id, AgentId::new(7), None, None)
            .await
            .expect("parent thread should create");
        let job_id = "job-parent-route".to_string();

        backend
            .job_manager
            .record_dispatched_job(parent_thread_id, job_id.clone());
        let child_thread_id = manager
            .thread_pool
            .enqueue_job(argus_job::types::ThreadPoolJobRequest {
                originating_thread_id: parent_thread_id,
                job_id,
                agent_id: AgentId::new(7),
                prompt: "route test".to_string(),
                context: None,
            })
            .await
            .expect("child job should enqueue");

        let targets = backend
            .resolve_message_targets(child_thread_id, "parent")
            .await
            .expect("parent target should resolve");

        assert_eq!(targets, vec![parent_thread_id]);
    }

    #[tokio::test]
    async fn scheduler_backend_check_inbox_and_mark_read_use_message_ids() {
        let (manager, backend) =
            test_session_manager_with_provider_delay(Duration::from_millis(150)).await;
        let session_id = manager
            .create("mailbox check".to_string())
            .await
            .expect("session should create");
        let thread_id = manager
            .create_thread(session_id, AgentId::new(7), None, None)
            .await
            .expect("thread should create");

        manager
            .thread_pool
            .register_chat_thread(session_id, thread_id);
        let thread = manager
            .thread_pool
            .ensure_chat_runtime(session_id, thread_id)
            .await
            .expect("chat runtime should load");
        manager
            .thread_pool
            .send_chat_message(session_id, thread_id, "long running request".to_string())
            .await
            .expect("chat message should start a turn");
        timeout(Duration::from_secs(1), async {
            loop {
                if thread.read().await.is_turn_running() {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("chat thread should become active");

        let mailbox_message = MailboxMessage {
            id: "msg-session-mailbox".to_string(),
            from_thread_id: ThreadId::new(),
            to_thread_id: thread_id,
            from_label: "Planner".to_string(),
            message_type: MailboxMessageType::Plain,
            text: "queued follow-up".to_string(),
            timestamp: "2026-04-01T00:00:00Z".to_string(),
            read: false,
            summary: Some("queued".to_string()),
        };
        manager
            .thread_pool
            .deliver_mailbox_message(thread_id, mailbox_message.clone())
            .await
            .expect("mailbox message should queue");

        let unread = timeout(Duration::from_secs(1), async {
            loop {
                let unread = backend
                    .check_inbox(CheckInboxRequest { thread_id })
                    .await
                    .expect("check_inbox should succeed");
                if !unread.is_empty() {
                    break unread;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("mailbox message should become visible in inbox");
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].id, mailbox_message.id);

        backend
            .mark_read(MarkReadRequest {
                thread_id,
                message_id: mailbox_message.id.clone(),
            })
            .await
            .expect("mark_read should succeed");

        let unread = backend
            .check_inbox(CheckInboxRequest { thread_id })
            .await
            .expect("check_inbox should succeed after mark_read");
        assert!(unread.is_empty());
    }

    #[tokio::test]
    async fn scheduler_backend_rejects_thread_route_outside_direct_topology() {
        let (manager, backend) =
            test_session_manager_with_provider_delay(Duration::from_millis(5)).await;
        let session_a = manager
            .create("mailbox-topology-a".to_string())
            .await
            .expect("first session should create");
        let thread_a = manager
            .create_thread(session_a, AgentId::new(7), None, None)
            .await
            .expect("first thread should create");
        let session_b = manager
            .create("mailbox-topology-b".to_string())
            .await
            .expect("second session should create");
        let thread_b = manager
            .create_thread(session_b, AgentId::new(7), None, None)
            .await
            .expect("second thread should create");

        let error = backend
            .send_message(SendMessageRequest {
                thread_id: thread_a,
                to: format!("thread:{thread_b}"),
                message: "hello from elsewhere".to_string(),
                summary: None,
            })
            .await
            .expect_err("unrelated thread route should be rejected");

        assert!(
            error
                .to_string()
                .contains("not reachable from the current thread"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn scheduler_backend_rejects_job_route_until_child_runtime_is_loaded() {
        let (manager, backend) =
            test_session_manager_with_provider_delay(Duration::from_millis(150)).await;
        let session_id = manager
            .create("mailbox-job-route".to_string())
            .await
            .expect("session should create");
        let parent_thread_id = manager
            .create_thread(session_id, AgentId::new(7), None, None)
            .await
            .expect("parent thread should create");
        let job_id = "job-mailbox-unloaded-child".to_string();

        backend
            .job_manager
            .record_dispatched_job(parent_thread_id, job_id.clone());
        manager
            .thread_pool
            .enqueue_job(argus_job::types::ThreadPoolJobRequest {
                originating_thread_id: parent_thread_id,
                job_id: job_id.clone(),
                agent_id: AgentId::new(7),
                prompt: "not started yet".to_string(),
                context: None,
            })
            .await
            .expect("child job should enqueue");

        let error = backend
            .send_message(SendMessageRequest {
                thread_id: parent_thread_id,
                to: format!("job:{job_id}"),
                message: "hello queued child".to_string(),
                summary: None,
            })
            .await
            .expect_err("queued child should not accept mailbox traffic yet");

        assert!(
            error
                .to_string()
                .contains("not ready to receive mailbox messages"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn load_restores_persisted_threads_into_session_runtime_map() {
        let manager = test_session_manager().await;
        let session_id = manager
            .create("restorable session".to_string())
            .await
            .expect("session should create");
        let thread_id = manager
            .create_thread(session_id, AgentId::new(7), None, None)
            .await
            .expect("thread should create");

        manager
            .unload(session_id)
            .await
            .expect("session should unload");
        let session = manager.load(session_id).await.expect("session should load");

        assert!(
            session.get_thread(&thread_id).is_some(),
            "loaded session should expose persisted live threads"
        );
    }

    #[tokio::test]
    async fn session_manager_registers_unified_scheduler_tool() {
        let (_manager, tool_manager) = test_session_manager_with_tool_manager().await;
        let tool_ids = tool_manager.list_ids();

        assert!(tool_ids.iter().any(|id| id == "scheduler"));
        assert!(!tool_ids.iter().any(|id| id == "dispatch_job"));
        assert!(!tool_ids.iter().any(|id| id == "dispath_job"));
        assert!(!tool_ids.iter().any(|id| id == "get_job_result"));
        assert!(!tool_ids.iter().any(|id| id == "list_subagents"));
    }
}
