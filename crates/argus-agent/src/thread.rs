//! Thread implementation.

use std::collections::VecDeque;
use std::sync::{Arc, Weak};

use chrono::{DateTime, Utc};
use derive_builder::Builder;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::task::{JoinError, JoinHandle};

use crate::turn::{self, TurnCancellation};
use argus_protocol::llm::{ChatMessage, LlmProvider};
use argus_protocol::tool::NamedTool;
use argus_protocol::{
    AgentRecord, HookHandler, HookRegistry, McpToolResolver, MessageOverride, QueuedUserMessage,
    SessionId, ThreadControlMessage, ThreadEvent, ThreadId, ThreadMessage, ThreadNoticeLevel,
    TokenUsage,
};
use argus_tool::ToolManager;

use super::compact::Compactor;
use super::compact::turn::LlmTurnCompactor;
use super::config::ThreadConfig;
use super::error::{ThreadError, TurnLogError};
use super::history::{TurnRecord, TurnRecordKind, derive_next_user_turn_number};
use super::plan_hook::PlanContinuationHook;
use super::plan_store::FilePlanStore;
use super::plan_tool::UpdatePlanTool;
use super::turn_log_store::{
    RecoveredThreadLogState, append_turn_record, recover_thread_log_state,
};
use super::types::{ThreadInfo, ThreadRuntimeSnapshot, ThreadState};
/// Default broadcast channel capacity.
const DEFAULT_CHANNEL_CAPACITY: usize = 256;

type TurnExecutionParts = (
    String,
    Arc<Vec<ChatMessage>>,
    Arc<Vec<Arc<dyn NamedTool>>>,
    Arc<Vec<Arc<dyn HookHandler>>>,
    Arc<dyn LlmProvider>,
    crate::TurnConfig,
    broadcast::Sender<ThreadEvent>,
    Arc<dyn Compactor>,
);

/// Internal runtime state for a loaded thread actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadRuntimeState {
    /// Runtime is idle and ready for work.
    Idle,
    /// Runtime is executing a turn.
    Running {
        /// Active turn number.
        turn_number: u32,
    },
    /// Runtime is stopping an active turn.
    Stopping {
        /// Active turn number being stopped.
        turn_number: u32,
    },
}

/// Runtime decisions emitted by the thread loop.
#[derive(Debug, Clone)]
pub(crate) enum ThreadLoopAction {
    /// Start a new turn immediately.
    StartTurn {
        /// Turn number to execute.
        turn_number: u32,
        /// User message content.
        content: String,
        /// Optional per-message overrides.
        msg_override: Option<MessageOverride>,
    },
    /// Active turn should be stopped.
    StopTurn {
        /// Turn number being stopped.
        turn_number: u32,
    },
    /// No immediate action is required.
    Noop,
}

enum ThreadOwnerCommand {
    SetTitle {
        title: Option<String>,
        ack: oneshot::Sender<()>,
    },
    SetProvider {
        provider: Arc<dyn LlmProvider>,
        ack: oneshot::Sender<()>,
    },
    SetMcpToolResolver {
        resolver: Option<Arc<dyn McpToolResolver>>,
        ack: oneshot::Sender<()>,
    },
}

struct ThreadHandleInner {
    id: ThreadId,
    session_id: SessionId,
    message_tx: mpsc::UnboundedSender<ThreadMessage>,
    pipe_tx: broadcast::Sender<ThreadEvent>,
    snapshot_rx: watch::Receiver<ThreadRuntimeSnapshot>,
    terminated_rx: watch::Receiver<bool>,
}

/// Cloneable observer/control handle for a loaded thread runtime.
#[derive(Clone)]
pub struct ThreadHandle {
    inner: Arc<ThreadHandleInner>,
}

/// Cloneable owner handle for a loaded thread runtime.
#[derive(Clone)]
pub struct ThreadOwnerHandle {
    handle: ThreadHandle,
    owner_tx: mpsc::UnboundedSender<ThreadOwnerCommand>,
}

/// Weak handle used by session-side caches to avoid keeping runtimes resident.
#[derive(Clone)]
pub struct WeakThreadHandle {
    inner: Weak<ThreadHandleInner>,
}

impl std::fmt::Debug for ThreadHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadHandle")
            .field("id", &self.id())
            .field("session_id", &self.session_id())
            .field("state", &self.state())
            .finish()
    }
}

impl std::fmt::Debug for WeakThreadHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WeakThreadHandle")
            .field("live", &self.inner.strong_count())
            .finish()
    }
}

impl std::fmt::Debug for ThreadOwnerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadOwnerHandle")
            .field("id", &self.id())
            .field("session_id", &self.session_id())
            .field("state", &self.observer().state())
            .finish()
    }
}

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

    /// Last update timestamp.
    #[builder(default = "Utc::now()")]
    updated_at: DateTime<Utc>,

    /// Settled turn history — single source of truth.
    #[builder(default)]
    turns: Vec<TurnRecord>,

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

    /// Current runtime state.
    #[builder(default)]
    runtime_state: ThreadRuntimeState,

    /// Cancellation handle for the active turn, if any.
    #[builder(default)]
    active_turn_cancellation: Option<TurnCancellation>,

    /// Pipe for sending/receiving ThreadEvents.
    #[builder(default)]
    pipe_tx: broadcast::Sender<ThreadEvent>,

    /// Eventually consistent runtime snapshot publisher for loaded-handle reads.
    #[builder(default)]
    snapshot_tx: Option<watch::Sender<ThreadRuntimeSnapshot>>,

    /// Thread ingress sender for low-volume orchestration and payload messages.
    #[builder(default)]
    message_tx: mpsc::UnboundedSender<ThreadMessage>,

    /// Single-consumer thread ingress receiver, taken by the session orchestrator.
    #[builder(default)]
    message_rx: Option<mpsc::UnboundedReceiver<ThreadMessage>>,

    /// FIFO payload messages owned by the thread runtime.
    #[builder(default)]
    pending_messages: VecDeque<ThreadMessage>,

    /// Owner-only command receiver, taken when a runtime handle is spawned.
    #[builder(default)]
    owner_rx: Option<mpsc::UnboundedReceiver<ThreadOwnerCommand>>,

    /// Optional runtime resolver that injects ready MCP tools for this agent.
    #[builder(default, setter(strip_option))]
    mcp_tool_resolver: Option<Arc<dyn McpToolResolver>>,

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
            .field("turns", &self.turns.len())
            .field("token_count", &self.token_count())
            .field("turn_count", &self.turn_count())
            .field("runtime_state", &self.runtime_state)
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

    /// Build the Thread.
    ///
    /// # Errors
    ///
    /// Returns `ThreadError` if required fields (`provider`, `compactor`, `agent_record`, `session_id`) are not set.
    pub fn build(self) -> Result<Thread, ThreadError> {
        let (pipe_tx, _) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);
        let (message_tx, message_rx) = mpsc::unbounded_channel();

        let agent_record = self.agent_record.ok_or(ThreadError::AgentRecordNotSet)?;
        let session_id = self.session_id.ok_or(ThreadError::SessionIdNotSet)?;
        let tool_manager = self
            .tool_manager
            .unwrap_or_else(|| Arc::new(ToolManager::new()));
        let hooks = self.hooks.flatten();
        let plan_store = self.plan_store.unwrap_or_default();
        let turns = self.turns.unwrap_or_default();
        Ok(Thread {
            id: self.id.unwrap_or_default(),
            agent_record,
            session_id,
            title: self.title.flatten(),
            updated_at: self.updated_at.unwrap_or_else(Utc::now),
            turns,
            provider: self.provider.ok_or(ThreadError::ProviderNotConfigured)?,
            tool_manager,
            compactor: self.compactor.ok_or(ThreadError::CompactorNotConfigured)?,
            hooks,
            config: self.config.unwrap_or_default(),
            runtime_state: ThreadRuntimeState::Idle,
            active_turn_cancellation: None,
            pipe_tx,
            snapshot_tx: None,
            message_tx,
            message_rx: Some(message_rx),
            pending_messages: VecDeque::new(),
            owner_rx: None,
            mcp_tool_resolver: self.mcp_tool_resolver.flatten(),
            plan_store,
        })
    }
}

impl WeakThreadHandle {
    /// Upgrade a weak runtime handle into a live handle if the runtime is still resident.
    pub fn upgrade(&self) -> Option<ThreadHandle> {
        self.inner.upgrade().map(|inner| ThreadHandle { inner })
    }
}

