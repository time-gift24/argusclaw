//! Thread implementation.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use derive_builder::Builder;
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};

use crate::command::ThreadRuntimeSnapshot;
use crate::turn::{TurnCancellation, TurnExecution, TurnProgress, TurnSharedContext};
use crate::{TurnBuilder, TurnOutput};
use argus_protocol::llm::{ChatMessage, LlmProvider, Role};
use argus_protocol::tool::NamedTool;
use argus_protocol::{
    AgentRecord, HookHandler, HookRegistry, MailboxMessage, MessageOverride, QueuedUserMessage,
    SessionId, ThreadCommand, ThreadControlEvent, ThreadEvent, ThreadId, ThreadMailbox,
    ThreadRuntimeState, TokenUsage,
};
use argus_tool::ToolManager;

use super::compact::Compactor;
use super::config::ThreadConfig;
use super::error::ThreadError;
use super::history::{InFlightTurn, InFlightTurnPhase, InFlightTurnShared, TurnRecord, TurnState};
use super::plan_hook::PlanContinuationHook;
use super::plan_store::FilePlanStore;
use super::plan_tool::UpdatePlanTool;
use super::turn_log_store::{
    RecoveredThreadLogState, TurnLogPersistenceSnapshot, persist_turn_log_snapshot,
};
use super::types::{ThreadInfo, ThreadState};
/// Default broadcast channel capacity.
const DEFAULT_CHANNEL_CAPACITY: usize = 256;

/// Runtime decisions that the thread-owned reactor can emit.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum ThreadReactorAction {
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

/// Lightweight thread-owned reactor state machine.
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct ThreadReactor {
    state: ThreadRuntimeState,
    next_turn_number: u32,
    queue_depth: usize,
}

impl Default for ThreadReactor {
    fn default() -> Self {
        Self {
            state: ThreadRuntimeState::Idle,
            next_turn_number: 1,
            queue_depth: 0,
        }
    }
}

#[allow(dead_code)]
impl ThreadReactor {
    /// Create a new thread reactor seeded from the owning thread's next turn number.
    #[must_use]
    pub(crate) fn seeded_from_next_turn_number(next_turn_number: u32) -> Self {
        Self {
            state: ThreadRuntimeState::Idle,
            next_turn_number,
            queue_depth: 0,
        }
    }

    /// Handle a runtime command and return the immediate action.
    pub(crate) fn apply_command(
        &mut self,
        command: ThreadCommand,
        mailbox: &mut ThreadMailbox,
    ) -> ThreadReactorAction {
        match command {
            ThreadCommand::EnqueueUserMessage {
                content,
                msg_override,
            } => {
                mailbox.push(ThreadControlEvent::UserMessage {
                    content,
                    msg_override,
                });
                self.queue_depth = mailbox.pending_len();
                self.try_start_next_turn(mailbox)
            }
            ThreadCommand::EnqueueMailboxMessage(message) => {
                mailbox.push(ThreadControlEvent::DeliverMailboxMessage(message));
                self.queue_depth = mailbox.pending_len();
                self.try_start_next_turn(mailbox)
            }
            ThreadCommand::CancelActiveTurn => self.cancel_active_turn(),
        }
    }

    /// Mark the current turn as finished and decide the next action.
    pub(crate) fn finish_active_turn(
        &mut self,
        mailbox: &mut ThreadMailbox,
    ) -> ThreadReactorAction {
        self.state = ThreadRuntimeState::Idle;
        self.queue_depth = mailbox.pending_len();
        self.try_start_next_turn(mailbox)
    }

    /// Return an immutable runtime snapshot.
    #[must_use]
    pub(crate) fn snapshot(&self) -> ThreadRuntimeSnapshot {
        ThreadRuntimeSnapshot {
            state: self.state,
            queue_depth: self.queue_depth,
        }
    }

    /// Return current runtime state.
    #[must_use]
    pub(crate) fn state(&self) -> ThreadRuntimeState {
        self.state
    }

    pub(crate) fn claim_queued_job_result(
        &mut self,
        mailbox: &mut ThreadMailbox,
        job_id: &str,
    ) -> Option<MailboxMessage> {
        let claimed = mailbox.claim_job_result(job_id);
        if claimed.is_some() {
            self.queue_depth = mailbox.pending_len();
        }
        claimed
    }

    pub(crate) fn mark_waiting_for_approval(&mut self, turn_number: u32) {
        if matches!(self.state, ThreadRuntimeState::Running { turn_number: active } if active == turn_number)
        {
            self.state = ThreadRuntimeState::WaitingForApproval { turn_number };
        }
    }

    pub(crate) fn mark_running_after_approval(&mut self, turn_number: u32) {
        if matches!(self.state, ThreadRuntimeState::WaitingForApproval { turn_number: active } if active == turn_number)
        {
            self.state = ThreadRuntimeState::Running { turn_number };
        }
    }

    fn try_start_next_turn(&mut self, mailbox: &mut ThreadMailbox) -> ThreadReactorAction {
        if !matches!(self.state, ThreadRuntimeState::Idle) {
            return ThreadReactorAction::Noop;
        }

        match self.take_next_turn_message(mailbox) {
            Some(message) => self.start_turn(message),
            None => ThreadReactorAction::Noop,
        }
    }

    fn start_turn(&mut self, message: QueuedUserMessage) -> ThreadReactorAction {
        let turn_number = self.next_turn_number;
        self.next_turn_number = self.next_turn_number.saturating_add(1);
        self.state = ThreadRuntimeState::Running { turn_number };

        ThreadReactorAction::StartTurn {
            turn_number,
            content: message.content,
            msg_override: message.msg_override,
        }
    }

    fn cancel_active_turn(&mut self) -> ThreadReactorAction {
        match self.state {
            ThreadRuntimeState::Running { turn_number }
            | ThreadRuntimeState::WaitingForApproval { turn_number } => {
                self.state = ThreadRuntimeState::Stopping { turn_number };
                ThreadReactorAction::StopTurn { turn_number }
            }
            ThreadRuntimeState::Idle | ThreadRuntimeState::Stopping { .. } => {
                ThreadReactorAction::Noop
            }
        }
    }

