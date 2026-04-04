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
use super::error::{ThreadError, TurnLogError};
use super::history::{
    InFlightTurn, InFlightTurnPhase, InFlightTurnShared, TurnRecord, TurnRecordKind,
    derive_next_user_turn_number,
};
use super::plan_hook::PlanContinuationHook;
use super::plan_store::FilePlanStore;
use super::plan_tool::UpdatePlanTool;
use super::turn_log_store::{
    RecoveredThreadLogState, append_turn_record, recover_thread_log_state,
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
    /// Create a new thread reactor seeded from the owning thread's committed turn count.
    #[must_use]
    pub(crate) fn seeded_from_turn_count(turn_count: u32) -> Self {
        Self {
            state: ThreadRuntimeState::Idle,
            next_turn_number: turn_count.saturating_add(1),
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
        committed: bool,
        mailbox: &mut ThreadMailbox,
    ) -> ThreadReactorAction {
        if committed {
            self.next_turn_number = self.next_turn_number.saturating_add(1);
        }
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
            .field("token_count", &self.token_count())
            .field("turn_count", &self.turn_count())
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
        let turns = self.turns.unwrap_or_default();
        let current_turn = self.current_turn.flatten();

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
            .filter(|turn| matches!(turn.kind, TurnRecordKind::UserTurn))
            .flat_map(|turn| turn.messages.iter())
    }

    fn build_turn_context(&self) -> Arc<Vec<ChatMessage>> {
        if let Some(checkpoint_index) = self
            .turns
            .iter()
            .rposition(|turn| matches!(turn.kind, TurnRecordKind::Checkpoint))
        {
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
            .filter(|turn| matches!(turn.kind, TurnRecordKind::UserTurn))
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
    }

    pub fn hydrate_from_turn_log_state(
        &mut self,
        recovered: RecoveredThreadLogState,
        updated_at: DateTime<Utc>,
    ) {
        self.turns = recovered.turns;
        self.current_turn = None;
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
        committed_messages: Vec<ChatMessage>,
        token_usage: TokenUsage,
    ) -> Result<(), ThreadError> {
        let current_turn = self.current_turn.take().ok_or_else(|| {
            ThreadError::TurnBuildFailed("missing in-flight turn during settle".to_string())
        })?;
        let mut committed_messages = committed_messages;
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

        if current_turn.turn_number == 1 && !self.agent_record.system_prompt.is_empty() {
            committed_messages.insert(0, ChatMessage::system(&self.agent_record.system_prompt));
        }

        self.turns.push(TurnRecord::user_turn_with_times(
            current_turn.turn_number,
            committed_messages,
            token_usage,
            current_turn.started_at,
            Utc::now(),
        ));
        self.updated_at = Utc::now();

        Ok(())
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
    }

    fn trace_base_dir(&self) -> Option<std::path::PathBuf> {
        let trace_config = self
            .config
            .turn_config
            .trace_config
            .as_ref()
            .filter(|config| config.enabled)?;
        let session_id = trace_config.session_id.unwrap_or(self.session_id);
        Some(
            trace_config
                .trace_dir
                .join(session_id.to_string())
                .join(self.id.to_string()),
        )
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
        let (mut control_rx, mailbox, turn_count) = {
            let mut guard = thread.write().await;
            let control_rx = match guard.take_control_rx() {
                Some(rx) => rx,
                None => {
                    tracing::warn!("thread control receiver already taken");
                    return;
                }
            };
            (control_rx, guard.mailbox(), guard.turn_count())
        };

        let mut runtime = ThreadReactor::seeded_from_turn_count(turn_count);
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
        let next_action = runtime.finish_active_turn(false, &mut mailbox);
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
        let committed = result.is_ok();
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
                        context_token_count: Some(output.token_usage.total_tokens),
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

        let finish_result = {
            let mut guard = thread.write().await;
            guard.set_active_turn_cancellation(None);
            guard.finish_turn(result)
        };

        if let Err(error) = finish_result {
            tracing::error!("turn failed: {}", error);
        }

        if committed {
            let (base_dir, turns) = {
                let guard = thread.read().await;
                (guard.trace_base_dir(), guard.turns.clone())
            };

            if let Some(base_dir) = base_dir
                && let Err(error) = Self::persist_trace_turns(&base_dir, &turns).await
            {
                tracing::warn!(
                    error = %error,
                    "failed to persist committed turn records"
                );
            }
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
            runtime.finish_active_turn(committed, &mut guard)
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
        let turn_number = derive_next_user_turn_number(&self.turns);
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
            .compact(turn_context.as_slice(), self.token_count())
            .await
        {
            Ok(Some(result)) => {
                let new_token_count = result.token_usage.total_tokens;
                self.append_checkpoint_record(result.summary_messages, result.token_usage.clone());
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
                self.settle_current_turn(output.appended_messages, output.token_usage)?;
                Ok(())
            }
            Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)) => {
                self.current_turn = None;
                Ok(())
            }
            Err(error) => {
                self.current_turn = None;
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

    #[cfg(test)]
    fn hydrate_turn_history_for_test(&mut self, turns: Vec<TurnRecord>) {
        self.turns = turns;
        self.current_turn = None;
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
    use crate::compact::CompactResult;
    use crate::config::{ThreadConfig, TurnConfigBuilder};
    use crate::error::CompactError;
    use crate::thread_handle::ThreadHandle;
    use crate::trace::TraceConfig;
    use crate::turn_log_store::recover_thread_log_state;
    use argus_protocol::llm::{CompletionRequest, CompletionResponse, LlmError};
    use argus_protocol::{AgentId, AgentType, ProviderId, ThreadCommand, ThreadRuntimeState};
    use async_trait::async_trait;
    use rust_decimal::Decimal;

    fn usage(total_tokens: u32) -> TokenUsage {
        TokenUsage {
            input_tokens: total_tokens.saturating_sub(1),
            output_tokens: 1,
            total_tokens,
        }
    }

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

    #[test]
    fn history_iter_reads_only_successful_user_turn_messages() {
        let mut thread = build_test_thread_without_system_prompt();
        thread.hydrate_turn_history_for_test(vec![
            TurnRecord::user_turn(
                1,
                vec![ChatMessage::user("u1"), ChatMessage::assistant("a1")],
                usage(2),
            ),
            TurnRecord::checkpoint(vec![ChatMessage::assistant("summary")], usage(7)),
            TurnRecord::user_turn(
                2,
                vec![ChatMessage::user("u2"), ChatMessage::assistant("a2")],
                usage(4),
            ),
        ]);

        let history: Vec<_> = thread.history_iter().map(|m| m.content.clone()).collect();
        assert_eq!(history, vec!["u1", "a1", "u2", "a2"]);
        assert_eq!(thread.turn_count(), 2);
        assert_eq!(thread.token_count(), 4);
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
    async fn finish_turn_prepends_system_prompt_only_on_first_successful_turn() {
        let mut thread = build_test_thread();
        let _turn = thread
            .begin_turn("hello".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should build");

        thread
            .finish_turn(Ok(TurnOutput {
                appended_messages: vec![
                    ChatMessage::user("hello"),
                    ChatMessage::assistant("world"),
                ],
                token_usage: usage(6),
            }))
            .expect("turn should settle");

        assert_eq!(thread.turns.len(), 1);
        assert_eq!(thread.turns[0].turn_number, 1);
        assert_eq!(thread.turns[0].messages[0].role, Role::System);
        assert_eq!(thread.turns[0].messages[0].content, "You are a test agent.");
        assert_eq!(thread.turns[0].messages[1].content, "hello");
    }

    #[tokio::test]
    async fn finish_turn_without_system_prompt_keeps_first_turn_transcript_only() {
        let mut thread = build_test_thread_without_system_prompt();
        let _turn = thread
            .begin_turn("hello".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should build");

        thread
            .finish_turn(Ok(TurnOutput {
                appended_messages: vec![
                    ChatMessage::user("hello"),
                    ChatMessage::assistant("world"),
                ],
                token_usage: usage(5),
            }))
            .expect("turn should settle");

        assert_eq!(thread.turns.len(), 1);
        assert_eq!(thread.turns[0].messages.len(), 2);
        assert_eq!(thread.turns[0].messages[0].role, Role::User);
    }

    #[tokio::test]
    async fn cancelled_turn_does_not_append_record() {
        let mut thread = build_test_thread_without_system_prompt();
        let _turn = thread
            .begin_turn("hello".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should build");

        thread
            .finish_turn(Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)))
            .expect("cancelled turn should settle");

        assert!(thread.turns.is_empty());
        assert!(thread.current_turn.is_none());
    }

    #[tokio::test]
    async fn failed_turn_does_not_append_record() {
        let mut thread = build_test_thread_without_system_prompt();
        let _turn = thread
            .begin_turn("hello".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should build");

        let result = thread.finish_turn(Err(ThreadError::TurnFailed(
            crate::TurnError::ToolExecutionFailed {
                name: "search".to_string(),
                reason: "boom".to_string(),
            },
        )));

        assert!(matches!(result, Err(ThreadError::TurnFailed(_))));
        assert!(thread.turns.is_empty());
        assert!(thread.current_turn.is_none());
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
    async fn begin_turn_uses_last_record_usage_for_compaction_and_persists_checkpoint() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let trace_config =
            TraceConfig::new(true, temp_dir.path().to_path_buf()).with_session_id(session_id);
        let seen_token_counts = Arc::new(Mutex::new(Vec::new()));
        let compactor = RecordingCompactor {
            seen_token_counts: Arc::clone(&seen_token_counts),
            next_result: Arc::new(Mutex::new(VecDeque::from(vec![CompactResult {
                summary_messages: vec![ChatMessage::assistant("summary")],
                token_usage: usage(11),
            }]))),
        };
        let mut thread = ThreadBuilder::new()
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
        let thread_id = thread.id();

        let _turn = thread
            .begin_turn("next".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should build");

        assert_eq!(seen_token_counts.lock().unwrap().as_slice(), &[7]);
        assert!(matches!(
            thread.turns.last().map(|r| &r.kind),
            Some(TurnRecordKind::Checkpoint)
        ));

        let persisted = recover_thread_log_state(
            &temp_dir
                .path()
                .join(session_id.to_string())
                .join(thread_id.to_string()),
        )
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

    #[test]
    fn reactor_reuses_turn_number_after_uncommitted_turn() {
        let mut reactor = ThreadReactor::seeded_from_turn_count(2);
        let mut mailbox = ThreadMailbox::default();

        let first = reactor.apply_command(
            ThreadCommand::EnqueueUserMessage {
                content: "first".to_string(),
                msg_override: None,
            },
            &mut mailbox,
        );
        assert!(matches!(
            first,
            ThreadReactorAction::StartTurn { turn_number: 3, .. }
        ));

        let second = reactor.apply_command(
            ThreadCommand::EnqueueUserMessage {
                content: "second".to_string(),
                msg_override: None,
            },
            &mut mailbox,
        );
        assert!(matches!(second, ThreadReactorAction::Noop));

        let retry = reactor.finish_active_turn(false, &mut mailbox);
        assert!(matches!(
            retry,
            ThreadReactorAction::StartTurn { turn_number: 3, .. }
        ));

        let next = reactor.finish_active_turn(true, &mut mailbox);
        assert!(matches!(next, ThreadReactorAction::Noop));
        assert_eq!(reactor.state(), ThreadRuntimeState::Idle);
    }

    #[test]
    fn thread_handle_finishes_runtime_turn_with_commit_signal() {
        let runtime = ThreadReactor::seeded_from_turn_count(0);
        let mut handle = ThreadHandle::with_runtime(runtime);
        let action = handle.dispatch(ThreadCommand::EnqueueUserMessage {
            content: "hi".to_string(),
            msg_override: None,
        });
        assert!(matches!(action, ThreadReactorAction::StartTurn { .. }));

        let finish = handle.finish_active_turn(true);
        assert!(matches!(finish, ThreadReactorAction::Noop));
        assert_eq!(handle.state(), ThreadRuntimeState::Idle);
    }
}