impl ThreadHandle {
    fn with_snapshot<T>(&self, project: impl FnOnce(&ThreadRuntimeSnapshot) -> T) -> T {
        let snapshot = self.inner.snapshot_rx.borrow();
        project(&snapshot)
    }

    /// Downgrade this runtime handle for cache-only storage.
    pub fn downgrade(&self) -> WeakThreadHandle {
        WeakThreadHandle {
            inner: Arc::downgrade(&self.inner),
        }
    }

    /// Returns true when two handles point at the same live runtime instance.
    pub fn same_runtime(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Get the thread identifier.
    pub fn id(&self) -> ThreadId {
        self.inner.id
    }

    /// Get the session identifier.
    pub fn session_id(&self) -> SessionId {
        self.inner.session_id
    }

    /// Subscribe to runtime thread events.
    pub fn subscribe(&self) -> broadcast::Receiver<ThreadEvent> {
        self.inner.pipe_tx.subscribe()
    }

    /// Send a protocol message into the thread mailbox.
    pub fn send_message(&self, message: ThreadMessage) -> Result<(), ThreadError> {
        if matches!(message, ThreadMessage::Control(_)) {
            return Err(ThreadError::OwnerControlRestricted);
        }
        self.inner
            .message_tx
            .send(message)
            .map_err(|_| ThreadError::ChannelClosed)
    }

    /// Return the latest eventually consistent runtime snapshot.
    pub fn snapshot(&self) -> ThreadRuntimeSnapshot {
        self.inner.snapshot_rx.borrow().clone()
    }

    /// Return the latest title.
    pub fn title(&self) -> Option<String> {
        self.with_snapshot(|snapshot| snapshot.title.clone())
    }

    /// Return the latest update timestamp.
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.with_snapshot(|snapshot| snapshot.updated_at)
    }

    /// Return committed history from the latest snapshot.
    pub fn history(&self) -> Vec<ChatMessage> {
        self.with_snapshot(|snapshot| snapshot.history.clone())
    }

    /// Return the current turn count.
    pub fn turn_count(&self) -> u32 {
        self.with_snapshot(|snapshot| snapshot.turn_count)
    }

    /// Return the current token count.
    pub fn token_count(&self) -> u32 {
        self.with_snapshot(|snapshot| snapshot.token_count)
    }

    /// Return the current plan item count.
    pub fn plan_item_count(&self) -> usize {
        self.with_snapshot(|snapshot| snapshot.plan_item_count)
    }

    /// Return the cached provider/model label.
    pub fn provider_model(&self) -> String {
        self.with_snapshot(|snapshot| snapshot.provider_model.clone())
    }

    /// Return the cached agent display name.
    pub fn agent_display_name(&self) -> String {
        self.with_snapshot(|snapshot| snapshot.agent_display_name.clone())
    }

    /// Return the cached agent identifier.
    pub fn agent_id(&self) -> argus_protocol::AgentId {
        self.with_snapshot(|snapshot| snapshot.agent_id)
    }

    /// Return the cached agent description.
    pub fn agent_description(&self) -> String {
        self.with_snapshot(|snapshot| snapshot.agent_description.clone())
    }

    /// Return the cached frozen system prompt.
    pub fn agent_system_prompt(&self) -> String {
        self.with_snapshot(|snapshot| snapshot.agent_system_prompt.clone())
    }

    /// Return the cached trace base directory.
    pub fn trace_base_dir(&self) -> Option<std::path::PathBuf> {
        self.with_snapshot(|snapshot| snapshot.trace_base_dir.clone())
    }

    /// Return the cached estimated memory bytes.
    pub fn estimated_memory_bytes(&self) -> u64 {
        self.with_snapshot(|snapshot| snapshot.estimated_memory_bytes)
    }

    /// Returns true when the runtime currently reports a running turn.
    pub fn is_turn_running(&self) -> bool {
        self.state() != ThreadState::Idle
    }

    /// Return the cached runtime state.
    pub fn state(&self) -> ThreadState {
        self.with_snapshot(|snapshot| snapshot.state)
    }

    /// Returns true when the snapshot contains visible history.
    pub fn has_non_system_history(&self) -> bool {
        self.with_snapshot(|snapshot| !snapshot.history.is_empty())
    }

    /// Wait until the runtime owner loop exits.
    pub async fn wait_for_termination(&self) {
        let mut terminated_rx = self.inner.terminated_rx.clone();
        if *terminated_rx.borrow() {
            return;
        }
        while terminated_rx.changed().await.is_ok() {
            if *terminated_rx.borrow() {
                return;
            }
        }
    }
}