    fn take_next_turn_message(&mut self, mailbox: &mut ThreadMailbox) -> Option<QueuedUserMessage> {
        let message = mailbox.take_next_turn_message();
        self.queue_depth = mailbox.pending_len();
        message
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

    /// Creation timestamp.
    #[builder(default = "Utc::now()")]
    created_at: DateTime<Utc>,

    /// Last update timestamp.
    #[builder(default = "Utc::now()")]
    updated_at: DateTime<Utc>,

    /// Settled turn history — single source of truth.
    #[builder(default)]
    turns: Vec<TurnRecord>,

    /// The single in-flight turn, if any.
    #[builder(default)]
    current_turn: Option<InFlightTurn>,

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

    /// Whether the stored token count predates a compaction and must be refreshed by provider usage.
    #[builder(default)]
    token_count_stale: bool,

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
            .field("turns", &self.turns.len())
            .field("token_count", &self.token_count)
            .field("token_count_stale", &self.token_count_stale)
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
        let mut turns = self.turns.unwrap_or_default();
        let current_turn = self.current_turn.flatten();

        // Seed a SystemBootstrap turn when no turns exist and system_prompt is present.
        let has_bootstrap = turns
            .first()
            .is_some_and(|t| matches!(t.kind, crate::history::TurnRecordKind::SystemBootstrap));
        if !has_bootstrap && !agent_record.system_prompt.is_empty() {
            turns.insert(
                0,
                TurnRecord::system_bootstrap(
                    0,
                    vec![ChatMessage::system(&agent_record.system_prompt)],
                ),
            );
        }

        let next_turn_number = self
            .next_turn_number
            .unwrap_or_else(|| Thread::derive_next_turn_number(&turns));
        Ok(Thread {
            id: self.id.unwrap_or_default(),
            agent_record,
            session_id,
            title: self.title.flatten(),
            created_at: self.created_at.unwrap_or_else(Utc::now),
            updated_at: self.updated_at.unwrap_or_else(Utc::now),
            turns,
            current_turn,
            provider: self.provider.ok_or(ThreadError::ProviderNotConfigured)?,
            tool_manager,
            compactor: self.compactor.ok_or(ThreadError::CompactorNotConfigured)?,
            hooks,
            config: self.config.unwrap_or_default(),
            token_count: 0,
            token_count_stale: false,
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
            message_count: self.history_iter().count(),
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

    /// Returns true when committed history contains visible transcript beyond system prompts.
    pub fn has_non_system_history(&self) -> bool {
        self.history_iter()
            .any(|message| message.role != Role::System)
    }

    /// Iterate over committed message history from turn records.
    pub fn history_iter(&self) -> impl Iterator<Item = &ChatMessage> + '_ {
        self.turns
            .iter()
            .filter(|turn| !matches!(turn.kind, crate::history::TurnRecordKind::Checkpoint { .. }))
            .flat_map(|turn| turn.messages.iter())
    }

    fn build_turn_context(&self) -> Arc<Vec<ChatMessage>> {
        // Find the latest checkpoint by seq
        let latest_checkpoint = self
            .turns
            .iter()
            .filter_map(|turn| {
                if let crate::history::TurnRecordKind::Checkpoint { through_turn } = &turn.kind {
                    Some((turn.seq, *through_turn, turn.messages.as_slice()))
                } else {
                    None
                }
            })
            .max_by_key(|(seq, _, _)| *seq);

        if let Some((_, through_turn, checkpoint_messages)) = latest_checkpoint {
            let mut context_messages: Vec<ChatMessage> = self
                .turns
                .iter()
                .filter(|turn| matches!(turn.kind, crate::history::TurnRecordKind::SystemBootstrap))
                .flat_map(|turn| turn.messages.iter().cloned())
                .collect();
            context_messages.extend(checkpoint_messages.iter().cloned());
            context_messages.extend(
                self.turns
                    .iter()
                    .filter(|turn| turn.turn_number.is_some_and(|tn| tn > through_turn))
                    .flat_map(|turn| turn.messages.iter().cloned()),
            );
            Arc::new(context_messages)
        } else {
            Arc::new(crate::history::flatten_turn_messages(&self.turns))
        }
    }

    /// Get current token count.
    pub fn token_count(&self) -> u32 {
        self.token_count
    }

    /// Get turn count.
    pub fn turn_count(&self) -> u32 {
        self.turn_count.max(
            self.turns
                .iter()
                .filter_map(|turn| turn.turn_number)
                .max()
                .unwrap_or(0),
        )
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

    /// Set the token count (for Compactor).
    pub fn set_token_count(&mut self, count: u32) {
        self.token_count = count;
        self.token_count_stale = false;
    }

    pub fn hydrate_from_turn_log_state(
        &mut self,
        recovered: RecoveredThreadLogState,
        updated_at: DateTime<Utc>,
    ) {
        let token_count = recovered.token_count();
        let turn_count = recovered.turn_count();
        let has_checkpoint = recovered
            .turns
            .iter()
            .any(|t| matches!(t.kind, crate::history::TurnRecordKind::Checkpoint { .. }));

        self.turns = recovered.turns;
        self.current_turn = None;
        self.token_count = token_count;
        self.token_count_stale = has_checkpoint;
        self.turn_count = turn_count;
        self.next_turn_number = Self::derive_next_turn_number(&self.turns);
        self.active_turn_cancellation = None;
        self.runtime_snapshot = ThreadRuntimeSnapshot::default();
        self.updated_at = updated_at;
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
        context_token_count: Option<u32>,
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
            seq: self.turns.len() as u64,
            kind: crate::history::TurnRecordKind::UserTurn,
            turn_number: Some(current_turn.turn_number),
            state,
            messages: committed_messages.clone(),
            token_usage,
            context_token_count,
            started_at: current_turn.started_at,
            finished_at: Some(Utc::now()),
            model: current_turn.model,
            error,
        });
        self.updated_at = Utc::now();

        Ok(())
    }

    /// Append a checkpoint record to the turn history.
    /// Checkpoints do not consume turn numbers.
    pub fn append_checkpoint_record(
        &mut self,
        through_turn: u32,
        summary_messages: Vec<ChatMessage>,
    ) {
        self.turns.push(TurnRecord::checkpoint(
            self.turns.len() as u64,
            through_turn,
            summary_messages,
        ));
    }

    pub(crate) fn turn_log_persistence_snapshot(&self) -> Option<TurnLogPersistenceSnapshot> {
        let trace_config = self
            .config
            .turn_config
            .trace_config
            .as_ref()
            .filter(|config| config.enabled)?;
        let last_turn = self.turns.last()?.clone();
        let mut base_dir = trace_config.trace_dir.clone();
        let session_id = trace_config.session_id.unwrap_or(self.session_id);
        base_dir = base_dir.join(session_id.to_string());
        base_dir = base_dir.join(self.id.to_string());

        // Include SystemBootstrap + latest turn so persist can write both if needed.
        let bootstrap = self
            .turns
            .iter()
            .find(|t| matches!(t.kind, crate::history::TurnRecordKind::SystemBootstrap))
            .cloned();
        let mut turns = Vec::with_capacity(2);
        if let Some(bootstrap) = bootstrap {
            turns.push(bootstrap);
        }
        turns.push(last_turn);

        Some(TurnLogPersistenceSnapshot { base_dir, turns })
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

    /// Spawn the thread-owned reactor loop that coordinates queued control events.
    pub fn spawn_reactor(thread: Arc<RwLock<Self>>) {
        tokio::spawn(async move {
            Self::run_reactor_loop(thread).await;
        });
    }

    async fn run_reactor_loop(thread: Arc<RwLock<Self>>) {
        let (mut control_rx, mailbox, next_turn_number) = {
            let mut guard = thread.write().await;
            let control_rx = match guard.take_control_rx() {
                Some(rx) => rx,
                None => {
                    tracing::warn!("thread control receiver already taken");
                    return;
                }
            };
            (
                control_rx,
                guard.mailbox(),
                guard.next_turn_number_for_runtime(),
            )
        };

        let mut runtime = ThreadReactor::seeded_from_next_turn_number(next_turn_number);
        let mut active_turn: Option<TurnExecution> = None;
        Self::sync_runtime_snapshot_async(&thread, &runtime).await;
        let mut shutdown_requested = false;

        loop {
            tokio::select! {
                Some(control_event) = control_rx.recv() => {
                    if shutdown_requested {
                        if let ThreadControlEvent::ClaimQueuedJobResult { reply_tx, .. } = control_event {
                            let _ = reply_tx.send(None);
                        }
                        continue;
                    }

                    let runtime_action = match control_event {
                        ThreadControlEvent::UserMessage { content, msg_override } => {
                            let mut mailbox = mailbox.lock().await;
                            let action = runtime.apply_command(
                                ThreadCommand::EnqueueUserMessage {
                                    content,
                                    msg_override,
                                },
                                &mut mailbox,
                            );
                            drop(mailbox);
                            Self::sync_runtime_snapshot_async(&thread, &runtime).await;
                            action
                        }
                        ThreadControlEvent::DeliverMailboxMessage(message) => {
                            let mut mailbox = mailbox.lock().await;
                            let action = runtime.apply_command(
                                ThreadCommand::EnqueueMailboxMessage(message),
                                &mut mailbox,
                            );
                            drop(mailbox);
                            Self::sync_runtime_snapshot_async(&thread, &runtime).await;
                            action
                        }
                        ThreadControlEvent::UserInterrupt { content } => {
                            let _ = content;
                            let mut mailbox = mailbox.lock().await;
                            let action =
                                runtime.apply_command(ThreadCommand::CancelActiveTurn, &mut mailbox);
                            drop(mailbox);
                            Self::sync_runtime_snapshot_async(&thread, &runtime).await;
                            action
                        }
                        ThreadControlEvent::ClaimQueuedJobResult { job_id, reply_tx } => {
                            let claimed = {
                                let mut mailbox = mailbox.lock().await;
                                runtime.claim_queued_job_result(&mut mailbox, &job_id)
                            };
                            Self::sync_runtime_snapshot_async(&thread, &runtime).await;
                            let _ = reply_tx.send(claimed);
                            ThreadReactorAction::Noop
                        }
                        ThreadControlEvent::ShutdownRuntime => {
                            shutdown_requested = true;
                            match runtime.state() {
                                ThreadRuntimeState::Idle => break,
                                _ => {
                                    let mut mailbox = mailbox.lock().await;
                                    let action = runtime.apply_command(
                                        ThreadCommand::CancelActiveTurn,
                                        &mut mailbox,
                                    );
                                    drop(mailbox);
                                    Self::sync_runtime_snapshot_async(&thread, &runtime).await;
                                    action
                                }
                            }
                        }
                    };

                    Self::process_reactor_action(
                        Arc::clone(&thread),
                        &mut runtime,
                        runtime_action,
                        &mut active_turn,
                    )
                    .await;
                }
                progress = async {
                    match active_turn.as_mut() {
                        Some(execution) => execution.recv().await,
                        None => None,
                    }
                }, if active_turn.is_some() => {
                    match progress {
                        Some(progress) => {
                            Self::handle_turn_progress(&thread, &mut runtime, &progress).await;
                        }
                        None => {
                            let result = active_turn
                                .take()
                                .expect("active turn should exist while receiving progress")
                                .finish()
                                .await
                                .map_err(ThreadError::TurnFailed);

                            Self::settle_active_turn(
                                &thread,
                                &mut runtime,
                                result,
                                &mut active_turn,
                                shutdown_requested,
                            )
                            .await;

                            if shutdown_requested && active_turn.is_none() {
                                break;
                            }
                        }
                    }
                }
                else => break,
            }
        }
    }

    async fn process_reactor_action(
        thread: Arc<RwLock<Self>>,
        runtime: &mut ThreadReactor,
        action: ThreadReactorAction,
        active_turn: &mut Option<TurnExecution>,
    ) {
        let mut next_action = action;
        loop {
            match next_action {
                ThreadReactorAction::StartTurn {
                    turn_number,
                    content,
                    msg_override,
                } => match Self::start_turn_execution(
                    Arc::clone(&thread),
                    turn_number,
                    content,
                    msg_override,
                )
                .await
                {
                    Ok(execution) => {
                        *active_turn = Some(execution);
                    }
                    Err(error) => {
                        let thread_id = {
                            let guard = thread.read().await;
                            guard.id().inner().to_string()
                        };
                        {
                            let guard = thread.read().await;
                            guard.broadcast_to_self(ThreadEvent::TurnFailed {
                                thread_id: thread_id.clone(),
                                turn_number,
                                error: error.to_string(),
                            });
                        }
                        tracing::error!(
                            turn_number,
                            queue_depth = runtime.snapshot().queue_depth,
                            "failed to start queued turn: {}",
                            error
                        );
                        next_action = Self::finish_failed_start_turn(
                            &thread,
                            runtime,
                            turn_number,
                            &thread_id,
                        )
                        .await;
                        continue;
                    }
                },
                ThreadReactorAction::StopTurn { turn_number } => {
                    let cancellation = {
                        let guard = thread.read().await;
                        guard.active_turn_cancellation()
                    };
                    if let Some(cancellation) = cancellation {
                        tracing::info!(turn_number, "cancelling active turn");
                        cancellation.cancel();
                    } else {
                        tracing::warn!(
                            turn_number,
                            "stop-turn requested but no active turn handle"
                        );
                    }
                }
                ThreadReactorAction::Noop => {}
            }
            break;
        }
    }

    async fn start_turn_execution(
        thread: Arc<RwLock<Self>>,
        turn_number: u32,
        content: String,
        msg_override: Option<MessageOverride>,
    ) -> Result<TurnExecution, ThreadError> {
        let cancellation = TurnCancellation::new();
        let turn = {
            let mut guard = thread.write().await;
            guard
                .begin_turn_with_number(turn_number, content, msg_override, cancellation.clone())
                .await?
        };

        Ok(turn.execute_progress())
    }

    async fn finish_failed_start_turn(
        thread: &Arc<RwLock<Self>>,
        runtime: &mut ThreadReactor,
        turn_number: u32,
        thread_id: &str,
    ) -> ThreadReactorAction {
        let mailbox = {
            let guard = thread.read().await;
            guard.mailbox()
        };
        let mut mailbox = mailbox.lock().await;
        let next_action = runtime.finish_active_turn(&mut mailbox);
        drop(mailbox);
        Self::sync_runtime_snapshot_async(thread, runtime).await;

        {
            let guard = thread.read().await;
            guard.broadcast_to_self(ThreadEvent::TurnSettled {
                thread_id: thread_id.to_string(),
                turn_number,
            });
        }
        if matches!(next_action, ThreadReactorAction::Noop) {
            let guard = thread.read().await;
            guard.broadcast_to_self(ThreadEvent::Idle {
                thread_id: thread_id.to_string(),
            });
        }

        next_action
    }

    async fn handle_turn_progress(
        thread: &Arc<RwLock<Self>>,
        runtime: &mut ThreadReactor,
        progress: &TurnProgress,
    ) {
        match progress {
            TurnProgress::WaitingForApproval { turn_number, .. } => {
                runtime.mark_waiting_for_approval(*turn_number);
                Self::sync_runtime_snapshot_async(thread, runtime).await;
            }
            TurnProgress::ApprovalResolved { turn_number, .. } => {
                runtime.mark_running_after_approval(*turn_number);
                Self::sync_runtime_snapshot_async(thread, runtime).await;
            }
            TurnProgress::LlmEvent(_)
            | TurnProgress::ToolStarted { .. }
            | TurnProgress::ToolCompleted { .. }
            | TurnProgress::Completed(_)
            | TurnProgress::Failed { .. } => {}
        }
    }

    async fn settle_active_turn(
        thread: &Arc<RwLock<Self>>,
        runtime: &mut ThreadReactor,
        result: Result<TurnOutput, ThreadError>,
        active_turn: &mut Option<TurnExecution>,
        shutdown_requested: bool,
    ) {
        let settled_turn_number = match runtime.state() {
            ThreadRuntimeState::Running { turn_number }
            | ThreadRuntimeState::WaitingForApproval { turn_number }
            | ThreadRuntimeState::Stopping { turn_number } => Some(turn_number),
            ThreadRuntimeState::Idle => None,
        };
        let thread_id = {
            let guard = thread.read().await;
            guard.id().inner().to_string()
        };

        {
            let guard = thread.read().await;
            match &result {
                Ok(output) => {
                    guard.broadcast_to_self(ThreadEvent::TurnCompleted {
                        thread_id: thread_id.clone(),
                        turn_number: settled_turn_number.unwrap_or_default(),
                        token_usage: output.token_usage.clone(),
                        context_token_count: output.context_token_count,
                    });
                }
                Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)) => {}
                Err(error) => {
                    guard.broadcast_to_self(ThreadEvent::TurnFailed {
                        thread_id: thread_id.clone(),
                        turn_number: settled_turn_number.unwrap_or_default(),
                        error: error.to_string(),
                    });
                }
            }
        }

        let (finish_result, turn_log_snapshot) = {
            let mut guard = thread.write().await;
            guard.set_active_turn_cancellation(None);
            let finish_result = guard.finish_turn(result);
            let turn_log_snapshot = guard.turn_log_persistence_snapshot();
            (finish_result, turn_log_snapshot)
        };

        if let Err(error) = finish_result {
            tracing::error!("turn failed: {}", error);
        }

        if let Some(snapshot) = turn_log_snapshot
            && let Err(error) = persist_turn_log_snapshot(&snapshot).await
        {
            tracing::warn!(
                turn_number = snapshot.turns.last().and_then(|t| t.turn_number),
                error = %error,
                "failed to persist committed turn log snapshot"
            );
        }

        {
            let mailbox = {
                let guard = thread.read().await;
                guard.mailbox()
            };
            let mut guard = mailbox.lock().await;
            guard.clear_interrupts_for_idle_handoff();
        }

        if let Some(turn_number) = settled_turn_number {
            let guard = thread.read().await;
            guard.broadcast_to_self(ThreadEvent::TurnSettled {
                thread_id: thread_id.clone(),
                turn_number,
            });
        }

        if shutdown_requested {
            return;
        }

        let runtime_action = {
            let mailbox = {
                let guard = thread.read().await;
                guard.mailbox()
            };
            let mut guard = mailbox.lock().await;
            runtime.finish_active_turn(&mut guard)
        };
        Self::sync_runtime_snapshot_async(thread, runtime).await;
        Self::process_reactor_action(
            Arc::clone(thread),
            runtime,
            runtime_action.clone(),
            active_turn,
        )
        .await;

        if matches!(runtime_action, ThreadReactorAction::Noop) && active_turn.is_none() {
            let guard = thread.read().await;
            guard.broadcast_to_self(ThreadEvent::Idle { thread_id });
        }
    }

