//! Thread implementation.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use derive_builder::Builder;
use tokio::sync::{Mutex, broadcast, mpsc};

use crate::turn::TurnCancellation;
use crate::{TurnBuilder, TurnOutput};
use argus_protocol::llm::{ChatMessage, LlmProvider};
use argus_protocol::tool::NamedTool;
use argus_protocol::{
    AgentRecord, HookHandler, HookRegistry, MessageOverride, SessionId, ThreadControlEvent,
    ThreadEvent, ThreadId, ThreadMailbox, TokenUsage,
};
use argus_tool::ToolManager;

use super::compact::Compactor;
use super::config::ThreadConfig;
use super::error::ThreadError;
use super::history::{
    flatten_turn_messages, CompactionCheckpoint, InFlightTurn, InFlightTurnPhase, TurnRecord,
    TurnState,
};
use super::plan_hook::PlanContinuationHook;
use super::plan_store::FilePlanStore;
use super::plan_tool::UpdatePlanTool;
use super::turn_log_store::TurnLogPersistenceSnapshot;
use super::types::{ThreadInfo, ThreadState};
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

    /// Tool manager.
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

    /// Whether a Turn is currently running.
    #[builder(default)]
    turn_running: bool,

    /// File-backed plan store with persistence.
    #[builder(default, setter(name = "plan_store"))]
    plan_store: FilePlanStore,

    /// Synthetic history messages that should be traced once with the next visible turn.
    #[builder(default)]
    pending_trace_prelude_messages: Vec<ChatMessage>,
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
                &self.cached_committed_messages.as_ref().map_or(0, |messages| messages.len()),
            )
            .field("turns", &self.turns.len())
            .field("token_count", &self.token_count)
            .field("turn_count", &self.turn_count)
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
        let turns = self.turns.unwrap_or_default();
        let current_turn = self.current_turn.flatten();

        // Initialize messages with system prompt if not empty and no existing system message
        let mut messages = self.messages.unwrap_or_else(|| Arc::new(Vec::new()));
        let has_system_message = messages
            .first()
            .is_some_and(|m| m.role == argus_protocol::llm::Role::System);
        if !has_system_message && !agent_record.system_prompt.is_empty() {
            Arc::make_mut(&mut messages).insert(0, ChatMessage::system(&agent_record.system_prompt));
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
            tool_manager: self
                .tool_manager
                .unwrap_or_else(|| Arc::new(ToolManager::new())),
            compactor: self.compactor.ok_or(ThreadError::CompactorNotConfigured)?,
            hooks: self.hooks.flatten(),
            config: self.config.unwrap_or_default(),
            token_count: 0,
            turn_count: 0,
            next_turn_number,
            pipe_tx,
            control_tx,
            control_rx: Some(control_rx),
            mailbox: Arc::new(Mutex::new(ThreadMailbox::default())),
            turn_running: false,
            plan_store: self.plan_store.unwrap_or_default(),
            pending_trace_prelude_messages: self.pending_trace_prelude_messages.unwrap_or_default(),
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
        self.turn_running
    }

    /// Mark that a turn has started or stopped.
    fn set_turn_running(&mut self, running: bool) {
        self.turn_running = running;
    }

    /// Get current state.
    pub fn state(&self) -> ThreadState {
        ThreadState::Idle
    }

    /// Get message history (read-only).
    pub fn history(&self) -> &[ChatMessage] {
        self.cached_committed_messages
            .as_ref()
            .map_or_else(|| self.messages.as_slice(), |messages| messages.as_slice())
    }

    fn build_turn_context(&self) -> Arc<Vec<ChatMessage>> {
        if let Some(checkpoint) = self.compaction_checkpoint.as_ref() {
            let mut context_messages = self.system_messages.clone();
            context_messages.extend(checkpoint.summary_messages.iter().cloned());
            context_messages.extend(
                self.turns
                    .iter()
                    .filter(|turn| turn.turn_number > checkpoint.summarized_through_turn)
                    .flat_map(|turn| turn.messages.iter().cloned()),
            );
            Arc::new(context_messages)
        } else {
            Arc::clone(&self.messages)
        }
    }

    /// Get current token count.
    pub fn token_count(&self) -> u32 {
        self.token_count
    }

    /// Get turn count.
    pub fn turn_count(&self) -> u32 {
        self.turn_count.max(self.turns.len() as u32)
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

    /// Hydrate thread runtime state from persisted history.
    pub fn hydrate_from_persisted_state(
        &mut self,
        mut messages: Vec<ChatMessage>,
        token_count: u32,
        turn_count: u32,
        updated_at: DateTime<Utc>,
    ) {
        let existing_system = self
            .messages
            .first()
            .filter(|message| message.role == argus_protocol::llm::Role::System)
            .cloned();
        let has_system_message = messages
            .first()
            .is_some_and(|message| message.role == argus_protocol::llm::Role::System);

        if !has_system_message && let Some(system_message) = existing_system {
            messages.insert(0, system_message);
        }

        self.messages = Arc::new(messages);
        self.system_messages = Self::collect_system_messages(self.messages.as_slice());
        self.turns.clear();
        self.current_turn = None;
        self.compaction_checkpoint = None;
        self.cached_committed_messages = Some(Arc::clone(&self.messages));
        self.token_count = token_count;
        self.turn_count = turn_count;
        self.next_turn_number = turn_count.saturating_add(1);
        self.updated_at = updated_at;
    }

    fn start_current_turn(&mut self, turn_number: u32, user_input: String) {
        self.current_turn = Some(InFlightTurn {
            turn_number,
            state: InFlightTurnPhase::CallingLlm,
            pending_messages: vec![ChatMessage::user(user_input)],
            token_usage: TokenUsage::default(),
            started_at: Utc::now(),
            model: Some(self.provider.model_name().to_string()),
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
                    message.role == pending_message.role && message.content == pending_message.content
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

    /// Spawn the thread runtime actor that coordinates queued control events.
    pub fn spawn_runtime_actor(thread: Arc<tokio::sync::RwLock<Self>>) {
        crate::runtime::spawn_runtime_actor(thread);
    }

    /// Begin building a turn without holding the caller's lock for the whole execution.
    pub async fn begin_turn(
        &mut self,
        user_input: String,
        msg_override: Option<MessageOverride>,
        cancellation: TurnCancellation,
    ) -> Result<crate::Turn, ThreadError> {
        self.set_turn_running(true);

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
                self.pending_trace_prelude_messages = result.trace_prelude_messages;
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

        let turn_number = self.next_turn_number;
        self.next_turn_number = self.next_turn_number.saturating_add(1);
        self.turn_count = turn_number;
        self.start_current_turn(turn_number, user_input);

        match self.build_turn(turn_number, effective_record, cancellation) {
            Ok(turn) => Ok(turn),
            Err(error) => {
                self.current_turn = None;
                self.set_turn_running(false);
                Err(error)
            }
        }
    }

    /// Finish a previously started turn and apply its output to thread state.
    pub fn finish_turn(
        &mut self,
        result: Result<TurnOutput, ThreadError>,
    ) -> Result<(), ThreadError> {
        self.set_turn_running(false);

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
    ) -> Result<crate::Turn, ThreadError> {
        let thread_id = self.id.to_string();
        let pending_messages = self
            .current_turn
            .as_ref()
            .map_or_else(Vec::new, |turn| turn.pending_messages.clone());
        let shared_messages = self.build_turn_context();

        // Thread is responsible for building: collect tools and hooks
        // Filter tools by agent_record.tool_names; empty means no tools
        let enabled_tool_names = agent_record
            .tool_names
            .iter()
            .collect::<std::collections::HashSet<_>>();
        let mut tools: Vec<Arc<dyn NamedTool>> = self
            .tool_manager
            .list_ids()
            .iter()
            .filter(|name| enabled_tool_names.contains(name))
            .filter_map(|name| self.tool_manager.get(name))
            .collect();

        // Append UpdatePlanTool with the thread's plan store
        tools.push(Arc::new(UpdatePlanTool::new(Arc::new(
            self.plan_store.clone(),
        ))));

        let mut hooks: Vec<Arc<dyn HookHandler>> = self
            .hooks
            .as_ref()
            .map(|registry| registry.all_handlers())
            .unwrap_or_default();
        hooks.push(Arc::new(PlanContinuationHook::new(Arc::new(
            self.plan_store.clone(),
        ))));
        let trace_prelude_messages = std::mem::take(&mut self.pending_trace_prelude_messages);

        // Create internal stream channel
        let (stream_tx, _stream_rx) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);

        // Build Turn using TurnBuilder
        let mut turn_builder = TurnBuilder::default()
            .turn_number(turn_number)
            .thread_id(thread_id.clone())
            .originating_thread_id(self.id)
            .session_id(self.session_id)
            .shared_messages(shared_messages)
            .pending_messages(pending_messages)
            .provider(self.provider.clone())
            .tools(tools)
            .hooks(hooks)
            .config(self.config.turn_config.clone())
            .agent_record(agent_record)
            .stream_tx(stream_tx)
            .thread_event_tx(self.pipe_tx.clone())
            .control_tx(self.control_tx.clone())
            .mailbox(Arc::clone(&self.mailbox))
            .trace_prelude_messages(trace_prelude_messages);

        if let Some(ref tc) = self.config.turn_config.trace_config {
            turn_builder = turn_builder.trace_config(tc.clone());
        }

        turn_builder
            .cancellation(cancellation)
            .build()
            .map_err(|e| ThreadError::TurnBuildFailed(e.to_string()))
    }

    fn collect_system_messages(messages: &[ChatMessage]) -> Vec<ChatMessage> {
        messages
            .iter()
            .take_while(|message| message.role == argus_protocol::llm::Role::System)
            .cloned()
            .collect()
    }

    fn sync_cached_history_from_flat_messages(&mut self) {
        self.system_messages = Self::collect_system_messages(self.messages.as_slice());
        self.cached_committed_messages = Some(Arc::clone(&self.messages));
    }

    fn materialize_committed_messages(
        system_messages: &[ChatMessage],
        turns: &[TurnRecord],
    ) -> Arc<Vec<ChatMessage>> {
        let mut messages = system_messages.to_vec();
        messages.extend(flatten_turn_messages(turns));
        Arc::new(messages)
    }

    fn derive_next_turn_number(turns: &[TurnRecord]) -> u32 {
        turns
            .iter()
            .map(|turn| turn.turn_number)
            .max()
            .map_or(1, |turn_number| turn_number.saturating_add(1))
    }

    #[cfg(test)]
    fn hydrate_turn_history_for_test(&mut self, turns: Vec<TurnRecord>) {
        self.turns = turns;
        self.current_turn = None;
        self.turn_count = self.turns.len() as u32;
        self.next_turn_number = Self::derive_next_turn_number(&self.turns);
        let committed_messages =
            Self::materialize_committed_messages(&self.system_messages, &self.turns);
        self.cached_committed_messages = Some(Arc::clone(&committed_messages));
        self.messages = committed_messages;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;
    use crate::compact::CompactResult;
    use crate::config::{ThreadConfig, TurnConfigBuilder};
    use crate::error::CompactError;
    use crate::history::TurnRecord;
    use crate::runtime::ThreadRuntimeAction;
    use crate::thread_handle::ThreadHandle;
    use crate::trace::TraceConfig;
    use crate::turn_log_store::{
        persist_turn_log_snapshot, read_turn_messages, read_turn_meta, turn_messages_path,
        turn_meta_path, turns_dir,
    };
    use argus_protocol::llm::{CompletionRequest, CompletionResponse, LlmError};
    use argus_protocol::{AgentId, AgentType, ProviderId, ThreadCommand, ThreadRuntimeState};
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use serde_json::json;
    use tokio::time::{sleep, timeout};

    struct DummyProvider;

    #[async_trait]
    impl LlmProvider for DummyProvider {
        fn model_name(&self) -> &str {
            "dummy"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            Err(LlmError::RequestFailed {
                provider: "dummy".to_string(),
                reason: "not implemented".to_string(),
            })
        }
    }

    struct SmallContextProvider {
        context_window: u32,
    }

    #[async_trait]
    impl LlmProvider for SmallContextProvider {
        fn model_name(&self) -> &str {
            "small-context"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            Err(LlmError::RequestFailed {
                provider: "small-context".to_string(),
                reason: "not implemented".to_string(),
            })
        }

        fn context_window(&self) -> u32 {
            self.context_window
        }
    }

    struct NoopCompactor;

    #[async_trait]
    impl Compactor for NoopCompactor {
        async fn compact(
            &self,
            _provider: &dyn LlmProvider,
            _messages: &[ChatMessage],
            _token_count: u32,
        ) -> Result<Option<CompactResult>, CompactError> {
            Ok(None)
        }

        fn name(&self) -> &'static str {
            "noop"
        }
    }

    struct FailingCompactor;

    #[async_trait]
    impl Compactor for FailingCompactor {
        async fn compact(
            &self,
            _provider: &dyn LlmProvider,
            _messages: &[ChatMessage],
            _token_count: u32,
        ) -> Result<Option<CompactResult>, CompactError> {
            Err(CompactError::Failed {
                reason: "boom".to_string(),
            })
        }

        fn name(&self) -> &'static str {
            "failing"
        }
    }

    fn test_agent_record() -> Arc<AgentRecord> {
        Arc::new(AgentRecord {
            id: AgentId::new(1),
            display_name: "Test Agent".to_string(),
            description: "A test agent".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(ProviderId::new(1)),
            model_id: None,
            system_prompt: "You are a test agent.".to_string(),
            tool_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: None,
            parent_agent_id: None,
            agent_type: AgentType::Standard,
        })
    }

    fn test_agent_record_without_system_prompt() -> Arc<AgentRecord> {
        Arc::new(AgentRecord {
            system_prompt: String::new(),
            ..(*test_agent_record()).clone()
        })
    }

    #[derive(Debug, Clone)]
    enum ResponsePlan {
        Ok(String),
    }

    #[derive(Debug)]
    struct SequencedProvider {
        delay: Duration,
        plans: Arc<Mutex<VecDeque<ResponsePlan>>>,
        captured_user_inputs: Arc<Mutex<Vec<String>>>,
    }

    impl SequencedProvider {
        fn new(delay: Duration, plans: Vec<ResponsePlan>) -> Self {
            Self {
                delay,
                plans: Arc::new(Mutex::new(VecDeque::from(plans))),
                captured_user_inputs: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn captured_user_inputs(&self) -> Vec<String> {
            self.captured_user_inputs.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LlmProvider for SequencedProvider {
        fn model_name(&self) -> &str {
            "sequenced"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
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

            let next_plan = self
                .plans
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| ResponsePlan::Ok("default response".to_string()));
            let ResponsePlan::Ok(content) = next_plan;
            Ok(CompletionResponse {
                content: Some(content),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 10,
                output_tokens: 5,
                finish_reason: argus_protocol::llm::FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }
    }

    fn build_test_thread() -> Thread {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(compactor)
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build")
    }

    fn build_test_thread_with_provider(
        provider: Arc<dyn LlmProvider>,
    ) -> Arc<tokio::sync::RwLock<Thread>> {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        Arc::new(tokio::sync::RwLock::new(
            ThreadBuilder::new()
                .provider(provider)
                .compactor(compactor)
                .agent_record(test_agent_record())
                .session_id(SessionId::new())
                .build()
                .expect("thread should build"),
        ))
    }

    fn repeated_test_message() -> String {
        ["test"; 10].join(" ")
    }

    fn token_count_for_messages(messages: &[ChatMessage]) -> u32 {
        // Simple heuristic: ~1 token per word, similar to the old estimate_tokens.
        messages
            .iter()
            .map(|message| message.content.split_whitespace().count() as u32)
            .sum()
    }

    async fn wait_for_idle_events(
        thread: &Arc<tokio::sync::RwLock<Thread>>,
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

    #[test]
    fn thread_builder_requires_provider() {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        let result = ThreadBuilder::new()
            .compactor(compactor)
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .build();
        assert!(matches!(result, Err(ThreadError::ProviderNotConfigured)));
    }

    #[test]
    fn thread_builder_requires_compactor() {
        let result = ThreadBuilder::new()
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn thread_builder_requires_agent_record() {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        let result = ThreadBuilder::new()
            .compactor(compactor)
            .session_id(SessionId::new())
            .build();
        assert!(matches!(result, Err(ThreadError::AgentRecordNotSet)));
    }

    #[test]
    fn thread_builder_requires_session_id() {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        let result = ThreadBuilder::new()
            .compactor(compactor)
            .agent_record(test_agent_record())
            .build();
        assert!(matches!(result, Err(ThreadError::SessionIdNotSet)));
    }

    #[test]
    fn hydrate_from_persisted_state_preserves_system_prompt_and_updates_runtime_state() {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        let updated_at = Utc::now();
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(compactor)
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .build()
            .unwrap();

        thread.hydrate_from_persisted_state(
            vec![
                ChatMessage::user("历史问题"),
                ChatMessage::assistant("历史回答"),
            ],
            42,
            3,
            updated_at,
        );

        assert_eq!(thread.history().len(), 3);
        assert_eq!(thread.history()[0].role, argus_protocol::llm::Role::System);
        assert_eq!(thread.history()[1].content, "历史问题");
        assert_eq!(thread.history()[2].content, "历史回答");
        assert_eq!(thread.token_count(), 42);
        assert_eq!(thread.turn_count(), 3);
        assert_eq!(thread.updated_at(), updated_at);
    }

    #[tokio::test]
    async fn history_reads_from_cached_committed_messages() {
        let mut thread = build_test_thread();
        thread.hydrate_turn_history_for_test(vec![TurnRecord::completed(
            1,
            vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
        )]);

        assert_eq!(thread.history().len(), 3);
        assert_eq!(thread.turn_count(), 1);
    }

    #[test]
    fn builder_keeps_committed_history_and_turn_count_aligned_when_seeded_with_turns() {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        let thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(compactor)
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .messages(vec![
                ChatMessage::system("You are a stale system message."),
                ChatMessage::user("stale"),
                ChatMessage::assistant("history"),
            ])
            .turns(vec![TurnRecord::completed(
                1,
                vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
            )])
            .build()
            .unwrap();

        assert_eq!(thread.history().len(), 3);
        assert_eq!(thread.history()[0].role, argus_protocol::llm::Role::System);
        assert_eq!(thread.history()[1].content, "hi");
        assert_eq!(thread.history()[2].content, "hello");
        assert_eq!(thread.turn_count(), 1);
    }

    #[test]
    fn builder_derives_next_turn_number_from_seeded_turns() {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        let thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(compactor)
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .turns(vec![TurnRecord::completed(
                4,
                vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
            )])
            .build()
            .unwrap();

        assert_eq!(thread.next_turn_number, 5);
    }

    #[test]
    fn plan_returns_read_only_snapshot() {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);

        // Create a temp dir and pre-populate plan.json at {temp_dir}/{thread_id}/plan.json
        let temp_dir = std::env::temp_dir()
            .join("argus-thread-test-plan")
            .join("thread-1");
        std::fs::create_dir_all(&temp_dir).unwrap();
        std::fs::write(
            temp_dir.join("plan.json"),
            serde_json::to_string_pretty(&vec![json!({
                "step": "Inspect review feedback",
                "status": "completed"
            })])
            .unwrap(),
        )
        .unwrap();

        let plan_store = FilePlanStore::new(
            std::env::temp_dir().join("argus-thread-test-plan"),
            "thread-1",
        );

        let thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(compactor)
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .plan_store(plan_store)
            .build()
            .unwrap();

        let mut snapshot = thread.plan();
        assert_eq!(
            snapshot,
            vec![json!({
                "step": "Inspect review feedback",
                "status": "completed"
            })]
        );

        snapshot.push(json!({
            "step": "Mutate local copy",
            "status": "pending"
        }));

        assert_eq!(thread.plan().len(), 1);
        assert_eq!(thread.info().plan_item_count, 1);
    }

    #[test]
    fn thread_handle_enqueue_tracks_pending_queue_depth_while_running() {
        let mut handle = ThreadHandle::new();

        let first = handle.dispatch(ThreadCommand::EnqueueUserMessage {
            content: "first".to_string(),
            msg_override: None,
        });
        assert!(matches!(
            first,
            ThreadRuntimeAction::StartTurn { turn_number: 1, .. }
        ));

        let second = handle.dispatch(ThreadCommand::EnqueueUserMessage {
            content: "second".to_string(),
            msg_override: None,
        });
        assert!(matches!(second, ThreadRuntimeAction::Noop));

        let snapshot = handle.snapshot();
        assert_eq!(
            snapshot.state,
            ThreadRuntimeState::Running { turn_number: 1 }
        );
        assert_eq!(snapshot.queue_depth, 1);
    }

    #[tokio::test]
    async fn cancelled_or_completed_turn_starts_next_queued_message() {
        let provider = Arc::new(SequencedProvider::new(
            Duration::from_millis(120),
            vec![
                ResponsePlan::Ok("first turn reply".to_string()),
                ResponsePlan::Ok("second turn reply".to_string()),
            ],
        ));
        let thread = build_test_thread_with_provider(provider.clone());

        Thread::spawn_runtime_actor(Arc::clone(&thread));

        {
            let guard = thread.read().await;
            guard
                .send_user_message("first queued".to_string(), None)
                .expect("first message should queue");
        }

        sleep(Duration::from_millis(20)).await;

        {
            let guard = thread.read().await;
            guard
                .send_user_message("second queued".to_string(), None)
                .expect("second message should queue");
        }

        sleep(Duration::from_millis(30)).await;
        {
            let guard = thread.read().await;
            assert_eq!(guard.turn_count(), 1);
        }
        assert_eq!(provider.captured_user_inputs().len(), 1);

        wait_for_idle_events(&thread, 2).await;

        let captured = provider.captured_user_inputs();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0], "first queued");
        assert_eq!(captured[1], "second queued");
    }

    #[tokio::test]
    async fn runtime_actor_emits_turn_settled_after_completed_turn() {
        let provider = Arc::new(SequencedProvider::new(
            Duration::from_millis(20),
            vec![ResponsePlan::Ok("settled reply".to_string())],
        ));
        let thread = build_test_thread_with_provider(provider);

        Thread::spawn_runtime_actor(Arc::clone(&thread));

        {
            let guard = thread.read().await;
            guard
                .send_user_message("settle this turn".to_string(), None)
                .expect("message should queue");
        }

        wait_for_idle_events(&thread, 1).await;

        let guard = thread.read().await;
        assert_eq!(guard.turn_count(), 1);
        assert!(guard.token_count() > 0);
    }

    #[tokio::test]
    async fn user_interrupt_cancels_active_turn_and_preserves_queue() {
        let provider = Arc::new(SequencedProvider::new(
            Duration::from_millis(120),
            vec![
                ResponsePlan::Ok("first turn reply".to_string()),
                ResponsePlan::Ok("second turn reply".to_string()),
            ],
        ));
        let thread = build_test_thread_with_provider(provider.clone());

        Thread::spawn_runtime_actor(Arc::clone(&thread));

        {
            let guard = thread.read().await;
            guard
                .send_user_message("first queued".to_string(), None)
                .expect("first message should queue");
        }

        sleep(Duration::from_millis(20)).await;

        {
            let guard = thread.read().await;
            guard
                .send_user_message("second queued".to_string(), None)
                .expect("second message should queue");
        }

        sleep(Duration::from_millis(20)).await;

        {
            let guard = thread.read().await;
            guard
                .send_control_event(ThreadControlEvent::UserInterrupt {
                    content: "stop".to_string(),
                })
                .expect("interrupt should request stop");
        }

        wait_for_idle_events(&thread, 2).await;

        let captured = provider.captured_user_inputs();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0], "first queued");
        assert_eq!(captured[1], "second queued");

        let assistant_count = {
            let guard = thread.read().await;
            guard
                .history()
                .iter()
                .filter(|message| message.role == argus_protocol::llm::Role::Assistant)
                .count()
        };

        assert_eq!(
            assistant_count, 1,
            "cancelled first turn should not append assistant output",
        );

        let (user_messages, assistant_messages) = {
            let guard = thread.read().await;
            let user_messages = guard
                .history()
                .iter()
                .filter(|message| message.role == argus_protocol::llm::Role::User)
                .map(|message| message.content.clone())
                .collect::<Vec<_>>();
            let assistant_messages = guard
                .history()
                .iter()
                .filter(|message| message.role == argus_protocol::llm::Role::Assistant)
                .map(|message| message.content.clone())
                .collect::<Vec<_>>();
            (user_messages, assistant_messages)
        };

        assert_eq!(
            user_messages,
            vec!["first queued".to_string(), "second queued".to_string()],
            "stop should preserve the user bubbles that were already sent",
        );
        assert_eq!(
            assistant_messages.len(),
            1,
            "stop should discard only the cancelled turn's assistant output",
        );
    }

    #[tokio::test]
    async fn legacy_mailbox_interrupt_does_not_leak_into_next_turn_after_idle_handoff() {
        let provider = Arc::new(SequencedProvider::new(
            Duration::from_millis(120),
            vec![
                ResponsePlan::Ok("first turn reply".to_string()),
                ResponsePlan::Ok("second turn reply".to_string()),
            ],
        ));
        let thread = build_test_thread_with_provider(provider.clone());

        Thread::spawn_runtime_actor(Arc::clone(&thread));

        {
            let guard = thread.read().await;
            guard
                .send_user_message("first queued".to_string(), None)
                .expect("first message should queue");
        }

        sleep(Duration::from_millis(20)).await;

        {
            let mailbox = {
                let guard = thread.read().await;
                guard.mailbox()
            };

            let mut guard = mailbox.lock().await;
            guard.push(ThreadControlEvent::UserInterrupt {
                content: "late interrupt".to_string(),
            });
        }

        wait_for_idle_events(&thread, 1).await;

        {
            let guard = thread.read().await;
            guard
                .send_user_message("second queued".to_string(), None)
                .expect("second message should queue");
        }

        wait_for_idle_events(&thread, 1).await;

        let captured = provider.captured_user_inputs();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0], "first queued");
        assert_eq!(
            captured[1], "second queued",
            "late interrupt should be cleared on idle handoff",
        );
    }

    /// A compactor that always returns a single-user-message result.
    struct SingleMessageCompactor;

    #[async_trait]
    impl Compactor for SingleMessageCompactor {
        async fn compact(
            &self,
            _provider: &dyn LlmProvider,
            _messages: &[ChatMessage],
            _token_count: u32,
        ) -> Result<Option<CompactResult>, CompactError> {
            Ok(Some(CompactResult {
                summary_messages: vec![ChatMessage::user("compacted")],
                token_count: 10,
                trace_prelude_messages: vec![],
            }))
        }

        fn name(&self) -> &'static str {
            "single_message"
        }
    }

    #[tokio::test]
    async fn begin_turn_broadcasts_compacted_event_when_pre_turn_compaction_changes_history() {
        let compactor: Arc<dyn Compactor> = Arc::new(SingleMessageCompactor);
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(SmallContextProvider {
                context_window: 100,
            }))
            .compactor(compactor)
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");
        let repeated = repeated_test_message();
        let persisted_messages = vec![
            ChatMessage::user(repeated.clone()),
            ChatMessage::user(repeated.clone()),
            ChatMessage::user(repeated.clone()),
        ];
        thread.hydrate_from_persisted_state(
            persisted_messages.clone(),
            token_count_for_messages(&persisted_messages),
            0,
            Utc::now(),
        );

        let mut rx = thread.subscribe();
        let _turn = thread
            .begin_turn("next".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should build after compaction");

        let event = timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("thread should emit compacted event")
            .expect("event should be readable");
        assert!(matches!(
            event,
            ThreadEvent::Compacted {
                new_token_count: 10,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn compaction_does_not_rewrite_committed_turn_history() {
        let compactor: Arc<dyn Compactor> = Arc::new(SingleMessageCompactor);
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(SmallContextProvider {
                context_window: 100,
            }))
            .compactor(compactor)
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");
        let repeated = repeated_test_message();
        let persisted_messages = vec![
            ChatMessage::user(repeated.clone()),
            ChatMessage::assistant("history reply"),
            ChatMessage::user(repeated),
        ];
        thread.hydrate_from_persisted_state(
            persisted_messages,
            90,
            0,
            Utc::now(),
        );
        let before = thread.history().to_vec();

        let _turn = thread
            .begin_turn("follow-up".to_string(), None, TurnCancellation::default())
            .await
            .expect("turn should build after compaction");

        let after = thread.history().to_vec();
        assert_eq!(after.len(), before.len());
        for (after_message, before_message) in after.iter().zip(before.iter()) {
            assert_eq!(after_message.role, before_message.role);
            assert_eq!(after_message.content, before_message.content);
        }
    }

    #[tokio::test]
    async fn build_turn_context_uses_compaction_checkpoint_summary_messages() {
        let compactor: Arc<dyn Compactor> = Arc::new(SingleMessageCompactor);
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(SmallContextProvider {
                context_window: 100,
            }))
            .compactor(compactor)
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");
        let persisted_messages = vec![
            ChatMessage::user("old question"),
            ChatMessage::assistant("old answer"),
        ];
        thread.hydrate_from_persisted_state(
            persisted_messages,
            90,
            0,
            Utc::now(),
        );

        let _turn = thread
            .begin_turn("follow-up".to_string(), None, TurnCancellation::default())
            .await
            .expect("turn should build after compaction");

        let context = thread.build_turn_context();
        assert_eq!(context.len(), 1);
        assert_eq!(context[0].content, "compacted");
    }

    #[tokio::test]
    async fn begin_turn_does_not_broadcast_compacted_event_when_history_is_unchanged() {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(SmallContextProvider {
                context_window: 100,
            }))
            .compactor(compactor)
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");
        let persisted_messages = vec![ChatMessage::user(repeated_test_message())];
        thread.hydrate_from_persisted_state(
            persisted_messages.clone(),
            token_count_for_messages(&persisted_messages),
            0,
            Utc::now(),
        );

        let mut rx = thread.subscribe();
        let _turn = thread
            .begin_turn("next".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should build without compaction");

        let no_event = timeout(Duration::from_millis(50), rx.recv()).await;
        assert!(
            no_event.is_err(),
            "unchanged history should not emit compacted event",
        );
    }

    #[tokio::test]
    async fn begin_turn_ignores_compaction_failure_and_does_not_emit_compacted_event() {
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(SmallContextProvider {
                context_window: 100,
            }))
            .compactor(Arc::new(FailingCompactor))
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");
        let repeated = repeated_test_message();
        let persisted_messages = vec![
            ChatMessage::user(repeated.clone()),
            ChatMessage::user(repeated),
        ];
        thread.hydrate_from_persisted_state(
            persisted_messages.clone(),
            token_count_for_messages(&persisted_messages),
            0,
            Utc::now(),
        );

        let mut rx = thread.subscribe();
        let _turn = thread
            .begin_turn("next".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should still build after compact failure");

        let no_event = timeout(Duration::from_millis(50), rx.recv()).await;
        assert!(
            no_event.is_err(),
            "failed compaction should not emit compacted event",
        );
    }

    #[tokio::test]
    async fn begin_turn_preserves_authoritative_token_count_until_visible_turn_completes() {
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(SmallContextProvider {
                context_window: 4_096,
            }))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");
        let persisted_messages = vec![ChatMessage::user(repeated_test_message())];
        let next_message = "next message".to_string();
        let authoritative_token_count = token_count_for_messages(&persisted_messages);
        thread.hydrate_from_persisted_state(
            persisted_messages.clone(),
            authoritative_token_count,
            0,
            Utc::now(),
        );

        let _turn = thread
            .begin_turn(next_message.clone(), None, TurnCancellation::new())
            .await
            .expect("turn should build");

        assert_eq!(thread.token_count(), authoritative_token_count);
    }

    #[tokio::test]
    async fn finish_turn_moves_current_turn_into_turns() {
        let mut thread = build_test_thread();
        let turn = thread
            .begin_turn("hi".to_string(), None, TurnCancellation::default())
            .await
            .expect("turn should build");

        let output = TurnOutput {
            appended_messages: vec![ChatMessage::assistant("hello")],
            token_usage: argus_protocol::TokenUsage {
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
            },
        };

        assert_eq!(thread.history().len(), 1);
        assert_eq!(thread.history()[0].content, "You are a test agent.");
        assert!(thread.current_turn.is_some());
        assert_eq!(thread.turns.len(), 0);

        drop(turn);
        thread.finish_turn(Ok(output)).expect("turn should settle");

        assert!(thread.current_turn.is_none());
        assert_eq!(thread.turn_count(), 1);
        assert_eq!(thread.turns.len(), 1);
        assert_eq!(thread.turns[0].messages.len(), 2);
        assert_eq!(thread.turns[0].messages[0].content, "hi");
        assert_eq!(thread.turns[0].messages[1].content, "hello");
    }

    #[tokio::test]
    async fn finish_turn_cancelled_preserves_last_authoritative_token_count() {
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(SmallContextProvider {
                context_window: 4_096,
            }))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");
        let persisted_messages = vec![ChatMessage::user(repeated_test_message())];
        let next_message = "cancel me".to_string();
        let authoritative_token_count = token_count_for_messages(&persisted_messages);
        thread.hydrate_from_persisted_state(
            persisted_messages.clone(),
            authoritative_token_count,
            0,
            Utc::now(),
        );

        let _turn = thread
            .begin_turn(next_message.clone(), None, TurnCancellation::new())
            .await
            .expect("turn should build");
        thread
            .finish_turn(Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)))
            .expect("cancelled turn should be ignored");

        assert_eq!(thread.token_count(), authoritative_token_count);
        assert_eq!(thread.turns.len(), 1);
        assert!(matches!(thread.turns[0].state, crate::history::TurnState::Cancelled));
        assert_eq!(thread.turns[0].messages.len(), 1);
        assert_eq!(thread.turns[0].messages[0].content, next_message);
    }

    #[tokio::test]
    async fn finish_turn_preserves_legacy_turn_count_bridge() {
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");
        let persisted_messages = vec![ChatMessage::user("legacy")];
        thread.hydrate_from_persisted_state(persisted_messages, 1, 5, Utc::now());

        let _turn = thread
            .begin_turn("next".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should build");
        thread
            .finish_turn(Ok(TurnOutput {
                appended_messages: vec![
                    ChatMessage::user("next"),
                    ChatMessage::assistant("done"),
                ],
                token_usage: argus_protocol::TokenUsage {
                    input_tokens: 1,
                    output_tokens: 1,
                    total_tokens: 2,
                },
            }))
            .expect("turn should settle");

        assert_eq!(thread.turn_count(), 6);
    }

    #[tokio::test]
    async fn turn_log_persistence_snapshot_writes_messages_and_meta() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let trace_config =
            TraceConfig::new(true, temp_dir.path().to_path_buf()).with_session_id(session_id);
        let thread_config = ThreadConfig {
            turn_config: TurnConfigBuilder::default()
                .trace_config(trace_config)
                .build()
                .expect("turn config should build"),
        };
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(session_id)
            .config(thread_config)
            .build()
            .expect("thread should build");

        let _turn = thread
            .begin_turn("hi".to_string(), None, TurnCancellation::default())
            .await
            .expect("turn should build");
        thread
            .finish_turn(Ok(TurnOutput {
                appended_messages: vec![ChatMessage::assistant("hello")],
                token_usage: argus_protocol::TokenUsage {
                    input_tokens: 1,
                    output_tokens: 1,
                    total_tokens: 2,
                },
            }))
            .expect("turn should settle");

        let snapshot = thread
            .turn_log_persistence_snapshot()
            .expect("trace-enabled thread should expose persistence snapshot");
        persist_turn_log_snapshot(&snapshot)
            .await
            .expect("snapshot should persist");

        let persisted_turns_dir = turns_dir(&snapshot.base_dir);
        let messages = read_turn_messages(&turn_messages_path(&persisted_turns_dir, 1))
            .await
            .expect("messages should read");
        let meta = read_turn_meta(&turn_meta_path(&persisted_turns_dir, 1))
            .await
            .expect("meta should read");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "hi");
        assert_eq!(messages[1].content, "hello");
        assert_eq!(meta.turn_number, 1);
        assert!(matches!(meta.state, crate::history::TurnState::Completed));
        assert_eq!(meta.token_usage.as_ref().map(|usage| usage.total_tokens), Some(2));
    }

    #[tokio::test]
    async fn finish_turn_failure_settles_failed_turn_and_returns_error() {
        let mut thread = build_test_thread();
        let _turn = thread
            .begin_turn("hi".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should build");

        let result = thread.finish_turn(Err(ThreadError::TurnFailed(
            crate::TurnError::ToolExecutionFailed {
                name: "search".to_string(),
                reason: "boom".to_string(),
            },
        )));

        assert!(matches!(result, Err(ThreadError::TurnFailed(_))));
        assert_eq!(thread.turns.len(), 1);
        assert!(matches!(thread.turns[0].state, crate::history::TurnState::Failed));
        assert_eq!(thread.turns[0].messages.len(), 1);
        assert_eq!(thread.turns[0].messages[0].content, "hi");
        assert_eq!(thread.token_count(), 0);
    }

}
