//! Thread implementation.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use derive_builder::Builder;
use tokio::sync::{Mutex, broadcast, mpsc};

use crate::command::ThreadRuntimeSnapshot;
use crate::turn::{TurnCancellation, TurnSharedContext};
use crate::{TurnBuilder, TurnOutput};
use argus_protocol::llm::{ChatMessage, LlmProvider, Role};
use argus_protocol::tool::NamedTool;
use argus_protocol::{
    AgentRecord, HookHandler, HookRegistry, MessageOverride, SessionId, ThreadControlEvent,
    ThreadEvent, ThreadId, ThreadMailbox, ThreadRuntimeState, TokenUsage,
};
use argus_tool::ToolManager;

use super::compact::Compactor;
use super::config::ThreadConfig;
use super::error::ThreadError;
use super::plan_hook::PlanContinuationHook;
use super::plan_store::FilePlanStore;
use super::plan_tool::UpdatePlanTool;
use super::turn_log_store::TurnLogPersistenceSnapshot;
use super::types::{ThreadInfo, ThreadState};
use crate::history::{
    CompactionCheckpoint, InFlightTurn, InFlightTurnPhase, InFlightTurnShared, TurnRecord,
    TurnState, shared_history,
};
mod history;
mod reactor;
mod runtime;

pub(crate) use reactor::{ThreadReactor, ThreadReactorAction};
/// Default broadcast channel capacity.
const DEFAULT_CHANNEL_CAPACITY: usize = 256;

/// Thread - multi-turn conversation session.
///
/// A Thread manages message history and executes Turns sequentially.
/// It broadcasts events to subscribers for real-time updates.
#[derive(Builder)]
#[builder(pattern = "owned", build_fn(skip))]
pub struct Thread {
    /// Unique identifier (strongly typed).
    id: ThreadId,

    /// Agent record with configuration.
    agent_record: Arc<AgentRecord>,

    /// Parent session ID.
    session_id: SessionId,

    /// Optional thread title.
    #[builder(default)]
    title: Option<String>,

    /// Creation timestamp.
    #[builder(default = "Utc::now()")]
    created_at: DateTime<Utc>,

    /// Last update timestamp.
    #[builder(default = "Utc::now()")]
    updated_at: DateTime<Utc>,

    /// Initial message history (for restoring sessions).
    #[builder(setter(custom), default = "Arc::new(Vec::new())")]
    messages: Arc<Vec<ChatMessage>>,

    /// System messages that prefix the committed history view.
    #[builder(default)]
    system_messages: Vec<ChatMessage>,

    /// Settled turn history. This becomes authoritative as migration proceeds.
    #[builder(default)]
    turns: Vec<TurnRecord>,

    /// The single in-flight turn, if any.
    #[builder(default)]
    current_turn: Option<InFlightTurn>,

    /// Compatibility cache for committed message history.
    #[builder(default)]
    cached_committed_messages: Option<Arc<Vec<ChatMessage>>>,

    /// Compaction checkpoint metadata for future context construction.
    #[builder(default)]
    compaction_checkpoint: Option<CompactionCheckpoint>,

    /// LLM provider (required, injected by Session).
    provider: Arc<dyn LlmProvider>,

    /// Tool manager retained as thread-owned configuration.
    #[builder(default = "Arc::new(ToolManager::new())")]
    tool_manager: Arc<ToolManager>,

    /// Compactor for managing context size.
    compactor: Arc<dyn Compactor>,

    /// Hook registry for lifecycle events (optional).
    #[builder(default, setter(strip_option))]
    hooks: Option<Arc<HookRegistry>>,

    /// Thread configuration.
    #[builder(default)]
    config: ThreadConfig,

    /// Token count (internal).
    #[builder(default)]
    token_count: u32,

    /// Turn count (internal).
    #[builder(default)]
    turn_count: u32,

    /// Next turn number to allocate.
    #[builder(default = "1")]
    next_turn_number: u32,

    /// Observable runtime snapshot owned by the thread reactor.
    #[builder(default)]
    runtime_snapshot: ThreadRuntimeSnapshot,