    async fn sync_runtime_snapshot_async(thread: &Arc<RwLock<Self>>, runtime: &ThreadReactor) {
        let snapshot = runtime.snapshot();
        let mut guard = thread.write().await;
        guard.sync_runtime_snapshot(snapshot);
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
        if !self.token_count_stale {
            match self
                .compactor
                .compact(turn_context.as_slice(), self.token_count)
                .await
            {
                Ok(Some(result)) => {
                    let new_token_count = result.token_count;
                    self.append_checkpoint_record(self.turn_count(), result.summary_messages);
                    self.token_count_stale = false;
                    self.token_count = new_token_count;
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
        } else {
            tracing::debug!("Skipping compaction because thread token count is stale");
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
        self.next_turn_number = self.next_turn_number.max(turn_number.saturating_add(1));
        self.turn_count = self.turn_count.max(turn_number);
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
                if let Some(context_token_count) = output.context_token_count {
                    self.token_count = context_token_count;
                    self.token_count_stale = false;
                } else {
                    self.token_count_stale = true;
                }
                self.settle_current_turn(
                    TurnState::Completed,
                    Some(output.appended_messages),
                    Some(output.token_usage),
                    output.context_token_count,
                    None,
                )?;
                Ok(())
            }
            Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)) => {
                self.settle_current_turn(TurnState::Cancelled, None, None, None, None)?;
                Ok(())
            }
            Err(error) => {
                let error_message = error.to_string();
                self.settle_current_turn(TurnState::Failed, None, None, None, Some(error_message))?;
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

    fn derive_next_turn_number(turns: &[TurnRecord]) -> u32 {
        turns
            .iter()
            .filter_map(|turn| turn.turn_number)
            .max()
            .map_or(1, |turn_number| turn_number.saturating_add(1))
    }

    #[cfg(test)]
    fn hydrate_turn_history_for_test(&mut self, turns: Vec<TurnRecord>) {
        self.turns = turns;
        self.current_turn = None;
        self.turn_count = self
            .turns
            .iter()
            .filter_map(|turn| turn.turn_number)
            .max()
            .unwrap_or(0);
        self.next_turn_number = Self::derive_next_turn_number(&self.turns);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
    use crate::thread_handle::ThreadHandle;
    use crate::trace::TraceConfig;
    use crate::turn_log_store::{
        append_turn_record, persist_turn_log_snapshot, recover_thread_log_state,
    };
    use argus_protocol::llm::{CompletionRequest, CompletionResponse, LlmError};
    use argus_protocol::{AgentId, AgentType, ProviderId, ThreadCommand, ThreadRuntimeState};
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use serde_json::json;
    use tokio::sync::{Notify, oneshot};
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

    /// Helper: convert flat messages to TurnRecords for test hydration.
    fn turns_from_flat_messages(messages: Vec<ChatMessage>) -> Vec<TurnRecord> {
        let mut turns = Vec::new();
        let mut seq = 0u64;
        let mut turn_number = 1u32;
        let mut pending = Vec::new();
        let mut system_done = false;
        for msg in messages {
            if !system_done && msg.role == Role::System {
                turns.push(TurnRecord::system_bootstrap(seq, vec![msg]));
                seq += 1;
                system_done = true;
                continue;
            }
            pending.push(msg);
            if pending.len() == 2 {
                turns.push(TurnRecord::user_completed(seq, turn_number, pending));
                seq += 1;
                turn_number += 1;
                pending = Vec::new();
            }
        }
        if !pending.is_empty() {
            turns.push(TurnRecord::user_completed(seq, turn_number, pending));
        }
        turns
    }

    /// Hydrate thread from flat messages (converting to turns), token/turn counts, and timestamp.
    fn hydrate_from_flat_messages(
        thread: &mut Thread,
        messages: Vec<ChatMessage>,
        token_count: u32,
        turn_count: u32,
        updated_at: DateTime<Utc>,
    ) {
        thread.hydrate_turn_history_for_test(turns_from_flat_messages(messages));
        thread.token_count = token_count;
        thread.turn_count = turn_count;
        // Ensure next_turn_number accounts for the authoritative turn_count
        // (which may exceed what derive_next_turn_number computed from turn records alone).
        thread.next_turn_number = thread.next_turn_number.max(turn_count.saturating_add(1));
        thread.updated_at = updated_at;
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

    #[derive(Debug)]
    struct BlockingSecondCallProvider {
        plans: Arc<Mutex<VecDeque<ResponsePlan>>>,
        captured_user_inputs: Arc<Mutex<Vec<String>>>,
        second_call_started_tx: Mutex<Option<oneshot::Sender<()>>>,
        release_second_call: Arc<Notify>,
    }

    impl BlockingSecondCallProvider {
        fn new(
            plans: Vec<ResponsePlan>,
            second_call_started_tx: oneshot::Sender<()>,
            release_second_call: Arc<Notify>,
        ) -> Self {
            Self {
                plans: Arc::new(Mutex::new(VecDeque::from(plans))),
                captured_user_inputs: Arc::new(Mutex::new(Vec::new())),
                second_call_started_tx: Mutex::new(Some(second_call_started_tx)),
                release_second_call,
            }
        }

        fn captured_user_inputs(&self) -> Vec<String> {
            self.captured_user_inputs.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LlmProvider for BlockingSecondCallProvider {
        fn model_name(&self) -> &str {
            "blocking-second-call"
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
            let call_index = {
                let mut captured = self.captured_user_inputs.lock().unwrap();
                captured.push(last_user_input);
                captured.len()
            };

            if call_index == 2 {
                if let Some(sender) = self.second_call_started_tx.lock().unwrap().take() {
                    let _ = sender.send(());
                }
                self.release_second_call.notified().await;
            }

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

    fn seeded_thread_with_system_and_one_user_turn() -> Thread {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(compactor)
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");
        thread.hydrate_turn_history_for_test(vec![
            TurnRecord::system_bootstrap(0, vec![ChatMessage::system("sys")]),
            TurnRecord::user_completed(
                1,
                1,
                vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
            ),
        ]);
        thread
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

    async fn collect_runtime_terminal_events(
        rx: &mut broadcast::Receiver<ThreadEvent>,
        expected_count: usize,
    ) -> Vec<ThreadEvent> {
        let mut events = Vec::with_capacity(expected_count);
        timeout(Duration::from_secs(5), async {
            loop {
                match rx.recv().await {
                    Ok(event @ ThreadEvent::TurnCompleted { .. })
                    | Ok(event @ ThreadEvent::TurnFailed { .. })
                    | Ok(event @ ThreadEvent::TurnSettled { .. })
                    | Ok(event @ ThreadEvent::Idle { .. }) => {
                        events.push(event);
                        if events.len() >= expected_count {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => {}
                }
            }
        })
        .await
        .expect("thread should emit terminal events");
        events
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
    fn hydrate_from_turns_preserves_system_prompt_and_updates_runtime_state() {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        let updated_at = Utc::now();
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(compactor)
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .build()
            .unwrap();

        thread.hydrate_turn_history_for_test(vec![
            TurnRecord::system_bootstrap(
                0,
                vec![ChatMessage::system("You are a helpful assistant.")],
            ),
            TurnRecord::user_completed(
                1,
                1,
                vec![
                    ChatMessage::user("历史问题"),
                    ChatMessage::assistant("历史回答"),
                ],
            ),
        ]);
        thread.token_count = 42;
        thread.turn_count = 3;
        thread.updated_at = updated_at;

        let h: Vec<ChatMessage> = thread.history_iter().cloned().collect();
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].role, argus_protocol::llm::Role::System);
        assert_eq!(h[1].content, "历史问题");
        assert_eq!(h[2].content, "历史回答");
        assert_eq!(thread.token_count(), 42);
        assert_eq!(thread.turn_count(), 3);
        assert_eq!(thread.updated_at(), updated_at);
    }

    #[tokio::test]
    async fn history_reads_from_cached_committed_messages() {
        let mut thread = build_test_thread();
        thread.hydrate_turn_history_for_test(vec![TurnRecord::user_completed(
            1,
            1,
            vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
        )]);

        let h: Vec<_> = thread.history_iter().cloned().collect::<Vec<_>>();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].content, "hi");
        assert_eq!(h[1].content, "hello");
        assert_eq!(thread.turn_count(), 1);
    }

    #[test]
    fn history_iter_reads_from_turn_records_without_cached_flattened_messages() {
        let thread = seeded_thread_with_system_and_one_user_turn();
        let history: Vec<_> = thread.history_iter().map(|m| m.content.clone()).collect();
        assert_eq!(history, vec!["sys", "hi", "hello"]);
    }

    #[test]
    fn history_iter_skips_checkpoint_records() {
        let mut thread = seeded_thread_with_system_and_one_user_turn();
        thread.hydrate_turn_history_for_test(vec![
            TurnRecord::system_bootstrap(0, vec![ChatMessage::system("sys")]),
            TurnRecord::user_completed(1, 1, vec![ChatMessage::user("turn1")]),
            TurnRecord::checkpoint(2, 1, vec![ChatMessage::assistant("summary")]),
            TurnRecord::user_completed(3, 2, vec![ChatMessage::user("turn2")]),
        ]);

        let history: Vec<_> = thread.history_iter().map(|m| m.content.clone()).collect();
        assert_eq!(history, vec!["sys", "turn1", "turn2"]);
    }

    #[tokio::test]
    async fn compaction_appends_checkpoint_record_without_consuming_turn_number() {
        // ensure next user turn number stays contiguous after checkpoint append
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(compactor)
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");

        // Seed with a turn
        thread.hydrate_turn_history_for_test(vec![
            TurnRecord::system_bootstrap(0, vec![ChatMessage::system("sys")]),
            TurnRecord::user_completed(1, 1, vec![ChatMessage::user("hi")]),
        ]);

        // Append checkpoint record
        thread.append_checkpoint_record(1, vec![ChatMessage::assistant("summary of turn 1")]);

        // Next turn number should still be 2 (checkpoint doesn't consume turn number)
        assert_eq!(thread.next_turn_number, 2);
        // Turn count should still be 1
        assert_eq!(thread.turn_count(), 1);
    }

    #[tokio::test]
    async fn build_turn_context_uses_latest_checkpoint_plus_following_user_turns() {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(compactor)
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");

        // Seed: sys + turn 1 + turn 2 + checkpoint through turn 1
        thread.hydrate_turn_history_for_test(vec![
            TurnRecord::system_bootstrap(0, vec![ChatMessage::system("sys")]),
            TurnRecord::user_completed(1, 1, vec![ChatMessage::user("turn1")]),
            TurnRecord::user_completed(2, 2, vec![ChatMessage::user("turn2")]),
            TurnRecord::checkpoint(3, 1, vec![ChatMessage::assistant("summary of turn 1")]),
        ]);

        let context = thread.build_turn_context();
        let contents: Vec<_> = context.iter().map(|m| m.content.clone()).collect();
        // Context should be: sys + summary + turn2
        assert_eq!(contents, vec!["sys", "summary of turn 1", "turn2"]);
    }

    #[test]
    fn shared_history_build_turn_context_reads_from_turn_records() {
        let mut thread = build_test_thread();
        thread.hydrate_turn_history_for_test(vec![
            TurnRecord::system_bootstrap(0, vec![ChatMessage::system("sys")]),
            TurnRecord::user_completed(
                1,
                1,
                vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
            ),
        ]);

        let h: Vec<_> = thread.history_iter().cloned().collect();
        assert_eq!(h[1].content, "hi");
        assert_eq!(h[2].content, "hello");

        let context = thread.build_turn_context();
        assert_eq!(context.len(), 3);
        assert_eq!(context[0].role, argus_protocol::llm::Role::System);
        assert_eq!(context[1].content, "hi");
        assert_eq!(context[2].content, "hello");
    }

    #[tokio::test]
    async fn shared_history_turn_reuses_thread_owned_inflight_snapshot() {
        let mut thread = build_test_thread();

        let turn = thread
            .begin_turn("hello".to_string(), None, TurnCancellation::default())
            .await
            .expect("turn should build");

        let current_turn = thread
            .current_turn
            .as_ref()
            .expect("thread should keep an in-flight turn");

        assert_eq!(
            Arc::as_ptr(&current_turn.shared),
            turn.shared_snapshot_ptr()
        );
    }

    #[tokio::test]
    async fn reactor_start_turn_uses_runtime_assigned_turn_number() {
        let thread = Arc::new(RwLock::new(build_test_thread()));
        let mut runtime = ThreadReactor::seeded_from_next_turn_number(7);
        let mut active_turn = None;

        Thread::process_reactor_action(
            Arc::clone(&thread),
            &mut runtime,
            ThreadReactorAction::StartTurn {
                turn_number: 7,
                content: "hello".to_string(),
                msg_override: None,
            },
            &mut active_turn,
        )
        .await;

        let guard = thread.read().await;
        let current_turn = guard
            .current_turn
            .as_ref()
            .expect("runtime should install an in-flight turn");

        assert_eq!(current_turn.turn_number, 7);
        assert_eq!(guard.turn_count(), 7);
    }

    #[test]
    fn builder_keeps_committed_history_and_turn_count_aligned_when_seeded_with_turns() {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        let thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(compactor)
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .turns(vec![TurnRecord::user_completed(
                1,
                1,
                vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
            )])
            .build()
            .unwrap();

        let h: Vec<_> = thread.history_iter().cloned().collect();
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].role, argus_protocol::llm::Role::System);
        assert_eq!(h[1].content, "hi");
        assert_eq!(h[2].content, "hello");
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
            .turns(vec![TurnRecord::user_completed(
                4,
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
            ThreadReactorAction::StartTurn { turn_number: 1, .. }
        ));

        let second = handle.dispatch(ThreadCommand::EnqueueUserMessage {
            content: "second".to_string(),
            msg_override: None,
        });
        assert!(matches!(second, ThreadReactorAction::Noop));

        let snapshot = handle.snapshot();
        assert_eq!(
            snapshot.state,
            ThreadRuntimeState::Running { turn_number: 1 }
        );
        assert_eq!(snapshot.queue_depth, 1);
    }

    #[test]
    fn thread_reactor_transitions_idle_to_running_and_back_to_idle() {
        let mut reactor = ThreadReactor::default();
        let mut mailbox = ThreadMailbox::default();

        let start = reactor.apply_command(
            ThreadCommand::EnqueueUserMessage {
                content: "hello".to_string(),
                msg_override: None,
            },
            &mut mailbox,
        );
        assert!(matches!(
            start,
            ThreadReactorAction::StartTurn { turn_number: 1, .. }
        ));
        assert_eq!(
            reactor.state(),
            ThreadRuntimeState::Running { turn_number: 1 }
        );

        let finish = reactor.finish_active_turn(&mut mailbox);
        assert!(matches!(finish, ThreadReactorAction::Noop));
        assert_eq!(reactor.state(), ThreadRuntimeState::Idle);
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

        Thread::spawn_reactor(Arc::clone(&thread));

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

        wait_for_idle_events(&thread, 1).await;

        let captured = provider.captured_user_inputs();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0], "first queued");
        assert_eq!(captured[1], "second queued");
    }

    #[tokio::test]
    async fn reactor_emits_turn_settled_before_idle_after_completed_turn() {
        let provider = Arc::new(SequencedProvider::new(
            Duration::from_millis(20),
            vec![ResponsePlan::Ok("settled reply".to_string())],
        ));
        let thread = build_test_thread_with_provider(provider);

        Thread::spawn_reactor(Arc::clone(&thread));
        let mut rx = {
            let guard = thread.read().await;
            guard.subscribe()
        };

        {
            let guard = thread.read().await;
            guard
                .send_user_message("settle this turn".to_string(), None)
                .expect("message should queue");
        }

        let events = collect_runtime_terminal_events(&mut rx, 3).await;
        assert!(matches!(events[0], ThreadEvent::TurnCompleted { .. }));
        assert!(matches!(events[1], ThreadEvent::TurnSettled { .. }));
        assert!(matches!(events[2], ThreadEvent::Idle { .. }));

        let guard = thread.read().await;
        assert_eq!(guard.turn_count(), 1);
        assert!(guard.token_count() > 0);
    }

    #[tokio::test]
    async fn reactor_emits_turn_settled_before_idle_after_failed_turn() {
        let thread = build_test_thread_with_provider(Arc::new(DummyProvider));

        Thread::spawn_reactor(Arc::clone(&thread));
        let mut rx = {
            let guard = thread.read().await;
            guard.subscribe()
        };

        {
            let guard = thread.read().await;
            guard
                .send_user_message("fail this turn".to_string(), None)
                .expect("message should queue");
        }

        let events = collect_runtime_terminal_events(&mut rx, 3).await;
        assert!(matches!(events[0], ThreadEvent::TurnFailed { .. }));
        assert!(matches!(events[1], ThreadEvent::TurnSettled { .. }));
        assert!(matches!(events[2], ThreadEvent::Idle { .. }));
    }

    #[tokio::test]
    async fn reactor_does_not_start_follow_up_turn_before_prior_settlement() {
        let (second_call_started_tx, mut second_call_started_rx) = oneshot::channel();
        let release_second_call = Arc::new(Notify::new());
        let provider = Arc::new(BlockingSecondCallProvider::new(
            vec![
                ResponsePlan::Ok("first turn reply".to_string()),
                ResponsePlan::Ok("second turn reply".to_string()),
            ],
            second_call_started_tx,
            Arc::clone(&release_second_call),
        ));
        let thread = build_test_thread_with_provider(provider.clone());

        Thread::spawn_reactor(Arc::clone(&thread));

        let mut rx = {
            let guard = thread.read().await;
            guard.subscribe()
        };

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

        let first_terminal_turn = timeout(Duration::from_secs(5), async {
            loop {
                tokio::select! {
                    event = rx.recv() => {
                        match event {
                            Ok(ThreadEvent::TurnCompleted { turn_number, .. })
                            | Ok(ThreadEvent::TurnFailed { turn_number, .. }) => break turn_number,
                            Ok(_) => {}
                            Err(_) => {}
                        }
                    }
                    started = &mut second_call_started_rx => {
                        started.expect("second call start signal should stay open");
                        panic!("queued follow-up work must not start before the first turn settles");
                    }
                }
            }
        })
        .await
        .expect("thread should emit a terminal turn event");

        timeout(Duration::from_secs(5), async {
            loop {
                match rx.recv().await {
                    Ok(ThreadEvent::TurnSettled { turn_number, .. })
                        if turn_number == first_terminal_turn =>
                    {
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => {}
                }
            }
        })
        .await
        .expect("first turn should settle");

        release_second_call.notify_one();
        wait_for_idle_events(&thread, 1).await;

        let captured = provider.captured_user_inputs();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0], "first queued");
        assert_eq!(captured[1], "second queued");
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

        Thread::spawn_reactor(Arc::clone(&thread));

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

        wait_for_idle_events(&thread, 1).await;

        let captured = provider.captured_user_inputs();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0], "first queued");
        assert_eq!(captured[1], "second queued");

        let assistant_count = {
            let guard = thread.read().await;
            guard
                .history_iter()
                .filter(|message| message.role == argus_protocol::llm::Role::Assistant)
                .count()
        };

        assert_eq!(
            assistant_count, 1,
            "cancelled first turn should not append assistant output",
        );

        let (user_messages, assistant_messages) = {
            let guard = thread.read().await;
            let user_messages: Vec<_> = guard
                .history_iter()
                .filter(|message| message.role == argus_protocol::llm::Role::User)
                .map(|message| message.content.clone())
                .collect();
            let assistant_messages: Vec<_> = guard
                .history_iter()
                .filter(|message| message.role == argus_protocol::llm::Role::Assistant)
                .map(|message| message.content.clone())
                .collect();
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

        Thread::spawn_reactor(Arc::clone(&thread));

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
            _messages: &[ChatMessage],
            _token_count: u32,
        ) -> Result<Option<CompactResult>, CompactError> {
            Ok(Some(CompactResult {
                summary_messages: vec![ChatMessage::user("compacted")],
                token_count: 10,
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
        hydrate_from_flat_messages(
            &mut thread,
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
        hydrate_from_flat_messages(&mut thread, persisted_messages, 90, 0, Utc::now());
        let before: Vec<_> = thread.history_iter().cloned().collect();

        let _turn = thread
            .begin_turn("follow-up".to_string(), None, TurnCancellation::default())
            .await
            .expect("turn should build after compaction");

        let after: Vec<_> = thread.history_iter().cloned().collect();
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
        hydrate_from_flat_messages(&mut thread, persisted_messages, 90, 0, Utc::now());

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
        hydrate_from_flat_messages(
            &mut thread,
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
        hydrate_from_flat_messages(
            &mut thread,
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
        hydrate_from_flat_messages(
            &mut thread,
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
            context_token_count: Some(2),
        };

        let h: Vec<_> = thread.history_iter().cloned().collect();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].content, "You are a test agent.");
        assert!(thread.current_turn.is_some());
        // Builder injected a SystemBootstrap turn
        assert_eq!(thread.turns.len(), 1);

        drop(turn);
        thread.finish_turn(Ok(output)).expect("turn should settle");

        assert!(thread.current_turn.is_none());
        assert_eq!(thread.turn_count(), 1);
        assert_eq!(thread.turns.len(), 2);
        assert_eq!(thread.turns[1].messages.len(), 2);
        assert_eq!(thread.turns[1].messages[0].content, "hi");
        assert_eq!(thread.turns[1].messages[1].content, "hello");
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
        hydrate_from_flat_messages(
            &mut thread,
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

        // turns: [hydrated user turn, cancelled turn]
        assert_eq!(thread.turns.len(), 2);
        assert!(matches!(
            thread.turns[1].state,
            crate::history::TurnState::Cancelled
        ));
        assert_eq!(thread.turns[1].messages.len(), 1);
        assert_eq!(thread.turns[1].messages[0].content, next_message);
        assert_eq!(thread.token_count(), authoritative_token_count);
    }

    #[tokio::test]
    async fn finish_turn_recomputes_token_count_from_committed_context() {
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(SmallContextProvider {
                context_window: 4_096,
            }))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");
        let persisted_messages = vec![ChatMessage::user("small history")];
        let authoritative_token_count = token_count_for_messages(&persisted_messages);
        hydrate_from_flat_messages(
            &mut thread,
            persisted_messages,
            authoritative_token_count,
            0,
            Utc::now(),
        );

        let turn = thread
            .begin_turn("next step".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should build");
        drop(turn);

        thread
            .finish_turn(Ok(TurnOutput {
                appended_messages: vec![ChatMessage::assistant("done")],
                token_usage: argus_protocol::TokenUsage {
                    input_tokens: 100,
                    output_tokens: 20,
                    total_tokens: 120,
                },
                context_token_count: Some(7),
            }))
            .expect("turn should settle");

        assert_eq!(thread.token_count(), 7);
    }

    #[tokio::test]
    async fn finish_turn_without_context_token_count_preserves_last_provider_count() {
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");
        hydrate_from_flat_messages(
            &mut thread,
            vec![ChatMessage::assistant("earlier")],
            9,
            1,
            Utc::now(),
        );

        let _turn = thread
            .begin_turn("next".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should build");
        thread
            .finish_turn(Ok(TurnOutput {
                appended_messages: vec![ChatMessage::assistant("done")],
                token_usage: argus_protocol::TokenUsage {
                    input_tokens: 40,
                    output_tokens: 5,
                    total_tokens: 45,
                },
                context_token_count: None,
            }))
            .expect("turn should settle");

        assert_eq!(thread.token_count(), 9);
        assert!(thread.token_count_stale);
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
        hydrate_from_flat_messages(&mut thread, persisted_messages, 1, 5, Utc::now());

        let _turn = thread
            .begin_turn("next".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should build");
        thread
            .finish_turn(Ok(TurnOutput {
                appended_messages: vec![ChatMessage::user("next"), ChatMessage::assistant("done")],
                token_usage: argus_protocol::TokenUsage {
                    input_tokens: 1,
                    output_tokens: 1,
                    total_tokens: 2,
                },
                context_token_count: Some(2),
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
            .agent_record(test_agent_record())
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
                context_token_count: Some(2),
            }))
            .expect("turn should settle");

        let snapshot = thread
            .turn_log_persistence_snapshot()
            .expect("trace-enabled thread should expose persistence snapshot");
        persist_turn_log_snapshot(&snapshot)
            .await
            .expect("snapshot should persist");

        let recovered = recover_thread_log_state(&snapshot.base_dir)
            .await
            .expect("meta.jsonl should recover");

        // SystemBootstrap + UserTurn
        assert_eq!(recovered.turns.len(), 2);
        assert!(matches!(
            recovered.turns[0].kind,
            crate::history::TurnRecordKind::SystemBootstrap
        ));
        let user_turn = recovered
            .turns
            .iter()
            .find(|t| matches!(t.kind, crate::history::TurnRecordKind::UserTurn))
            .expect("should have a user turn");
        assert_eq!(user_turn.messages.len(), 2);
        assert_eq!(user_turn.messages[0].content, "hi");
        assert_eq!(user_turn.messages[1].content, "hello");
        assert_eq!(user_turn.turn_number, Some(1));
        assert!(matches!(
            user_turn.state,
            crate::history::TurnState::Completed
        ));
        assert_eq!(
            user_turn
                .token_usage
                .as_ref()
                .map(|usage| usage.total_tokens),
            Some(2)
        );
    }

    #[tokio::test]
    async fn recovered_turn_log_state_restores_checkpoint_context_and_next_turn_number() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let base_dir = temp_dir.path().join("session").join("thread");

        // Append records in seq order: SystemBootstrap, UserTurn, UserTurn, Checkpoint
        let bootstrap =
            TurnRecord::system_bootstrap(0, vec![ChatMessage::system("Persisted system prompt")]);
        append_turn_record(&base_dir, &bootstrap)
            .await
            .expect("bootstrap should append");

        let turn_one = TurnRecord {
            seq: 1,
            kind: crate::history::TurnRecordKind::UserTurn,
            turn_number: Some(1),
            state: crate::history::TurnState::Completed,
            messages: vec![
                ChatMessage::user("old question"),
                ChatMessage::assistant("old answer"),
            ],
            token_usage: Some(argus_protocol::TokenUsage {
                input_tokens: 4,
                output_tokens: 6,
                total_tokens: 10,
            }),
            context_token_count: Some(10),
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            model: Some("dummy".to_string()),
            error: None,
        };
        append_turn_record(&base_dir, &turn_one)
            .await
            .expect("turn one should append");

        let turn_two = TurnRecord {
            seq: 2,
            kind: crate::history::TurnRecordKind::UserTurn,
            turn_number: Some(2),
            state: crate::history::TurnState::Completed,
            messages: vec![
                ChatMessage::user("latest question"),
                ChatMessage::assistant("latest answer"),
            ],
            token_usage: Some(argus_protocol::TokenUsage {
                input_tokens: 10,
                output_tokens: 12,
                total_tokens: 22,
            }),
            context_token_count: Some(22),
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            model: Some("dummy".to_string()),
            error: None,
        };
        append_turn_record(&base_dir, &turn_two)
            .await
            .expect("turn two should append");

        let checkpoint_record =
            TurnRecord::checkpoint(3, 1, vec![ChatMessage::assistant("summary of turn one")]);
        append_turn_record(&base_dir, &checkpoint_record)
            .await
            .expect("checkpoint should append");

        let recovered = recover_thread_log_state(&base_dir)
            .await
            .expect("turn log state should recover");

        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");
        thread.hydrate_from_turn_log_state(recovered, Utc::now());

        let h: Vec<_> = thread.history_iter().cloned().collect();
        assert_eq!(h.len(), 5);
        assert_eq!(h[0].content, "Persisted system prompt");
        assert_eq!(h[1].content, "old question");
        assert_eq!(h[4].content, "latest answer");
        assert_eq!(thread.turn_count(), 2);

        let context = thread.build_turn_context();
        assert_eq!(context.len(), 4);
        assert_eq!(context[0].content, "Persisted system prompt");
        assert_eq!(context[1].content, "summary of turn one");
        assert_eq!(context[2].content, "latest question");
        assert_eq!(context[3].content, "latest answer");
        assert_eq!(thread.token_count(), 22);

        let _turn = thread
            .begin_turn("follow-up".to_string(), None, TurnCancellation::default())
            .await
            .expect("recovered thread should start the next turn");
        let current_turn = thread
            .current_turn
            .as_ref()
            .expect("recovered thread should install an in-flight turn");

        assert_eq!(current_turn.turn_number, 3);
        assert_eq!(current_turn.shared.history.len(), 4);
        assert_eq!(
            current_turn.shared.history[0].content,
            "Persisted system prompt"
        );
        assert_eq!(
            current_turn.shared.history[1].content,
            "summary of turn one"
        );
        assert_eq!(current_turn.shared.history[2].content, "latest question");
        assert_eq!(current_turn.shared.history[3].content, "latest answer");
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
        assert_eq!(thread.turns.len(), 2); // SystemBootstrap + failed turn
        assert!(matches!(
            thread.turns[1].state,
            crate::history::TurnState::Failed
        ));
        assert_eq!(thread.turns[1].messages.len(), 1);
        assert_eq!(thread.turns[1].messages[0].content, "hi");
        assert_eq!(thread.token_count(), 0);
    }
}
