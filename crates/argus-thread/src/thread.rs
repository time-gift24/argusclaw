//! Thread implementation.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use derive_builder::Builder;
use tokio::sync::broadcast;

use argus_protocol::llm::{ChatMessage, LlmProvider};
use argus_protocol::tool::NamedTool;
use argus_protocol::{
    AgentRecord, HookHandler, HookRegistry, MessageOverride, SessionId, ThreadEvent, ThreadId,
};
use argus_tool::ToolManager;
use argus_turn::{TurnBuilder, TurnOutput};

use super::compact::{CompactContext, Compactor};
use super::config::ThreadConfig;
use super::error::ThreadError;
use super::plan_store::FilePlanStore;
use super::plan_tool::UpdatePlanTool;
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
    #[builder(default)]
    messages: Vec<ChatMessage>,

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

    /// Pipe for sending/receiving ThreadEvents.
    #[builder(default)]
    pipe_tx: broadcast::Sender<ThreadEvent>,

    /// Whether a Turn is currently running.
    #[builder(default)]
    turn_running: bool,

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

    /// Build the Thread.
    ///
    /// # Errors
    ///
    /// Returns `ThreadError` if required fields (`provider`, `compactor`, `agent_record`, `session_id`) are not set.
    pub fn build(self) -> Result<Thread, ThreadError> {
        let (pipe_tx, _) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);

        let agent_record = self.agent_record.ok_or(ThreadError::AgentRecordNotSet)?;
        let session_id = self.session_id.ok_or(ThreadError::SessionIdNotSet)?;

        // Initialize messages with system prompt if not empty and no existing system message
        let mut messages = self.messages.unwrap_or_default();
        let has_system_message = messages
            .first()
            .is_some_and(|m| m.role == argus_protocol::llm::Role::System);
        if !has_system_message && !agent_record.system_prompt.is_empty() {
            messages.insert(0, ChatMessage::system(&agent_record.system_prompt));
        }

        Ok(Thread {
            id: self.id.unwrap_or_default(),
            agent_record,
            session_id,
            title: self.title.flatten(),
            created_at: self.created_at.unwrap_or_else(Utc::now),
            updated_at: self.updated_at.unwrap_or_else(Utc::now),
            messages,
            provider: self.provider.ok_or(ThreadError::ProviderNotConfigured)?,
            tool_manager: self
                .tool_manager
                .unwrap_or_else(|| Arc::new(ToolManager::new())),
            compactor: self.compactor.ok_or(ThreadError::CompactorNotConfigured)?,
            hooks: self.hooks.flatten(),
            config: self.config.unwrap_or_default(),
            token_count: 0,
            turn_count: 0,
            pipe_tx,
            turn_running: false,
            plan_store: self.plan_store.unwrap_or_default(),
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
            message_count: self.messages.len(),
            token_count: self.token_count,
            turn_count: self.turn_count,
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

    /// Returns true if a Turn is currently executing.
    pub fn is_turn_running(&self) -> bool {
        self.turn_running
    }

    /// Mark that a turn has started or stopped.
    fn set_turn_running(&mut self, running: bool) {
        self.turn_running = running;
    }

    async fn spawn_turn(
        &mut self,
        content: String,
        msg_override: Option<MessageOverride>,
    ) -> Result<(), ThreadError> {
        self.set_turn_running(true);
        self.send_message_internal(content, msg_override).await?;
        self.set_turn_running(false);
        Ok(())
    }

    /// Main orchestration loop.
    ///
    /// Runs as a background task (spawned by session). Waits on the pipe,
    /// spawning turns when UserMessage arrives. Queues one pending message
    /// if a turn is already running.
    pub async fn run(&mut self) {
        use argus_protocol::ThreadEvent::{Idle, UserMessage};

        let mut rx = self.pipe_tx.subscribe();
        let mut pending_user_message: Option<ThreadEvent> = None;

        loop {
            match rx.recv().await {
                Ok(event) => {
                    match event {
                        UserMessage { content, msg_override } => {
                            if self.is_turn_running() {
                                if pending_user_message.is_none() {
                                    pending_user_message = Some(ThreadEvent::UserMessage {
                                        content: content.clone(),
                                        msg_override: msg_override.clone(),
                                    });
                                }
                            } else {
                                if let Err(e) = self.spawn_turn(content, msg_override).await {
                                    tracing::error!("turn failed: {}", e);
                                }
                            }
                        }
                        Idle { .. } => {
                            if let Some(msg) = pending_user_message.take() {
                                if let ThreadEvent::UserMessage { content, msg_override } = msg {
                                    if let Err(e) = self.spawn_turn(content, msg_override).await {
                                        tracing::error!("turn failed: {}", e);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    tracing::warn!("pipe recv error in run(): {}", e);
                }
            }
        }
    }

    /// Get current state.
    pub fn state(&self) -> ThreadState {
        ThreadState::Idle
    }

    /// Get message history (read-only).
    pub fn history(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Get current token count.
    pub fn token_count(&self) -> u32 {
        self.token_count
    }

    /// Get turn count.
    pub fn turn_count(&self) -> u32 {
        self.turn_count
    }

    /// Get a read-only snapshot of the current plan state.
    pub fn plan(&self) -> Vec<serde_json::Value> {
        self.plan_store.store().read().unwrap().clone()
    }

    /// Get the LLM provider.
    pub fn provider(&self) -> &Arc<dyn LlmProvider> {
        &self.provider
    }

    /// Get mutable access to messages (for Compactor).
    pub fn messages_mut(&mut self) -> &mut Vec<ChatMessage> {
        &mut self.messages
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

        self.messages = messages;
        self.token_count = token_count;
        self.turn_count = turn_count;
        self.updated_at = updated_at;
    }

    /// Recalculate token count from messages.
    pub fn recalculate_token_count(&mut self) {
        self.token_count = self
            .messages
            .iter()
            .map(|m| Self::estimate_tokens(&m.content))
            .sum();
    }

    fn apply_turn_output(&mut self, output: TurnOutput) {
        self.messages = output.messages;
        // Use the authoritative token count from LLMProvider's response,
        // rather than re-estimating via content length / 4.
        self.token_count = output.token_usage.total_tokens;
        self.updated_at = Utc::now();
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
        let event = ThreadEvent::UserMessage {
            content,
            msg_override,
        };
        if self.pipe_tx.send(event).is_err() {
            tracing::warn!("pipe send failed in send_user_message");
        }
        Ok(())
    }

    async fn send_message_internal(
        &mut self,
        user_input: String,
        msg_override: Option<MessageOverride>,
    ) -> Result<(), ThreadError> {
        // Compactor decides internally whether to compact
        // Clone the Arc first to avoid borrow conflicts
        let compactor = self.compactor.clone();

        // Create CompactContext for compaction
        {
            let mut context =
                CompactContext::new(&self.provider, &mut self.token_count, &mut self.messages);
            if let Err(e) = compactor.compact(&mut context).await {
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

        self.messages.push(ChatMessage::user(user_input));
        self.recalculate_token_count();
        self.execute_turn_streaming(effective_record).await
    }

    async fn execute_turn_streaming(
        &mut self,
        agent_record: Arc<AgentRecord>,
    ) -> Result<(), ThreadError> {
        self.turn_count += 1;
        let turn_number = self.turn_count;
        let thread_id = self.id.to_string();

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

        let hooks: Vec<Arc<dyn HookHandler>> = self
            .hooks
            .as_ref()
            .map(|registry| registry.all_handlers())
            .unwrap_or_default();

        // Create internal stream channel
        let (stream_tx, _stream_rx) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);

        // Build Turn using TurnBuilder
        let mut turn_builder = TurnBuilder::default()
            .turn_number(turn_number)
            .thread_id(thread_id.clone())
            .session_id(self.session_id)
            .messages(self.messages.clone())
            .provider(self.provider.clone())
            .tools(tools)
            .hooks(hooks)
            .config(self.config.turn_config.clone())
            .agent_record(agent_record)
            .stream_tx(stream_tx)
            .thread_event_tx(self.pipe_tx.clone());

        if let Some(ref tc) = self.config.turn_config.trace_config {
            turn_builder = turn_builder.trace_config(tc.clone());
        }

        let turn = turn_builder
            .build()
            .map_err(|e| ThreadError::TurnBuildFailed(e.to_string()))?;

        // Turn is responsible for execution
        let result = turn.execute().await;

        match result {
            Ok(output) => {
                self.apply_turn_output(output);
                Ok(())
            }
            Err(error) => Err(ThreadError::TurnFailed(error)),
        }
    }

    /// Estimate token count for a string.
    fn estimate_tokens(content: &str) -> u32 {
        (content.len() / 4).max(1) as u32
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compact::KeepRecentCompactor;
    use argus_protocol::llm::{
        CompletionRequest, CompletionResponse, LlmError, ToolCompletionRequest,
        ToolCompletionResponse,
    };
    use argus_protocol::{AgentId, AgentType, ProviderId};
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use serde_json::json;

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

        async fn complete_with_tools(
            &self,
            _request: ToolCompletionRequest,
        ) -> Result<ToolCompletionResponse, LlmError> {
            Err(LlmError::RequestFailed {
                provider: "dummy".to_string(),
                reason: "not implemented".to_string(),
            })
        }
    }

    fn test_agent_record() -> Arc<AgentRecord> {
        Arc::new(AgentRecord {
            id: AgentId::new(1),
            display_name: "Test Agent".to_string(),
            description: "A test agent".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(ProviderId::new(1)),
            system_prompt: "You are a test agent.".to_string(),
            tool_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: None,
            parent_agent_id: None,
            agent_type: AgentType::Standard,
        })
    }

    #[test]
    fn thread_builder_requires_provider() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());
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
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());
        let result = ThreadBuilder::new()
            .compactor(compactor)
            .session_id(SessionId::new())
            .build();
        assert!(matches!(result, Err(ThreadError::AgentRecordNotSet)));
    }

    #[test]
    fn thread_builder_requires_session_id() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());
        let result = ThreadBuilder::new()
            .compactor(compactor)
            .agent_record(test_agent_record())
            .build();
        assert!(matches!(result, Err(ThreadError::SessionIdNotSet)));
    }

    #[test]
    fn estimate_tokens_reasonable() {
        assert_eq!(Thread::estimate_tokens("test"), 1);
        assert_eq!(Thread::estimate_tokens("test test"), 2);
        assert_eq!(Thread::estimate_tokens(""), 1);
    }

    #[test]
    fn hydrate_from_persisted_state_preserves_system_prompt_and_updates_runtime_state() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());
        let updated_at = Utc::now();
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(compactor)
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .build()
            .unwrap();

        thread.hydrate_from_persisted_state(
            vec![ChatMessage::user("历史问题"), ChatMessage::assistant("历史回答")],
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

    #[test]
    fn plan_returns_read_only_snapshot() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());

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
}