    /// Cancellation handle for the active turn, if any.
    #[builder(default)]
    active_turn_cancellation: Option<TurnCancellation>,

    /// Pipe for sending/receiving ThreadEvents.
    #[builder(default)]
    pipe_tx: broadcast::Sender<ThreadEvent>,

    /// Internal control-plane sender for low-volume orchestration messages.
    #[builder(default)]
    control_tx: mpsc::UnboundedSender<ThreadControlEvent>,

    /// Single-consumer control receiver, taken by the session orchestrator.
    #[builder(default)]
    control_rx: Option<mpsc::UnboundedReceiver<ThreadControlEvent>>,

    /// Thread-level mailbox shared between the orchestrator and active turns.
    #[builder(default = "Arc::new(Mutex::new(ThreadMailbox::default()))")]
    mailbox: Arc<Mutex<ThreadMailbox>>,

    /// File-backed plan store with persistence.
    #[builder(default, setter(name = "plan_store"))]
    plan_store: FilePlanStore,
}

impl std::fmt::Debug for Thread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Thread")
            .field("id", &self.id)
            .field("session_id", &self.session_id)
            .field("agent_id", &self.agent_record.id)
            .field("title", &self.title)
            .field("messages", &self.messages.len())
            .field(
                "cached_committed_messages",
                &self
                    .cached_committed_messages
                    .as_ref()
                    .map_or(0, |messages| messages.len()),
            )
            .field("turns", &self.turns.len())
            .field("token_count", &self.token_count)
            .field("turn_count", &self.turn_count)
            .field("runtime_state", &self.runtime_snapshot.state)
            .field("runtime_queue_depth", &self.runtime_snapshot.queue_depth)
            .field("plan_items", &self.plan_store.store().read().unwrap().len())
            .field("config", &self.config)
            .finish()
    }
}

impl ThreadBuilder {
    /// Create a new ThreadBuilder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the initial message history.
    pub fn messages(mut self, value: Vec<ChatMessage>) -> Self {
        self.messages = Some(Arc::new(value));
        self
    }

    /// Share an existing message history buffer.
    pub fn shared_messages(mut self, value: Arc<Vec<ChatMessage>>) -> Self {
        self.messages = Some(value);
        self
    }

    /// Build the Thread.
    ///
    /// # Errors
    ///
    /// Returns `ThreadError` if required fields (`provider`, `compactor`, `agent_record`, `session_id`) are not set.
    pub fn build(self) -> Result<Thread, ThreadError> {
        let (pipe_tx, _) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);
        let (control_tx, control_rx) = mpsc::unbounded_channel();

        let agent_record = self.agent_record.ok_or(ThreadError::AgentRecordNotSet)?;
        let session_id = self.session_id.ok_or(ThreadError::SessionIdNotSet)?;
        let tool_manager = self
            .tool_manager
            .unwrap_or_else(|| Arc::new(ToolManager::new()));
        let hooks = self.hooks.flatten();
        let plan_store = self.plan_store.unwrap_or_default();
        let turns = self.turns.unwrap_or_default();
        let current_turn = self.current_turn.flatten();

        // Initialize messages with system prompt if not empty and no existing system message
        let mut messages = self.messages.unwrap_or_else(|| Arc::new(Vec::new()));
        let has_system_message = messages
            .first()
            .is_some_and(|m| m.role == argus_protocol::llm::Role::System);
        if !has_system_message && !agent_record.system_prompt.is_empty() {
            Arc::make_mut(&mut messages)
                .insert(0, ChatMessage::system(&agent_record.system_prompt));
        }