impl ThreadOwnerHandle {
    async fn send_owner_command(
        &self,
        command: impl FnOnce(oneshot::Sender<()>) -> ThreadOwnerCommand,
    ) -> Result<(), ThreadError> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.owner_tx
            .send(command(ack_tx))
            .map_err(|_| ThreadError::ChannelClosed)?;
        ack_rx.await.map_err(|_| ThreadError::ChannelClosed)
    }

    /// Get the thread identifier.
    pub fn id(&self) -> ThreadId {
        self.handle.id()
    }

    /// Get the session identifier.
    pub fn session_id(&self) -> SessionId {
        self.handle.session_id()
    }

    /// Return the observer/control handle exposed to non-owner callers.
    pub fn observer(&self) -> ThreadHandle {
        self.handle.clone()
    }

    /// Returns true when two owner handles point at the same live runtime instance.
    pub fn same_runtime(&self, other: &Self) -> bool {
        self.handle.same_runtime(&other.handle)
    }

    /// Update the in-memory runtime title.
    pub async fn set_title(&self, title: Option<String>) -> Result<(), ThreadError> {
        self.send_owner_command(|ack| ThreadOwnerCommand::SetTitle { title, ack })
            .await
    }

    /// Update the bound provider for subsequent turns.
    pub async fn set_provider(&self, provider: Arc<dyn LlmProvider>) -> Result<(), ThreadError> {
        self.send_owner_command(|ack| ThreadOwnerCommand::SetProvider { provider, ack })
            .await
    }

    /// Update the MCP tool resolver for subsequent turns.
    pub async fn set_mcp_tool_resolver(
        &self,
        resolver: Option<Arc<dyn McpToolResolver>>,
    ) -> Result<(), ThreadError> {
        self.send_owner_command(|ack| ThreadOwnerCommand::SetMcpToolResolver { resolver, ack })
            .await
    }

    /// Request shutdown of the owner loop and wait for termination.
    pub async fn shutdown(self) -> Result<(), ThreadError> {
        self.handle
            .inner
            .message_tx
            .send(ThreadMessage::Control(ThreadControlMessage::ShutdownRuntime))
            .map_err(|_| ThreadError::ChannelClosed)?;
        self.handle.wait_for_termination().await;
        Ok(())
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
        self.publish_runtime_snapshot();
    }

    /// Get last update timestamp.
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    fn runtime_state_view(&self) -> ThreadState {
        if self.is_turn_running() {
            ThreadState::Processing
        } else {
            ThreadState::Idle
        }
    }

    fn estimate_memory_bytes(&self) -> u64 {
        let history_bytes = self
            .history_iter()
            .map(|message| message.content.len() as u64)
            .sum::<u64>();
        let plan_bytes = self.plan_store.store().read().unwrap().len() as u64 * 128;
        history_bytes + plan_bytes + u64::from(self.token_count())
    }

    fn runtime_snapshot(&self) -> ThreadRuntimeSnapshot {
        let plan_item_count = self.plan_store.store().read().unwrap().len();
        ThreadRuntimeSnapshot {
            id: self.id,
            session_id: self.session_id,
            title: self.title.clone(),
            updated_at: self.updated_at,
            history: self.history_iter().cloned().collect(),
            turn_count: self.turn_count(),
            token_count: self.token_count(),
            plan_item_count,
            state: self.runtime_state_view(),
            provider_model: self.provider.model_name().to_string(),
            agent_display_name: self.agent_record.display_name.clone(),
            agent_id: self.agent_record.id,
            agent_description: self.agent_record.description.clone(),
            agent_system_prompt: self.agent_record.system_prompt.clone(),
            trace_base_dir: self.trace_base_dir(),
            estimated_memory_bytes: self.estimate_memory_bytes(),
        }
    }

    fn publish_runtime_snapshot(&self) {
        if let Some(snapshot_tx) = &self.snapshot_tx {
            let _ = snapshot_tx.send(self.runtime_snapshot());
        }
    }

    fn take_owner_rx(&mut self) -> Option<mpsc::UnboundedReceiver<ThreadOwnerCommand>> {
        self.owner_rx.take()
    }

    /// Spawn a single-owner runtime task and return the cloneable observer/control handle.
    pub fn spawn_runtime(mut self) -> Result<ThreadOwnerHandle, ThreadError> {
        let message_rx = self.take_message_rx().ok_or(ThreadError::RuntimeActive)?;
        let (owner_tx, owner_rx) = mpsc::unbounded_channel();
        let (snapshot_tx, snapshot_rx) = watch::channel(self.runtime_snapshot());
        let (terminated_tx, terminated_rx) = watch::channel(false);
        self.snapshot_tx = Some(snapshot_tx);
        self.owner_rx = Some(owner_rx);
        self.reset_runtime_loop_state();
        self.publish_runtime_snapshot();

        let handle = ThreadHandle {
            inner: Arc::new(ThreadHandleInner {
                id: self.id,
                session_id: self.session_id,
                message_tx: self.message_tx.clone(),
                pipe_tx: self.pipe_tx.clone(),
                snapshot_rx,
                terminated_rx,
            }),
        };
        let owner_handle = ThreadOwnerHandle { handle, owner_tx };

        let owner_rx = self.take_owner_rx().ok_or(ThreadError::ChannelClosed)?;
        tokio::spawn(async move {
            Self::run_reactor_loop(self, message_rx, owner_rx).await;
            let _ = terminated_tx.send(true);
        });

        Ok(owner_handle)
    }

    /// Get information about this thread.
    pub fn info(&self) -> ThreadInfo {
        ThreadInfo {
            id: self.id.to_string(),
            message_count: self.history_iter().count(),
            token_count: self.token_count(),
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

    /// Send a message into the thread-owned runtime ingress.
    pub fn send_message(&self, message: ThreadMessage) -> Result<(), ThreadError> {
        self.message_tx
            .send(message)
            .map_err(|_| ThreadError::ChannelClosed)
    }

    /// Take the single thread ingress receiver owned by the session orchestrator.
    pub fn take_message_rx(&mut self) -> Option<mpsc::UnboundedReceiver<ThreadMessage>> {
        self.message_rx.take()
    }

    /// Returns true if a Turn is currently executing.
    pub fn is_turn_running(&self) -> bool {
        !matches!(self.runtime_state, ThreadRuntimeState::Idle)
    }

    fn reset_runtime_loop_state(&mut self) {
        self.runtime_state = ThreadRuntimeState::Idle;
        self.active_turn_cancellation = None;
        self.publish_runtime_snapshot();
    }

    fn dispatch_runtime_message(&mut self, message: ThreadMessage) -> ThreadLoopAction {
        match message {
            ThreadMessage::UserInput { .. }
            | ThreadMessage::PeerMessage { .. }
            | ThreadMessage::JobResult { .. } => {
                self.pending_messages.push_back(message);
                self.try_start_next_turn()
            }
            ThreadMessage::Interrupt => self.cancel_active_turn(),
            ThreadMessage::Control(ThreadControlMessage::ShutdownRuntime) => ThreadLoopAction::Noop,
        }
    }

    fn complete_runtime_turn(&mut self, _committed: bool) -> ThreadLoopAction {
        self.runtime_state = ThreadRuntimeState::Idle;
        self.publish_runtime_snapshot();
        self.try_start_next_turn()
    }

    fn try_start_next_turn(&mut self) -> ThreadLoopAction {
        if !matches!(self.runtime_state, ThreadRuntimeState::Idle) {
            return ThreadLoopAction::Noop;
        }

        match self.take_next_turn_message() {
            Some(message) => self.start_runtime_turn(message),
            None => ThreadLoopAction::Noop,
        }
    }

    fn start_runtime_turn(&mut self, message: QueuedUserMessage) -> ThreadLoopAction {
        let turn_number = derive_next_user_turn_number(&self.turns);
        self.runtime_state = ThreadRuntimeState::Running { turn_number };
        self.publish_runtime_snapshot();

        ThreadLoopAction::StartTurn {
            turn_number,
            content: message.content,
            msg_override: message.msg_override,
        }
    }

    fn cancel_active_turn(&mut self) -> ThreadLoopAction {
        match self.runtime_state {
            ThreadRuntimeState::Running { turn_number } => {
                self.runtime_state = ThreadRuntimeState::Stopping { turn_number };
                self.publish_runtime_snapshot();
                ThreadLoopAction::StopTurn { turn_number }
            }
            ThreadRuntimeState::Idle | ThreadRuntimeState::Stopping { .. } => {
                ThreadLoopAction::Noop
            }
        }
    }

    fn take_next_turn_message(&mut self) -> Option<QueuedUserMessage> {
        match self.pending_messages.pop_front()? {
            ThreadMessage::UserInput {
                content,
                msg_override,
            } => Some(QueuedUserMessage {
                content,
                msg_override,
            }),
            ThreadMessage::PeerMessage { message } | ThreadMessage::JobResult { message } => {
                Some(message.into_queued_user_message())
            }
            ThreadMessage::Interrupt | ThreadMessage::Control(_) => None,
        }
    }

    /// Returns true when committed history contains any visible transcript.
    pub fn has_non_system_history(&self) -> bool {
        self.history_iter().next().is_some()
    }

    /// Iterate over committed message history from turn records.
    pub fn history_iter(&self) -> impl Iterator<Item = &ChatMessage> + '_ {
        self.turns
            .iter()
            .filter(|turn| {
                matches!(
                    turn.kind,
                    TurnRecordKind::UserTurn | TurnRecordKind::TurnCheckpoint
                )
            })
            .flat_map(|turn| turn.messages.iter())
    }

    fn build_turn_context(&self) -> Arc<Vec<ChatMessage>> {
        if let Some(checkpoint_index) = self.turns.iter().rposition(|turn| {
            matches!(
                turn.kind,
                TurnRecordKind::Checkpoint | TurnRecordKind::TurnCheckpoint
            )
        }) {
            let checkpoint = &self.turns[checkpoint_index];
            let mut context_messages = checkpoint.messages.clone();
            context_messages.extend(
                self.turns
                    .iter()
                    .skip(checkpoint_index + 1)
                    .filter(|turn| matches!(turn.kind, TurnRecordKind::UserTurn))
                    .flat_map(|turn| turn.messages.iter().cloned()),
            );
            Arc::new(context_messages)
        } else {
            Arc::new(crate::history::flatten_turn_messages(&self.turns))
        }
    }

    /// Get current token count.
    pub fn token_count(&self) -> u32 {
        self.turns
            .last()
            .map(|turn| turn.token_usage.total_tokens)
            .unwrap_or(0)
    }

    /// Get turn count.
    pub fn turn_count(&self) -> u32 {
        self.turns
            .iter()
            .filter(|turn| {
                matches!(
                    turn.kind,
                    TurnRecordKind::UserTurn | TurnRecordKind::TurnCheckpoint
                )
            })
            .map(|turn| turn.turn_number)
            .max()
            .unwrap_or(0)
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
        self.publish_runtime_snapshot();
    }

    /// Replace the runtime MCP tool resolver for subsequent turns.
    pub fn set_mcp_tool_resolver(&mut self, resolver: Option<Arc<dyn McpToolResolver>>) {
        self.mcp_tool_resolver = resolver;
    }

    pub fn hydrate_from_turn_log_state(
        &mut self,
        recovered: RecoveredThreadLogState,
        updated_at: DateTime<Utc>,
    ) {
        self.turns = recovered.turns;
        self.active_turn_cancellation = None;
        self.runtime_state = ThreadRuntimeState::Idle;
        self.updated_at = updated_at;
        self.publish_runtime_snapshot();
    }

    /// Append a checkpoint record to the turn history.
    /// Checkpoints do not consume turn numbers.
    pub fn append_checkpoint_record(
        &mut self,
        summary_messages: Vec<ChatMessage>,
        token_usage: TokenUsage,
    ) {
        self.turns
            .push(TurnRecord::checkpoint(summary_messages, token_usage));
        self.updated_at = Utc::now();
        self.publish_runtime_snapshot();
    }

    fn emit_mcp_notice(&self, message: String) {
        self.broadcast_to_self(ThreadEvent::Notice {
            thread_id: self.id.to_string(),
            level: ThreadNoticeLevel::Warning,
            message,
        });
    }

    async fn resolve_mcp_tools(
        &self,
        agent_record: &Arc<AgentRecord>,
    ) -> Result<Vec<Arc<dyn NamedTool>>, ThreadError> {
        let Some(resolver) = self.mcp_tool_resolver.as_ref() else {
            return Ok(Vec::new());
        };

        let resolved = resolver
            .resolve_for_agent(agent_record.id)
            .await
            .map_err(|error| ThreadError::McpToolResolutionFailed {
                reason: error.to_string(),
            })?;

        for unavailable in &resolved.unavailable_servers {
            self.emit_mcp_notice(format!(
                "MCP server '{}' is unavailable for this turn: {}",
                unavailable.display_name, unavailable.reason
            ));
        }

        Ok(resolved.tools)
    }

    pub fn trace_base_dir(&self) -> Option<std::path::PathBuf> {
        self.config
            .turn_config
            .trace_config
            .as_ref()
            .filter(|config| config.enabled)
            .map(|config| config.thread_base_dir.clone())
    }

    async fn persist_trace_turns(
        base_dir: &std::path::Path,
        turns: &[TurnRecord],
    ) -> Result<(), TurnLogError> {
        let recovered = recover_thread_log_state(base_dir).await?;
        if recovered.turns.len() > turns.len() {
            return Err(TurnLogError::MalformedEvent {
                line: 0,
                reason: "persisted trace is longer than in-memory turn history".to_string(),
            });
        }

        for (index, persisted) in recovered.turns.iter().enumerate() {
            let expected = turns
                .get(index)
                .ok_or_else(|| TurnLogError::MalformedEvent {
                    line: 0,
                    reason: "persisted trace diverged from in-memory turn history".to_string(),
                })?;
            let persisted_json =
                serde_json::to_string(persisted).map_err(|error| TurnLogError::MalformedEvent {
                    line: 0,
                    reason: format!("failed to serialize persisted turn record: {error}"),
                })?;
            let expected_json =
                serde_json::to_string(expected).map_err(|error| TurnLogError::MalformedEvent {
                    line: 0,
                    reason: format!("failed to serialize in-memory turn record: {error}"),
                })?;
            if persisted_json != expected_json {
                return Err(TurnLogError::MalformedEvent {
                    line: 0,
                    reason: "persisted trace diverged from in-memory turn history".to_string(),
                });
            }
        }

        for record in turns.iter().skip(recovered.turns.len()) {
            append_turn_record(base_dir, record).await?;
        }

        Ok(())
    }

    fn handle_owner_command(&mut self, command: ThreadOwnerCommand) {
        match command {
            ThreadOwnerCommand::SetTitle { title, ack } => {
                self.set_title(title);
                let _ = ack.send(());
            }
            ThreadOwnerCommand::SetProvider { provider, ack } => {
                self.set_provider(provider);
                let _ = ack.send(());
            }
            ThreadOwnerCommand::SetMcpToolResolver { resolver, ack } => {
                self.set_mcp_tool_resolver(resolver);
                let _ = ack.send(());
            }
        }
    }

    async fn run_reactor_loop(
        mut thread: Self,
        mut message_rx: mpsc::UnboundedReceiver<ThreadMessage>,
        mut owner_rx: mpsc::UnboundedReceiver<ThreadOwnerCommand>,
    ) {
        let mut active_turn: Option<JoinHandle<Result<TurnRecord, ThreadError>>> = None;
        let mut shutdown_requested = false;

        loop {
            tokio::select! {
                Some(message) = message_rx.recv() => {
                    if shutdown_requested {
                        continue;
                    }

                    let runtime_action = match message {
                        ThreadMessage::Control(ThreadControlMessage::ShutdownRuntime) => {
                            shutdown_requested = true;
                            match thread.runtime_state {
                                ThreadRuntimeState::Idle => break,
                                _ => thread.cancel_active_turn(),
                            }
                        }
                        other => thread.dispatch_runtime_message(other),
                    };

                    thread.process_loop_action(runtime_action, &mut active_turn).await;
                }
                Some(command) = owner_rx.recv() => {
                    thread.handle_owner_command(command);
                }
                result = async {
                    match active_turn.as_mut() {
                        Some(execution) => Some(execution.await),
                        None => None,
                    }
                }, if active_turn.is_some() => {
                    let result = match result
                        .expect("active turn should exist while awaiting completion")
                    {
                        Ok(result) => result,
                        Err(error) => Err(Self::map_turn_join_error(error)),
                    };
                    active_turn = None;

                    thread
                        .settle_active_turn(result, &mut active_turn, shutdown_requested)
                        .await;

                    if shutdown_requested && active_turn.is_none() {
                        break;
                    }
                }
                else => break,
            }
        }
    }

    async fn process_loop_action(
        &mut self,
        action: ThreadLoopAction,
        active_turn: &mut Option<JoinHandle<Result<TurnRecord, ThreadError>>>,
    ) {
        match action {
            ThreadLoopAction::StartTurn {
                turn_number,
                content,
                msg_override,
            } => {
                *active_turn = Some(
                    self.start_turn_execution(turn_number, content, msg_override)
                        .await,
                );
            }
            ThreadLoopAction::StopTurn { turn_number } => {
                let cancellation = self.active_turn_cancellation.clone();
                if let Some(cancellation) = cancellation {
                    tracing::info!(turn_number, "cancelling active turn");
                    cancellation.cancel();
                } else {
                    tracing::warn!(turn_number, "stop-turn requested but no active turn handle");
                }
            }
            ThreadLoopAction::Noop => {}
        }
    }

    async fn start_turn_execution(
        &mut self,
        turn_number: u32,
        content: String,
        msg_override: Option<MessageOverride>,
    ) -> JoinHandle<Result<TurnRecord, ThreadError>> {
        let cancellation = TurnCancellation::new();
        let (
            thread_id,
            originating_thread_id,
            history,
            tools,
            hooks,
            provider,
            config,
            agent_record,
            thread_event_tx,
            compactor,
        ) = {
            let effective_record = self
                .prepare_turn_start(msg_override, cancellation.clone())
                .await;
            let mcp_tools = self
                .resolve_mcp_tools(&effective_record)
                .await
                .unwrap_or_default();
            let (thread_id, history, tools, hooks, provider, config, thread_event_tx, compactor) =
                self.build_turn_execution_parts(effective_record.clone(), mcp_tools);

            (
                thread_id,
                self.id,
                history,
                tools,
                hooks,
                provider,
                config,
                effective_record,
                thread_event_tx,
                compactor,
            )
        };

        tokio::spawn(async move {
            turn::execute_thread_turn(
                turn_number,
                thread_id,
                originating_thread_id,
                history,
                vec![ChatMessage::user(content)],
                tools,
                hooks,
                Utc::now(),
                provider,
                config,
                agent_record,
                None,
                thread_event_tx,
                cancellation,
                Some(compactor),
            )
            .await
            .map_err(ThreadError::TurnFailed)
        })
    }

    fn map_turn_join_error(error: JoinError) -> ThreadError {
        ThreadError::TurnFailed(crate::TurnError::BuildFailed(format!(
            "turn setup task failed: {error}"
        )))
    }

    fn broadcast_turn_terminal_event(
        &self,
        turn_number: u32,
        result: &Result<TurnRecord, ThreadError>,
    ) {
        match result {
            Ok(record) => {
                self.broadcast_to_self(ThreadEvent::TurnCompleted {
                    thread_id: self.id.to_string(),
                    turn_number,
                    token_usage: record.token_usage.clone(),
                });
            }
            Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)) => {}
            Err(error) => {
                self.broadcast_to_self(ThreadEvent::TurnFailed {
                    thread_id: self.id.to_string(),
                    turn_number,
                    error: error.to_string(),
                });
            }
        }
    }

    async fn persist_committed_turns_if_needed(&self) {
        if let Some(base_dir) = self.trace_base_dir()
            && let Err(error) = Self::persist_trace_turns(&base_dir, &self.turns).await
        {
            tracing::warn!(
                error = %error,
                "failed to persist committed turn records"
            );
        }
    }

    async fn settle_active_turn(
        &mut self,
        result: Result<TurnRecord, ThreadError>,
        active_turn: &mut Option<JoinHandle<Result<TurnRecord, ThreadError>>>,
        shutdown_requested: bool,
    ) {
        let committed = result.is_ok();
        let settled_turn_number = match self.runtime_state {
            ThreadRuntimeState::Running { turn_number }
            | ThreadRuntimeState::Stopping { turn_number } => Some(turn_number),
            ThreadRuntimeState::Idle => None,
        };
        let settled_turn_number = settled_turn_number.unwrap_or_default();
        let thread_id = self.id().inner().to_string();

        self.broadcast_turn_terminal_event(settled_turn_number, &result);

        self.active_turn_cancellation = None;
        let finish_result = self.finish_turn(result);

        if let Err(error) = finish_result {
            tracing::error!("turn failed: {}", error);
        }

        if committed {
            self.persist_committed_turns_if_needed().await;
        }

        self.broadcast_to_self(ThreadEvent::TurnSettled {
            thread_id: thread_id.clone(),
            turn_number: settled_turn_number,
        });

        if shutdown_requested {
            self.reset_runtime_loop_state();
            self.broadcast_to_self(ThreadEvent::Idle { thread_id });
            return;
        }

        let runtime_action = self.complete_runtime_turn(committed);
        self.process_loop_action(runtime_action.clone(), active_turn)
            .await;

        if matches!(runtime_action, ThreadLoopAction::Noop) && active_turn.is_none() {
            self.broadcast_to_self(ThreadEvent::Idle { thread_id });
        }
    }

    /// Execute one user turn through the thread-owned turn lifecycle.
    pub async fn execute_turn(
        &mut self,
        user_input: String,
        msg_override: Option<MessageOverride>,
        cancellation: TurnCancellation,
    ) -> Result<TurnRecord, ThreadError> {
        if self.message_rx.is_none() {
            return Err(ThreadError::RuntimeActive);
        }

        let turn_number = derive_next_user_turn_number(&self.turns);
        let thread_id_for_events = self.id.to_string();
        let originating_thread_id = self.id;
        let effective_record = self
            .prepare_turn_start(msg_override, cancellation.clone())
            .await;
        let mcp_tools = self
            .resolve_mcp_tools(&effective_record)
            .await
            .unwrap_or_default();
        let (thread_id, history, tools, hooks, provider, config, thread_event_tx, compactor) =
            self.build_turn_execution_parts(effective_record.clone(), mcp_tools);

        let result = match tokio::spawn(async move {
            turn::execute_thread_turn(
                turn_number,
                thread_id,
                originating_thread_id,
                history,
                vec![ChatMessage::user(user_input)],
                tools,
                hooks,
                Utc::now(),
                provider,
                config,
                effective_record,
                None,
                thread_event_tx,
                cancellation,
                Some(compactor),
            )
            .await
            .map_err(ThreadError::TurnFailed)
        })
        .await
        {
            Ok(result) => result,
            Err(error) => Err(Self::map_turn_join_error(error)),
        };

        self.broadcast_turn_terminal_event(turn_number, &result);

        match result {
            Ok(record) => {
                self.finish_turn(Ok(record.clone()))?;
                self.persist_committed_turns_if_needed().await;
                self.broadcast_to_self(ThreadEvent::TurnSettled {
                    thread_id: thread_id_for_events.clone(),
                    turn_number,
                });
                self.broadcast_to_self(ThreadEvent::Idle {
                    thread_id: thread_id_for_events.clone(),
                });
                Ok(record)
            }
            Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)) => {
                self.finish_turn(Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)))?;
                self.broadcast_to_self(ThreadEvent::TurnSettled {
                    thread_id: thread_id_for_events.clone(),
                    turn_number,
                });
                self.broadcast_to_self(ThreadEvent::Idle {
                    thread_id: thread_id_for_events.clone(),
                });
                Err(ThreadError::TurnFailed(crate::TurnError::Cancelled))
            }
            Err(error) => match self.finish_turn(Err(error)) {
                Ok(()) => unreachable!("only cancelled turns should settle without committing"),
                Err(error) => {
                    self.broadcast_to_self(ThreadEvent::TurnSettled {
                        thread_id: thread_id_for_events.clone(),
                        turn_number,
                    });
                    self.broadcast_to_self(ThreadEvent::Idle {
                        thread_id: thread_id_for_events,
                    });
                    Err(error)
                }
            },
        }
    }

    async fn prepare_turn_start(
        &mut self,
        msg_override: Option<MessageOverride>,
        cancellation: TurnCancellation,
    ) -> Arc<AgentRecord> {
        let turn_context = self.build_turn_context();
        match self
            .compactor
            .compact(turn_context.as_slice(), self.token_count())
            .await
        {
            Ok(Some(result)) => {
                let new_token_count = result.token_usage.total_tokens;
                self.append_checkpoint_record(result.messages, result.token_usage.clone());
                if let Some(base_dir) = self.trace_base_dir()
                    && let Err(error) = Self::persist_trace_turns(&base_dir, &self.turns).await
                {
                    tracing::warn!(error = %error, "failed to persist checkpoint turn records");
                }
                self.broadcast_to_self(ThreadEvent::Compacted {
                    thread_id: self.id.to_string(),
                    new_token_count,
                });
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("Compact failed: {}", e);
            }
        }

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

        self.active_turn_cancellation = Some(cancellation);
        self.publish_runtime_snapshot();
        effective_record
    }

    /// Finish a previously started turn and apply its output to thread state.
    pub(crate) fn finish_turn(
        &mut self,
        result: Result<TurnRecord, ThreadError>,
    ) -> Result<(), ThreadError> {
        self.active_turn_cancellation = None;

        match result {
            Ok(record) => {
                self.turns.push(record);
                self.updated_at = Utc::now();
                self.publish_runtime_snapshot();
                Ok(())
            }
            Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)) => {
                self.publish_runtime_snapshot();
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn build_turn_execution_parts(
        &self,
        agent_record: Arc<AgentRecord>,
        mcp_tools: Vec<Arc<dyn NamedTool>>,
    ) -> TurnExecutionParts {
        let provider = self.provider.clone();
        (
            self.id.to_string(),
            self.build_turn_context(),
            Arc::new(self.build_shared_turn_tools(agent_record.as_ref(), mcp_tools)),
            Arc::new(self.build_shared_turn_hooks()),
            provider.clone(),
            self.config.turn_config.clone(),
            self.pipe_tx.clone(),
            Arc::new(LlmTurnCompactor::new(provider)) as Arc<dyn Compactor>,
        )
    }

    fn build_shared_turn_tools(
        &self,
        agent_record: &AgentRecord,
        mcp_tools: Vec<Arc<dyn NamedTool>>,
    ) -> Vec<Arc<dyn NamedTool>> {
        let mut enabled_tool_names = agent_record
            .tool_names
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        if !agent_record.subagent_names.is_empty() {
            enabled_tool_names.insert("scheduler");
        }
        enabled_tool_names.insert("sleep");
        let mut tools = self
            .tool_manager
            .list_ids()
            .iter()
            .filter(|name| enabled_tool_names.contains(name.as_str()))
            .filter_map(|name| self.tool_manager.get(name))
            .collect::<Vec<_>>();
        let plan_tool: Arc<dyn NamedTool> =
            Arc::new(UpdatePlanTool::new(Arc::new(self.plan_store.clone())));
        tools.push(plan_tool);
        tools.extend(mcp_tools);
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

    #[cfg(test)]
    fn hydrate_turn_history_for_test(&mut self, turns: Vec<TurnRecord>) {
        self.turns = turns;
        self.runtime_state = ThreadRuntimeState::Idle;
        self.active_turn_cancellation = None;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::compact::{CompactResult, Compactor};
    use crate::config::{ThreadConfig, TurnConfigBuilder};
    use crate::error::CompactError;
    use crate::thread_trace_store::chat_thread_base_dir;
    use crate::trace::TraceConfig;
    use crate::turn_log_store::recover_thread_log_state;
    use argus_protocol::llm::{CompletionRequest, CompletionResponse, LlmError, ToolDefinition};
    use argus_protocol::tool::{NamedTool, ToolError, ToolExecutionContext};
    use argus_protocol::{
        AgentId, MailboxMessage, MailboxMessageType, ProviderId, ThreadControlMessage,
        ThreadMessage,
    };
    use async_trait::async_trait;
    use futures_util::stream;
    use rust_decimal::Decimal;
    use tokio::time::{Duration, sleep, timeout};

    fn usage(total_tokens: u32) -> TokenUsage {
        TokenUsage {
            input_tokens: total_tokens.saturating_sub(1),
            output_tokens: 1,
            total_tokens,
        }
    }

    struct DummyProvider;

    struct StubTool {
        name: &'static str,
    }

    #[async_trait]
    impl NamedTool for StubTool {
        fn name(&self) -> &str {
            self.name
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: self.name.to_string(),
                description: format!("stub {}", self.name),
                parameters: serde_json::json!({ "type": "object" }),
            }
        }

        async fn execute(
            &self,
            _input: serde_json::Value,
            _ctx: Arc<ToolExecutionContext>,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({}))
        }
    }

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

    struct NoopCompactor;

    #[async_trait]
    impl Compactor for NoopCompactor {
        async fn compact(
            &self,
            _messages: &[ChatMessage],
            _token_count: u32,
        ) -> Result<Option<CompactResult>, CompactError> {
            Ok(None)
        }

        fn name(&self) -> &'static str {
            "noop"
        }
    }

    #[derive(Debug)]
    struct PendingStreamProvider {
        captured_user_inputs: Arc<Mutex<Vec<String>>>,
    }

    impl PendingStreamProvider {
        fn new(captured_user_inputs: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                captured_user_inputs,
            }
        }

        fn capture_request(&self, request: &CompletionRequest) {
            let last_user_input = request
                .messages
                .iter()
                .rev()
                .find(|message| message.role == argus_protocol::llm::Role::User)
                .map(|message| message.content.clone())
                .unwrap_or_default();
            self.captured_user_inputs
                .lock()
                .unwrap()
                .push(last_user_input);
        }
    }

    #[async_trait]
    impl LlmProvider for PendingStreamProvider {
        fn model_name(&self) -> &str {
            "pending-stream"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            Err(LlmError::UnsupportedCapability {
                provider: "pending-stream".to_string(),
                capability: "complete".to_string(),
            })
        }

        async fn stream_complete(
            &self,
            request: CompletionRequest,
        ) -> Result<argus_protocol::llm::LlmEventStream, LlmError> {
            self.capture_request(&request);
            Ok(Box::pin(stream::pending()))
        }
    }

    #[derive(Debug)]
    struct RecordingCompactor {
        seen_token_counts: Arc<Mutex<Vec<u32>>>,
        next_result: Arc<Mutex<VecDeque<CompactResult>>>,
    }

    #[async_trait]
    impl Compactor for RecordingCompactor {
        async fn compact(
            &self,
            _messages: &[ChatMessage],
            token_count: u32,
        ) -> Result<Option<CompactResult>, CompactError> {
            self.seen_token_counts.lock().unwrap().push(token_count);
            Ok(self.next_result.lock().unwrap().pop_front())
        }

        fn name(&self) -> &'static str {
            "recording"
        }
    }

    fn test_agent_record() -> Arc<AgentRecord> {
        Arc::new(AgentRecord {
            id: AgentId::new(1),
            display_name: "test-agent".to_string(),
            description: "A test agent".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(ProviderId::new(1)),
            model_id: None,
            system_prompt: "You are a test agent.".to_string(),
            tool_names: vec![],
            subagent_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: None,
        })
    }

    fn test_agent_record_without_system_prompt() -> Arc<AgentRecord> {
        Arc::new(AgentRecord {
            system_prompt: String::new(),
            ..(*test_agent_record()).clone()
        })
    }

    fn build_test_thread() -> Thread {
        ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build")
    }

    fn build_test_thread_without_system_prompt() -> Thread {
        ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build")
    }

    fn plain_mailbox_message(text: &str) -> MailboxMessage {
        MailboxMessage {
            id: format!("msg-{text}"),
            from_thread_id: ThreadId::new(),
            to_thread_id: ThreadId::new(),
            from_label: "Peer".to_string(),
            message_type: MailboxMessageType::Plain,
            text: text.to_string(),
            timestamp: "2026-04-01T00:00:00Z".to_string(),
            read: false,
            summary: Some(format!("summary {text}")),
        }
    }

    fn job_result_mailbox_message(job_id: &str, text: &str) -> MailboxMessage {
        MailboxMessage {
            id: format!("job-msg-{job_id}"),
            from_thread_id: ThreadId::new(),
            to_thread_id: ThreadId::new(),
            from_label: "Worker".to_string(),
            message_type: MailboxMessageType::JobResult {
                job_id: job_id.to_string(),
                success: true,
                cancelled: false,
                token_usage: None,
                agent_id: AgentId::new(7),
                agent_display_name: "Worker".to_string(),
                agent_description: "Background worker".to_string(),
            },
            text: text.to_string(),
            timestamp: "2026-04-01T00:00:00Z".to_string(),
            read: false,
            summary: Some(format!("summary {job_id}")),
        }
    }

    #[test]
    fn build_shared_turn_tools_includes_scheduler_for_dispatch_capable_agents() {
        let tool_manager = Arc::new(ToolManager::new());
        tool_manager.register(Arc::new(StubTool { name: "scheduler" }));

        let thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(Arc::new(NoopCompactor))
            .tool_manager(Arc::clone(&tool_manager))
            .agent_record(Arc::new(AgentRecord {
                subagent_names: vec!["Researcher".to_string()],
                ..(*test_agent_record()).clone()
            }))
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");

        let tools = thread.build_shared_turn_tools(thread.agent_record.as_ref(), Vec::new());
        let tool_names: Vec<_> = tools.iter().map(|tool| tool.name().to_string()).collect();

        assert!(
            tool_names.iter().any(|name| name == "scheduler"),
            "dispatch-capable agents should automatically receive scheduler"
        );
    }

    #[test]
    fn build_shared_turn_tools_includes_sleep_by_default() {
        let tool_manager = Arc::new(ToolManager::new());
        tool_manager.register(Arc::new(StubTool { name: "sleep" }));

        let thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(Arc::new(NoopCompactor))
            .tool_manager(Arc::clone(&tool_manager))
            .agent_record(Arc::new(AgentRecord {
                tool_names: vec![],
                ..(*test_agent_record()).clone()
            }))
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");

        let tools = thread.build_shared_turn_tools(thread.agent_record.as_ref(), Vec::new());
        let tool_names: Vec<_> = tools.iter().map(|tool| tool.name().to_string()).collect();

        assert!(
            tool_names.iter().any(|name| name == "sleep"),
            "sleep should be available without explicit agent tool_names selection"
        );
    }

    #[test]
    fn build_shared_turn_tools_deduplicates_explicit_sleep_selection() {
        let tool_manager = Arc::new(ToolManager::new());
        tool_manager.register(Arc::new(StubTool { name: "sleep" }));

        let thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(Arc::new(NoopCompactor))
            .tool_manager(Arc::clone(&tool_manager))
            .agent_record(Arc::new(AgentRecord {
                tool_names: vec!["sleep".to_string()],
                ..(*test_agent_record()).clone()
            }))
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");

        let tools = thread.build_shared_turn_tools(thread.agent_record.as_ref(), Vec::new());
        let sleep_count = tools.iter().filter(|tool| tool.name() == "sleep").count();

        assert_eq!(
            sleep_count, 1,
            "sleep should only appear once even if selected explicitly"
        );
    }

    #[test]
    fn history_iter_includes_turn_checkpoint_messages() {
        let mut thread = build_test_thread_without_system_prompt();
        thread.hydrate_turn_history_for_test(vec![
            TurnRecord::user_turn(
                1,
                vec![ChatMessage::user("u1"), ChatMessage::assistant("a1")],
                usage(2),
            ),
            TurnRecord::checkpoint(vec![ChatMessage::assistant("summary")], usage(7)),
            TurnRecord::turn_checkpoint(
                2,
                vec![
                    ChatMessage::user("snapshot"),
                    ChatMessage::assistant("state"),
                ],
                usage(4),
            ),
            TurnRecord::user_turn(
                3,
                vec![ChatMessage::user("u2"), ChatMessage::assistant("a2")],
                usage(5),
            ),
        ]);

        let history: Vec<_> = thread.history_iter().map(|m| m.content.clone()).collect();
        assert_eq!(history, vec!["u1", "a1", "snapshot", "state", "u2", "a2"]);
        assert_eq!(thread.turn_count(), 3);
        assert_eq!(thread.token_count(), 5);
    }

    #[test]
    fn build_turn_context_uses_latest_checkpoint_plus_following_turns() {
        let mut thread = build_test_thread_without_system_prompt();
        thread.hydrate_turn_history_for_test(vec![
            TurnRecord::user_turn(
                1,
                vec![ChatMessage::user("u1"), ChatMessage::assistant("a1")],
                usage(3),
            ),
            TurnRecord::checkpoint(vec![ChatMessage::assistant("summary one")], usage(9)),
            TurnRecord::user_turn(
                2,
                vec![ChatMessage::user("u2"), ChatMessage::assistant("a2")],
                usage(5),
            ),
        ]);

        let context: Vec<_> = thread
            .build_turn_context()
            .iter()
            .map(|message| message.content.clone())
            .collect();
        assert_eq!(context, vec!["summary one", "u2", "a2"]);
    }

    #[tokio::test]
    async fn finish_turn_commits_user_turn_record_history_and_counts() {
        let mut thread = build_test_thread();
        let _agent = thread
            .prepare_turn_start(None, TurnCancellation::new())
            .await;
        let record = TurnRecord::user_turn(
            1,
            vec![ChatMessage::user("hello"), ChatMessage::assistant("world")],
            usage(6),
        );

        thread
            .finish_turn(Ok(record.clone()))
            .expect("turn should settle");

        let committed = thread.turns.first().expect("record should be committed");
        assert!(matches!(committed.kind, TurnRecordKind::UserTurn));
        assert_eq!(committed.turn_number, 1);
        let stored_messages: Vec<_> = committed
            .messages
            .iter()
            .map(|message| (message.role, message.content.clone()))
            .collect();
        let expected_messages: Vec<_> = record
            .messages
            .iter()
            .map(|message| (message.role, message.content.clone()))
            .collect();
        assert_eq!(stored_messages, expected_messages);
        let history_messages: Vec<_> = thread
            .history_iter()
            .map(|message| (message.role, message.content.clone()))
            .collect();
        assert_eq!(history_messages, expected_messages);
        assert_eq!(thread.turn_count(), 1);
        assert_eq!(thread.token_count(), 6);
    }

    #[tokio::test]
    async fn finish_turn_without_system_prompt_keeps_record_messages_unchanged() {
        let mut thread = build_test_thread_without_system_prompt();
        let _agent = thread
            .prepare_turn_start(None, TurnCancellation::new())
            .await;
        let record = TurnRecord::user_turn(
            1,
            vec![ChatMessage::user("hello"), ChatMessage::assistant("world")],
            usage(5),
        );

        thread
            .finish_turn(Ok(record.clone()))
            .expect("turn should settle");

        assert_eq!(thread.turns.len(), 1);
        let stored_messages: Vec<_> = thread.turns[0]
            .messages
            .iter()
            .map(|message| (message.role, message.content.clone()))
            .collect();
        let expected_messages: Vec<_> = record
            .messages
            .iter()
            .map(|message| (message.role, message.content.clone()))
            .collect();
        assert_eq!(stored_messages, expected_messages);
    }

    #[tokio::test]
    async fn cancelled_turn_does_not_append_record() {
        let mut thread = build_test_thread_without_system_prompt();
        let _agent = thread
            .prepare_turn_start(None, TurnCancellation::new())
            .await;

        let result = thread.finish_turn(Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)));
        assert!(result.is_ok());
        assert!(thread.history_iter().next().is_none());
        assert_eq!(thread.turn_count(), 0);
        assert_eq!(thread.token_count(), 0);
        assert!(thread.turns.is_empty());
        assert!(thread.active_turn_cancellation.is_none());
    }

    #[tokio::test]
    async fn finish_turn_commits_turn_checkpoint_history_and_counts() {
        let mut thread = build_test_thread_without_system_prompt();
        let _agent = thread
            .prepare_turn_start(None, TurnCancellation::new())
            .await;
        let checkpoint = TurnRecord::turn_checkpoint(
            1,
            vec![
                ChatMessage::user("summary"),
                ChatMessage::assistant("world"),
            ],
            usage(4),
        );

        thread
            .finish_turn(Ok(checkpoint.clone()))
            .expect("turn should settle");

        let committed = thread.turns.first().expect("record should be committed");
        assert!(matches!(committed.kind, TurnRecordKind::TurnCheckpoint));
        assert_eq!(committed.turn_number, 1);
        assert_eq!(
            committed
                .messages
                .iter()
                .map(|message| message.content.clone())
                .collect::<Vec<_>>(),
            checkpoint
                .messages
                .iter()
                .map(|message| message.content.clone())
                .collect::<Vec<_>>()
        );
        let history_messages: Vec<_> = thread
            .history_iter()
            .map(|message| message.content.clone())
            .collect();
        assert_eq!(
            history_messages,
            vec!["summary".to_string(), "world".to_string()]
        );
        assert_eq!(thread.turn_count(), 1);
        assert_eq!(thread.token_count(), 4);
    }

    #[tokio::test]
    async fn failed_turn_does_not_append_record() {
        let mut thread = build_test_thread_without_system_prompt();
        let _agent = thread
            .prepare_turn_start(None, TurnCancellation::new())
            .await;

        let result = thread.finish_turn(Err(ThreadError::TurnFailed(
            crate::TurnError::ToolExecutionFailed {
                name: "search".to_string(),
                reason: "boom".to_string(),
            },
        )));

        assert!(matches!(result, Err(ThreadError::TurnFailed(_))));
        assert!(thread.turns.is_empty());
        assert!(thread.active_turn_cancellation.is_none());
    }

    #[test]
    fn hydrate_from_turn_log_state_restores_turns_and_context() {
        let updated_at = Utc::now();
        let recovered = RecoveredThreadLogState {
            turns: vec![
                TurnRecord::user_turn(
                    1,
                    vec![
                        ChatMessage::system("Persisted system prompt"),
                        ChatMessage::user("turn one"),
                        ChatMessage::assistant("answer one"),
                    ],
                    usage(10),
                ),
                TurnRecord::checkpoint(
                    vec![ChatMessage::assistant("summary of turn one")],
                    usage(12),
                ),
                TurnRecord::user_turn(
                    2,
                    vec![
                        ChatMessage::user("turn two"),
                        ChatMessage::assistant("answer two"),
                    ],
                    usage(22),
                ),
            ],
        };

        let mut thread = build_test_thread_without_system_prompt();
        thread.hydrate_from_turn_log_state(recovered, updated_at);

        assert_eq!(thread.turn_count(), 2);
        assert_eq!(thread.token_count(), 22);
        let context: Vec<_> = thread
            .build_turn_context()
            .iter()
            .map(|message| message.content.clone())
            .collect();
        assert_eq!(
            context,
            vec!["summary of turn one", "turn two", "answer two"]
        );
        assert_eq!(thread.updated_at(), updated_at);
    }

    #[tokio::test]
    async fn prepare_turn_start_uses_last_record_usage_for_compaction_and_persists_checkpoint() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let trace_config = TraceConfig::new(
            true,
            chat_thread_base_dir(temp_dir.path(), session_id, thread_id),
        );
        let seen_token_counts = Arc::new(Mutex::new(Vec::new()));
        let compactor = RecordingCompactor {
            seen_token_counts: Arc::clone(&seen_token_counts),
            next_result: Arc::new(Mutex::new(VecDeque::from(vec![CompactResult {
                messages: vec![ChatMessage::assistant("summary")],
                token_usage: usage(11),
            }]))),
        };
        let mut thread = ThreadBuilder::new()
            .id(thread_id)
            .provider(Arc::new(DummyProvider))
            .compactor(Arc::new(compactor))
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(session_id)
            .config(ThreadConfig {
                turn_config: TurnConfigBuilder::default()
                    .trace_config(trace_config)
                    .build()
                    .expect("thread config should build"),
            })
            .turns(vec![TurnRecord::user_turn(
                1,
                vec![ChatMessage::user("u1"), ChatMessage::assistant("a1")],
                usage(7),
            )])
            .build()
            .expect("thread should build");

        let _agent = thread
            .prepare_turn_start(None, TurnCancellation::new())
            .await;

        assert_eq!(seen_token_counts.lock().unwrap().as_slice(), &[7]);
        assert!(matches!(
            thread.turns.last().map(|r| &r.kind),
            Some(TurnRecordKind::Checkpoint)
        ));

        let persisted = recover_thread_log_state(&chat_thread_base_dir(
            temp_dir.path(),
            session_id,
            thread_id,
        ))
        .await
        .expect("checkpoint should be persisted");
        assert_eq!(persisted.turns.len(), 2);
        assert!(matches!(
            persisted.turns[1].kind,
            TurnRecordKind::Checkpoint
        ));
    }

    #[test]
    fn thread_info_derives_counts_from_turns() {
        let mut thread = build_test_thread_without_system_prompt();
        thread.hydrate_turn_history_for_test(vec![
            TurnRecord::user_turn(
                1,
                vec![ChatMessage::user("u1"), ChatMessage::assistant("a1")],
                usage(3),
            ),
            TurnRecord::checkpoint(vec![ChatMessage::assistant("summary")], usage(8)),
            TurnRecord::user_turn(
                2,
                vec![ChatMessage::user("u2"), ChatMessage::assistant("a2")],
                usage(5),
            ),
        ]);

        let info = thread.info();
        assert_eq!(info.message_count, 4);
        assert_eq!(info.turn_count, 2);
        assert_eq!(info.token_count, 5);
    }

    #[tokio::test]
    async fn thread_reuses_turn_number_after_uncommitted_turn() {
        let mut thread = build_test_thread_without_system_prompt();
        thread.hydrate_turn_history_for_test(vec![
            TurnRecord::user_turn(
                1,
                vec![ChatMessage::user("u1"), ChatMessage::assistant("a1")],
                usage(3),
            ),
            TurnRecord::user_turn(
                2,
                vec![ChatMessage::user("u2"), ChatMessage::assistant("a2")],
                usage(5),
            ),
        ]);
        thread.dispatch_runtime_message(ThreadMessage::UserInput {
            content: "first".to_string(),
            msg_override: None,
        });
        assert_eq!(
            thread.runtime_state,
            ThreadRuntimeState::Running { turn_number: 3 }
        );

        thread.dispatch_runtime_message(ThreadMessage::UserInput {
            content: "second".to_string(),
            msg_override: None,
        });
        assert_eq!(thread.pending_messages.len(), 1);

        let _agent = thread
            .prepare_turn_start(None, TurnCancellation::new())
            .await;
        thread
            .finish_turn(Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)))
            .expect("cancelled turn should settle");
        thread.complete_runtime_turn(false);
        assert_eq!(
            thread.runtime_state,
            ThreadRuntimeState::Running { turn_number: 3 }
        );
        assert_eq!(thread.pending_messages.len(), 0);

        let _agent = thread
            .prepare_turn_start(None, TurnCancellation::new())
            .await;
        thread
            .finish_turn(Ok(TurnRecord::user_turn(
                3,
                vec![ChatMessage::user("second"), ChatMessage::assistant("done")],
                usage(7),
            )))
            .expect("turn should settle");
        thread.complete_runtime_turn(true);
        assert_eq!(thread.runtime_state, ThreadRuntimeState::Idle);
    }

    #[tokio::test]
    async fn thread_runtime_advances_turn_numbers_after_committed_turns() {
        let mut thread = build_test_thread_without_system_prompt();
        thread.dispatch_runtime_message(ThreadMessage::UserInput {
            content: "hi".to_string(),
            msg_override: None,
        });
        assert_eq!(
            thread.runtime_state,
            ThreadRuntimeState::Running { turn_number: 1 }
        );

        let _agent = thread
            .prepare_turn_start(None, TurnCancellation::new())
            .await;
        thread
            .finish_turn(Ok(TurnRecord::user_turn(
                1,
                vec![ChatMessage::user("hi"), ChatMessage::assistant("there")],
                usage(4),
            )))
            .expect("turn should settle");
        thread.complete_runtime_turn(true);
        assert_eq!(thread.runtime_state, ThreadRuntimeState::Idle);

        thread.dispatch_runtime_message(ThreadMessage::UserInput {
            content: "again".to_string(),
            msg_override: None,
        });
        assert_eq!(
            thread.runtime_state,
            ThreadRuntimeState::Running { turn_number: 2 }
        );
    }

    #[test]
    fn peer_message_starts_next_turn_via_thread_message_routing() {
        let mut thread = build_test_thread_without_system_prompt();
        let message = plain_mailbox_message("peer hello");

        let action = thread.dispatch_runtime_message(ThreadMessage::PeerMessage {
            message: message.clone(),
        });

        assert!(matches!(
            action,
            ThreadLoopAction::StartTurn {
                turn_number: 1,
                ref content,
                msg_override: None,
            } if content == &message.into_message_text()
        ));
        assert_eq!(
            thread.runtime_state,
            ThreadRuntimeState::Running { turn_number: 1 }
        );
        assert!(
            thread.pending_messages.is_empty(),
            "idle routing should promote peer message directly into the next turn"
        );
    }

    #[test]
    fn job_result_starts_next_turn_via_thread_message_routing() {
        let mut thread = build_test_thread_without_system_prompt();
        let message = job_result_mailbox_message("job-42", "finished task");

        let action = thread.dispatch_runtime_message(ThreadMessage::JobResult {
            message: message.clone(),
        });

        let expected_content = message.into_message_text();
        assert!(matches!(
            action,
            ThreadLoopAction::StartTurn {
                turn_number: 1,
                ref content,
                msg_override: None,
            } if content == &expected_content
        ));
        assert_eq!(
            thread.runtime_state,
            ThreadRuntimeState::Running { turn_number: 1 }
        );
        assert!(
            thread.pending_messages.is_empty(),
            "idle routing should promote job results directly into the next turn"
        );
    }

    #[tokio::test]
    async fn thread_interrupt_does_not_cancel_next_turn_after_settle() {
        let mut thread = build_test_thread_without_system_prompt();
        thread.dispatch_runtime_message(ThreadMessage::UserInput {
            content: "first".to_string(),
            msg_override: None,
        });
        thread.dispatch_runtime_message(ThreadMessage::UserInput {
            content: "second".to_string(),
            msg_override: None,
        });

        let _agent = thread
            .prepare_turn_start(None, TurnCancellation::new())
            .await;
        let action = thread.dispatch_runtime_message(ThreadMessage::Interrupt);
        assert!(matches!(
            action,
            ThreadLoopAction::StopTurn { turn_number: 1 }
        ));
        thread
            .finish_turn(Ok(TurnRecord::user_turn(
                1,
                vec![ChatMessage::user("first"), ChatMessage::assistant("done")],
                usage(4),
            )))
            .expect("turn should settle");

        thread.complete_runtime_turn(true);
        assert_eq!(
            thread.runtime_state,
            ThreadRuntimeState::Running { turn_number: 2 }
        );
    }

    #[tokio::test]
    async fn shutdown_runtime_control_cancels_active_turn_and_ignores_followup_messages() {
        let captured_user_inputs = Arc::new(Mutex::new(Vec::new()));
        let owner = ThreadBuilder::new()
            .provider(Arc::new(PendingStreamProvider::new(Arc::clone(
                &captured_user_inputs,
            ))))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build")
            .spawn_runtime()
            .expect("runtime handle should spawn");
        let thread = owner.observer();

        thread
            .send_message(ThreadMessage::UserInput {
                content: "first".to_string(),
                msg_override: None,
            })
            .expect("first message should enqueue");

        timeout(Duration::from_secs(5), async {
            loop {
                if thread.is_turn_running() {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runtime should start the first turn");

        owner
            .shutdown()
            .await
            .expect("shutdown control should enqueue");
        thread
            .send_message(ThreadMessage::UserInput {
                content: "second".to_string(),
                msg_override: None,
            })
            .expect_err("observer handle should not enqueue after shutdown");

        sleep(Duration::from_millis(100)).await;

        let captured = captured_user_inputs.lock().unwrap().clone();
        assert_eq!(
            captured,
            vec!["first".to_string()],
            "shutdown should prevent any follow-up turn from starting"
        );
        assert_eq!(thread.turn_count(), 0);
        assert_eq!(thread.state(), ThreadState::Idle);
    }

    #[tokio::test]
    async fn observer_handle_rejects_owner_control_messages() {
        let thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build")
            .spawn_runtime()
            .expect("runtime handle should spawn")
            .observer();

        let error = thread
            .send_message(ThreadMessage::Control(
                ThreadControlMessage::ShutdownRuntime,
            ))
            .expect_err("observer handle must not accept owner-only control messages");
        assert!(matches!(error, ThreadError::OwnerControlRestricted));
    }

    #[tokio::test]
    async fn runtime_handles_distinguish_instances_even_with_same_thread_id() {
        let thread_id = ThreadId::new();
        let first = ThreadBuilder::new()
            .id(thread_id)
            .provider(Arc::new(DummyProvider))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build")
            .spawn_runtime()
            .expect("runtime handle should spawn");
        let first_clone = first.clone();
        let second = ThreadBuilder::new()
            .id(thread_id)
            .provider(Arc::new(DummyProvider))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build")
            .spawn_runtime()
            .expect("runtime handle should spawn");

        assert!(first.same_runtime(&first_clone));
        assert!(!first.same_runtime(&second));
    }

    #[tokio::test]
    async fn turn_join_failure_is_mapped_to_regular_thread_error() {
        let handle: JoinHandle<Result<TurnRecord, ThreadError>> = tokio::spawn(async move {
            panic!("boom");
            #[allow(unreachable_code)]
            Ok(TurnRecord::user_turn(1, Vec::new(), TokenUsage::default()))
        });

        let join_error = handle.await.expect_err("join should fail");
        let error = Thread::map_turn_join_error(join_error);

        match error {
            ThreadError::TurnFailed(crate::TurnError::BuildFailed(message)) => {
                assert!(message.contains("turn setup task failed"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