        let system_messages = Thread::collect_system_messages(messages.as_slice());
        let cached_committed_messages = if turns.is_empty() {
            Some(Arc::clone(&messages))
        } else {
            Some(Thread::materialize_committed_messages(
                &system_messages,
                &turns,
            ))
        };
        let next_turn_number = self
            .next_turn_number
            .unwrap_or_else(|| Thread::derive_next_turn_number(&turns));
        if let Some(committed_messages) = cached_committed_messages.as_ref() {
            messages = Arc::clone(committed_messages);
        }
        Ok(Thread {
            id: self.id.unwrap_or_default(),
            agent_record,
            session_id,
            title: self.title.flatten(),
            created_at: self.created_at.unwrap_or_else(Utc::now),
            updated_at: self.updated_at.unwrap_or_else(Utc::now),
            messages,
            system_messages,
            turns,
            current_turn,
            cached_committed_messages,
            compaction_checkpoint: self.compaction_checkpoint.flatten(),
            provider: self.provider.ok_or(ThreadError::ProviderNotConfigured)?,
            tool_manager,
            compactor: self.compactor.ok_or(ThreadError::CompactorNotConfigured)?,
            hooks,
            config: self.config.unwrap_or_default(),
            token_count: 0,
            turn_count: 0,
            next_turn_number,
            runtime_snapshot: ThreadRuntimeSnapshot::default(),
            active_turn_cancellation: None,
            pipe_tx,
            control_tx,
            control_rx: Some(control_rx),
            mailbox: Arc::new(Mutex::new(ThreadMailbox::default())),
            plan_store,
        })
    }
}

impl Thread {
    /// Get the Thread ID.
    pub fn id(&self) -> ThreadId {
        self.id
    }

    /// Get the Session ID.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Get the Agent Record.
    #[allow(clippy::explicit_auto_deref)]
    pub fn agent_record(&self) -> &AgentRecord {
        &*self.agent_record
    }

    /// Get the thread title.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Set the thread title.
    pub fn set_title(&mut self, title: Option<String>) {
        self.title = title.filter(|value| !value.trim().is_empty());
        self.updated_at = Utc::now();
    }

    /// Get creation timestamp.
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Get last update timestamp.
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// Get information about this thread.
    pub fn info(&self) -> ThreadInfo {
        ThreadInfo {
            id: self.id.to_string(),
            message_count: self.history().len(),
            token_count: self.token_count,
            turn_count: self.turn_count(),
            plan_item_count: self.plan_store.store().read().unwrap().len(),
        }
    }

    /// Subscribe to Thread events.
    ///
    /// Multiple subscribers can receive events simultaneously.
    pub fn subscribe(&self) -> broadcast::Receiver<ThreadEvent> {
        self.pipe_tx.subscribe()
    }

    /// Broadcast a ThreadEvent to this thread's subscribers.
    pub fn broadcast_to_self(&self, event: ThreadEvent) {
        let _ = self.pipe_tx.send(event);
    }

    /// Get a reference to the broadcast sender (for creating receivers).
    pub fn pipe_tx(&self) -> &broadcast::Sender<ThreadEvent> {
        &self.pipe_tx
    }

    /// Clone the internal control sender for this thread.
    pub fn control_tx(&self) -> mpsc::UnboundedSender<ThreadControlEvent> {
        self.control_tx.clone()
    }

    /// Take the single control receiver owned by the session orchestrator.
    pub fn take_control_rx(&mut self) -> Option<mpsc::UnboundedReceiver<ThreadControlEvent>> {
        self.control_rx.take()
    }

    /// Clone the shared mailbox.
    pub fn mailbox(&self) -> Arc<Mutex<ThreadMailbox>> {
        Arc::clone(&self.mailbox)
    }

    /// Returns true if a Turn is currently executing.
    pub fn is_turn_running(&self) -> bool {
        !matches!(self.runtime_snapshot.state, ThreadRuntimeState::Idle)
    }

    /// Get current state.
    pub fn state(&self) -> ThreadState {
        match self.runtime_snapshot.state {
            ThreadRuntimeState::Idle => ThreadState::Idle,
            ThreadRuntimeState::Running { .. }
            | ThreadRuntimeState::Stopping { .. }
            | ThreadRuntimeState::WaitingForApproval { .. } => ThreadState::Processing,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn runtime_snapshot(&self) -> ThreadRuntimeSnapshot {
        self.runtime_snapshot
    }

    pub(crate) fn sync_runtime_snapshot(&mut self, snapshot: ThreadRuntimeSnapshot) {
        self.runtime_snapshot = snapshot;
    }

    #[allow(dead_code)]
    pub(crate) fn active_turn_cancellation(&self) -> Option<TurnCancellation> {
        self.active_turn_cancellation.clone()
    }

    pub(crate) fn set_active_turn_cancellation(&mut self, cancellation: Option<TurnCancellation>) {
        self.active_turn_cancellation = cancellation;
    }

    /// Get message history (read-only).
    pub fn history(&self) -> &[ChatMessage] {
        shared_history(&self.messages, self.cached_committed_messages.as_ref()).as_slice()
    }

    /// Returns true when committed history contains visible transcript beyond system prompts.
    pub fn has_non_system_history(&self) -> bool {
        self.history()
            .iter()
            .any(|message| message.role != Role::System)
    }

    /// Get current token count.
    pub fn token_count(&self) -> u32 {
        self.token_count
    }

    /// Get turn count.
    pub fn turn_count(&self) -> u32 {
        self.turn_count.max(Self::latest_turn_number(&self.turns))
    }

    pub(crate) fn next_turn_number_for_runtime(&self) -> u32 {
        self.next_turn_number
    }

    /// Get a read-only snapshot of the current plan state.
    pub fn plan(&self) -> Vec<serde_json::Value> {
        self.plan_store.store().read().unwrap().clone()
    }

    /// Get the LLM provider.
    pub fn provider(&self) -> &Arc<dyn LlmProvider> {
        &self.provider
    }

    /// Replace the bound LLM provider for subsequent turns.
    pub fn set_provider(&mut self, provider: Arc<dyn LlmProvider>) {
        let model_name = provider.model_name().to_string();
        self.provider = provider;
        if let Some(trace_config) = self.config.turn_config.trace_config.as_mut() {
            trace_config.model = Some(model_name);
        }
        self.updated_at = Utc::now();
    }

    /// Get mutable access to messages (for Compactor).
    pub fn messages_mut(&mut self) -> &mut Vec<ChatMessage> {
        self.cached_committed_messages = None;
        Arc::make_mut(&mut self.messages)
    }

    /// Set the token count (for Compactor).
    pub fn set_token_count(&mut self, count: u32) {
        self.token_count = count;
    }

    fn start_current_turn(
        &mut self,
        turn_number: u32,
        user_input: String,
        shared: Arc<InFlightTurnShared>,
    ) {
        self.current_turn = Some(InFlightTurn {
            turn_number,
            state: InFlightTurnPhase::CallingLlm,
            pending_messages: vec![ChatMessage::user(user_input)],
            token_usage: TokenUsage::default(),
            started_at: Utc::now(),
            model: Some(self.provider.model_name().to_string()),
            shared,
        });
    }

    fn settle_current_turn(
        &mut self,
        state: TurnState,
        committed_messages: Option<Vec<ChatMessage>>,
        token_usage: Option<TokenUsage>,
        error: Option<String>,
    ) -> Result<(), ThreadError> {
        let current_turn = self.current_turn.take().ok_or_else(|| {
            ThreadError::TurnBuildFailed("missing in-flight turn during settle".to_string())
        })?;
        let mut committed_messages =
            committed_messages.unwrap_or_else(|| current_turn.pending_messages.clone());
        if !current_turn.pending_messages.is_empty()
            && !Self::starts_with_pending_messages(
                committed_messages.as_slice(),
                current_turn.pending_messages.as_slice(),
            )
        {
            let mut normalized_messages = current_turn.pending_messages.clone();
            normalized_messages.extend(committed_messages);
            committed_messages = normalized_messages;
        }

        self.turns.push(TurnRecord {
            turn_number: current_turn.turn_number,
            state,
            messages: committed_messages.clone(),
            token_usage,
            started_at: current_turn.started_at,
            finished_at: Some(Utc::now()),
            model: current_turn.model,
            error,
        });
        Arc::make_mut(&mut self.messages).extend(committed_messages);
        self.sync_cached_history_from_flat_messages();
        self.updated_at = Utc::now();

        Ok(())
    }

    pub(crate) fn turn_log_persistence_snapshot(&self) -> Option<TurnLogPersistenceSnapshot> {
        let trace_config = self
            .config
            .turn_config
            .trace_config
            .as_ref()
            .filter(|config| config.enabled)?;
        let turn = self.turns.last()?.clone();
        let mut base_dir = trace_config.trace_dir.clone();
        let session_id = trace_config.session_id.unwrap_or(self.session_id);
        base_dir = base_dir.join(session_id.to_string());
        base_dir = base_dir.join(self.id.to_string());

        Some(TurnLogPersistenceSnapshot {
            base_dir,
            turn,
            system_messages: self.system_messages.clone(),
            checkpoint: self.compaction_checkpoint.clone(),
        })
    }

    fn starts_with_pending_messages(
        messages: &[ChatMessage],
        pending_messages: &[ChatMessage],
    ) -> bool {
        pending_messages.len() <= messages.len()
            && messages
                .iter()
                .zip(pending_messages.iter())
                .all(|(message, pending_message)| {
                    message.role == pending_message.role
                        && message.content == pending_message.content
                })
    }
    /// Send a user message into the pipe for processing.
    ///
    /// This is the entry point for external callers (CLI, Tauri).
    /// The message is written to the pipe; Thread.run() picks it up.
    pub fn send_user_message(
        &self,
        content: String,
        msg_override: Option<MessageOverride>,
    ) -> Result<(), ThreadError> {
        let event = ThreadControlEvent::UserMessage {
            content,
            msg_override,
        };
        if self.control_tx.send(event).is_err() {
            tracing::warn!("control send failed in send_user_message");
        }
        Ok(())
    }

    /// Send a low-volume control event into this thread.
    pub fn send_control_event(&self, event: ThreadControlEvent) -> Result<(), ThreadError> {
        if self.control_tx.send(event).is_err() {
            tracing::warn!("control send failed in send_control_event");
        }
        Ok(())
    }

    /// Begin building a turn without holding the caller's lock for the whole execution.
    pub async fn begin_turn(
        &mut self,
        user_input: String,
        msg_override: Option<MessageOverride>,
        cancellation: TurnCancellation,
    ) -> Result<crate::Turn, ThreadError> {
        let turn_number = self.next_turn_number;
        self.next_turn_number = self.next_turn_number.saturating_add(1);
        self.begin_turn_with_number(turn_number, user_input, msg_override, cancellation)
            .await
    }

    async fn begin_turn_with_number(
        &mut self,
        turn_number: u32,
        user_input: String,
        msg_override: Option<MessageOverride>,
        cancellation: TurnCancellation,
    ) -> Result<crate::Turn, ThreadError> {
        let turn_context = self.build_turn_context();
        match self
            .compactor
            .compact(
                self.provider.as_ref(),
                turn_context.as_slice(),
                self.token_count,
            )
            .await
        {
            Ok(Some(result)) => {
                self.compaction_checkpoint = Some(CompactionCheckpoint {
                    summarized_through_turn: self.turn_count(),
                    summary_messages: result.summary_messages,
                    created_at: Utc::now(),
                });
                self.token_count = result.token_count;
                self.broadcast_to_self(ThreadEvent::Compacted {
                    thread_id: self.id.to_string(),
                    new_token_count: self.token_count,
                });
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("Compact failed: {}", e);
            }
        }

        // Apply message-level override in-place if provided.
        // Arc::make_mut clones the inner record only if this Arc is shared (multiple owners).
        // If no override is provided, just clone the Arc reference (O(1)).
        let effective_record = if let Some(overrides) = msg_override {
            let record = Arc::make_mut(&mut self.agent_record);
            if let Some(v) = overrides.max_tokens {
                record.max_tokens = Some(v);
            }
            if let Some(v) = overrides.temperature {
                record.temperature = Some(v);
            }
            if let Some(v) = overrides.thinking_config {
                record.thinking_config = Some(v);
            }
            self.agent_record.clone()
        } else {
            self.agent_record.clone()
        };

        let shared = Arc::new(InFlightTurnShared {
            history: self.build_turn_context(),
            tools: Arc::new(self.build_shared_turn_tools(effective_record.as_ref())),
            hooks: Arc::new(self.build_shared_turn_hooks()),
        });
        self.sync_turn_counters(turn_number);
        self.start_current_turn(turn_number, user_input, Arc::clone(&shared));
        self.set_active_turn_cancellation(Some(cancellation.clone()));

        match self.build_turn(turn_number, effective_record, cancellation, shared) {
            Ok(turn) => Ok(turn),
            Err(error) => {
                self.current_turn = None;
                self.set_active_turn_cancellation(None);
                Err(error)
            }
        }
    }

    /// Finish a previously started turn and apply its output to thread state.
    pub fn finish_turn(
        &mut self,
        result: Result<TurnOutput, ThreadError>,
    ) -> Result<(), ThreadError> {
        self.set_active_turn_cancellation(None);

        match result {
            Ok(output) => {
                self.token_count = output.token_usage.total_tokens;
                self.settle_current_turn(
                    TurnState::Completed,
                    Some(output.appended_messages),
                    Some(output.token_usage),
                    None,
                )?;
                Ok(())
            }
            Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)) => {
                self.settle_current_turn(TurnState::Cancelled, None, None, None)?;
                Ok(())
            }
            Err(error) => {
                let error_message = error.to_string();
                self.settle_current_turn(TurnState::Failed, None, None, Some(error_message))?;
                Err(error)
            }
        }
    }

    fn build_turn(
        &mut self,
        turn_number: u32,
        agent_record: Arc<AgentRecord>,
        cancellation: TurnCancellation,
        shared_state: Arc<InFlightTurnShared>,
    ) -> Result<crate::Turn, ThreadError> {
        let thread_id = self.id.to_string();
        let pending_messages = self
            .current_turn
            .as_ref()
            .map_or_else(Vec::new, |turn| turn.pending_messages.clone());
        let shared = Arc::new(TurnSharedContext::for_thread(shared_state));
        // Create internal stream channel
        let (stream_tx, _stream_rx) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);

        // Build Turn using TurnBuilder
        let turn_builder = TurnBuilder::default()
            .turn_number(turn_number)
            .thread_id(thread_id.clone())
            .originating_thread_id(self.id)
            .session_id(self.session_id)
            .shared(shared)
            .pending_messages(pending_messages)
            .provider(self.provider.clone())
            .config(self.config.turn_config.clone())
            .agent_record(agent_record)
            .stream_tx(stream_tx)
            .thread_event_tx(self.pipe_tx.clone())
            .control_tx(self.control_tx.clone())
            .mailbox(Arc::clone(&self.mailbox));

        turn_builder
            .cancellation(cancellation)
            .build()
            .map_err(|e| ThreadError::TurnBuildFailed(e.to_string()))
    }

    fn build_shared_turn_tools(&self, agent_record: &AgentRecord) -> Vec<Arc<dyn NamedTool>> {
        let enabled_tool_names = agent_record
            .tool_names
            .iter()
            .collect::<std::collections::HashSet<_>>();
        let mut tools = self
            .tool_manager
            .list_ids()
            .iter()
            .filter(|name| enabled_tool_names.contains(name))
            .filter_map(|name| self.tool_manager.get(name))
            .collect::<Vec<_>>();
        let plan_tool: Arc<dyn NamedTool> =
            Arc::new(UpdatePlanTool::new(Arc::new(self.plan_store.clone())));
        tools.push(plan_tool);
        tools
    }

    fn build_shared_turn_hooks(&self) -> Vec<Arc<dyn HookHandler>> {
        let mut hooks = self
            .hooks
            .as_ref()
            .map_or_else(Vec::new, |registry| registry.all_handlers());
        let plan_hook: Arc<dyn HookHandler> =
            Arc::new(PlanContinuationHook::new(Arc::new(self.plan_store.clone())));
        hooks.push(plan_hook);
        hooks
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
