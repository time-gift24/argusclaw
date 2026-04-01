//! Thread implementation.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use derive_builder::Builder;
use tokio::sync::{Mutex, broadcast, mpsc};

use crate::turn::TurnCancellation;
use crate::{TurnBuilder, TurnOutput};
use argus_protocol::llm::{
    ChatMessage, ChatMessageMetadata, ChatMessageMetadataMode, CompletionRequest, LlmProvider,
};
use argus_protocol::tool::NamedTool;
use argus_protocol::{
    AgentRecord, HookHandler, HookRegistry, McpToolResolver, MessageOverride, SessionId,
    ThreadControlEvent, ThreadEvent, ThreadId, ThreadMailbox, ThreadNoticeLevel,
};
use argus_tool::ToolManager;

use super::compact::{CompactContext, Compactor};
use super::config::ThreadConfig;
use super::error::ThreadError;
use super::plan_hook::PlanContinuationHook;
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

    /// Optional hidden compact agent binding used for pre-turn summarization.
    #[builder(default, setter(strip_option))]
    compact_agent_record: Option<Arc<AgentRecord>>,

    /// Provider used to execute hidden compact-agent turns.
    #[builder(default, setter(strip_option))]
    compact_agent_provider: Option<Arc<dyn LlmProvider>>,

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

    /// Internal control-plane sender for low-volume orchestration messages.
    #[builder(default)]
    control_tx: mpsc::UnboundedSender<ThreadControlEvent>,

    /// Single-consumer control receiver, taken by the session orchestrator.
    #[builder(default)]
    control_rx: Option<mpsc::UnboundedReceiver<ThreadControlEvent>>,

    /// Thread-level mailbox shared between the orchestrator and active turns.
    #[builder(default = "Arc::new(Mutex::new(ThreadMailbox::default()))")]
    mailbox: Arc<Mutex<ThreadMailbox>>,

    /// Optional runtime resolver that injects ready MCP tools for this agent.
    #[builder(default, setter(strip_option))]
    mcp_tool_resolver: Option<Arc<dyn McpToolResolver>>,

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
            .field("token_count", &self.token_count)
            .field("turn_count", &self.turn_count)
            .field("compact_agent_id", &self.config.compact_agent_id)
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
            compact_agent_record: self.compact_agent_record.flatten(),
            compact_agent_provider: self.compact_agent_provider.flatten(),
            compactor: self.compactor.ok_or(ThreadError::CompactorNotConfigured)?,
            hooks: self.hooks.flatten(),
            config: self.config.unwrap_or_default(),
            token_count: 0,
            turn_count: 0,
            pipe_tx,
            control_tx,
            control_rx: Some(control_rx),
            mailbox: Arc::new(Mutex::new(ThreadMailbox::default())),
            mcp_tool_resolver: self.mcp_tool_resolver.flatten(),
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

    /// Replace the bound LLM provider for subsequent turns.
    pub fn set_provider(&mut self, provider: Arc<dyn LlmProvider>) {
        let model_name = provider.model_name().to_string();
        self.provider = provider;
        if let Some(trace_config) = self.config.turn_config.trace_config.as_mut() {
            trace_config.model = Some(model_name);
        }
        self.updated_at = Utc::now();
    }

    /// Replace the runtime MCP tool resolver for subsequent turns.
    pub fn set_mcp_tool_resolver(&mut self, resolver: Option<Arc<dyn McpToolResolver>>) {
        self.mcp_tool_resolver = resolver;
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

    fn apply_turn_output(&mut self, output: TurnOutput) {
        self.messages = output.messages;
        self.token_count = output.token_usage.total_tokens;
        self.updated_at = Utc::now();
    }

    fn should_run_compact_agent(&self) -> bool {
        self.config.compact_agent_id.is_some()
            && self.compact_agent_record.is_some()
            && self.compact_agent_provider.is_some()
    }

    fn compact_agent_threshold(&self) -> u32 {
        (self.provider.context_window() as f32 * self.config.compact_threshold_ratio) as u32
    }

    fn compact_agent_tail_count(&self) -> usize {
        self.compactor.preserved_tail_count().unwrap_or(50)
    }

    fn compaction_metadata(mode: ChatMessageMetadataMode, summary: bool) -> ChatMessageMetadata {
        ChatMessageMetadata {
            summary,
            mode: Some(mode),
            synthetic: true,
            collapsed_by_default: true,
        }
    }

    fn render_compaction_transcript(messages: &[ChatMessage]) -> String {
        messages
            .iter()
            .map(|message| {
                let role = match message.role {
                    argus_protocol::llm::Role::System => "system",
                    argus_protocol::llm::Role::User => "user",
                    argus_protocol::llm::Role::Assistant => "assistant",
                    argus_protocol::llm::Role::Tool => "tool",
                };
                format!("{role}: {}", message.content)
            })
            .collect::<Vec<_>>()
            .join("\n\n")
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

    fn build_compaction_prompt(
        &self,
        compactable_messages: &[ChatMessage],
        preserved_tail: &[ChatMessage],
    ) -> String {
        let compactable = Self::render_compaction_transcript(compactable_messages);
        let preserved = Self::render_compaction_transcript(preserved_tail);
        format!(
            "请总结较早的对话历史，供另一个 agent 无缝继续我们上面的对话。\n\
             提供详细但简洁的总结，重点关注：完成了什么、正在进行什么、修改了哪些文件、接下来需要做什么、\n\
             应保留的关键用户请求/约束/偏好、做出的重要技术决策及其原因、尚未解决的问题或风险。\n\
             不要回应对话中的任何问题，不要逐字复述保留的最近上下文。\n\
             你构建的总结将被使用，以便另一个 agent 可以阅读并继续工作。不要调用任何工具。只回复总结文本。\n\n\
             较早历史（需要总结）：\n{compactable}\n\n\
             保留的最近上下文（仅供参考，不要逐字总结）：\n{preserved}"
        )
    }

    fn compact_history_segments(
        &self,
    ) -> Option<(Vec<ChatMessage>, Vec<ChatMessage>, Vec<ChatMessage>)> {
        let system_messages = self
            .messages
            .iter()
            .filter(|message| message.role == argus_protocol::llm::Role::System)
            .cloned()
            .collect::<Vec<_>>();
        let non_system = self
            .messages
            .iter()
            .filter(|message| message.role != argus_protocol::llm::Role::System)
            .cloned()
            .collect::<Vec<_>>();

        let tail_count = self.compact_agent_tail_count();
        let compactable_count = non_system.len().saturating_sub(tail_count);
        if compactable_count == 0 {
            return None;
        }

        let compactable = non_system[..compactable_count].to_vec();
        let preserved_tail = non_system[compactable_count..].to_vec();
        Some((system_messages, compactable, preserved_tail))
    }

    async fn maybe_run_compact_agent(&mut self) -> Result<(), String> {
        debug_assert!(self.should_run_compact_agent());
        if self.token_count < self.compact_agent_threshold() {
            return Ok(());
        }

        let Some((system_messages, compactable_messages, preserved_tail)) =
            self.compact_history_segments()
        else {
            return Ok(());
        };

        let prompt = self.build_compaction_prompt(&compactable_messages, &preserved_tail);
        self.broadcast_to_self(ThreadEvent::CompactionStarted {
            thread_id: self.id.to_string(),
        });

        let compact_agent_record = self
            .compact_agent_record
            .as_ref()
            .expect("compact agent record checked above");
        let compact_agent_provider = self
            .compact_agent_provider
            .as_ref()
            .expect("compact agent provider checked above");

        let mut request_messages = Vec::new();
        if !compact_agent_record.system_prompt.trim().is_empty() {
            request_messages.push(ChatMessage::system(
                compact_agent_record.system_prompt.clone(),
            ));
        }
        request_messages.push(ChatMessage::user(prompt.clone()));

        let mut request = CompletionRequest::new(request_messages);
        if let Some(model) = compact_agent_record.model_id.as_deref() {
            request = request.with_model(model);
        }
        if let Some(max_tokens) = compact_agent_record.max_tokens {
            request.max_tokens = Some(max_tokens);
        }
        if let Some(temperature) = compact_agent_record.temperature {
            request.temperature = Some(temperature);
        }
        if let Some(thinking) = compact_agent_record.thinking_config.clone() {
            request.thinking = Some(thinking);
        }

        let response = compact_agent_provider
            .complete(request)
            .await
            .map_err(|error| error.to_string())?;
        let summary = response.content.unwrap_or_default();

        let synthetic_prompt = ChatMessage::user(prompt).with_metadata(Self::compaction_metadata(
            ChatMessageMetadataMode::CompactionPrompt,
            false,
        ));
        let synthetic_summary = ChatMessage::assistant(summary).with_metadata(
            Self::compaction_metadata(ChatMessageMetadataMode::CompactionSummary, true),
        );
        let synthetic_replay = ChatMessage::user(
            "Continue the conversation using the summary above and the preserved recent tail below.",
        )
        .with_metadata(Self::compaction_metadata(
            ChatMessageMetadataMode::CompactionReplay,
            false,
        ));
        let trace_prelude_messages = vec![
            synthetic_prompt.clone(),
            synthetic_summary.clone(),
            synthetic_replay.clone(),
        ];

        self.messages = system_messages;
        self.messages.push(synthetic_prompt);
        self.messages.push(synthetic_summary);
        self.messages.push(synthetic_replay);
        self.messages.extend(preserved_tail);
        self.pending_trace_prelude_messages = trace_prelude_messages;

        self.broadcast_to_self(ThreadEvent::CompactionFinished {
            thread_id: self.id.to_string(),
        });
        Ok(())
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

        if self.should_run_compact_agent() {
            if let Err(error) = self.maybe_run_compact_agent().await {
                tracing::warn!("Compact agent failed: {}", error);
                self.broadcast_to_self(ThreadEvent::CompactionFailed {
                    thread_id: self.id.to_string(),
                    error,
                });
            }
        } else {
            let compactor = self.compactor.clone();
            let pre_token_count = self.token_count;
            let pre_message_count = self.messages.len();

            let compact_result = {
                let mut context =
                    CompactContext::new(&self.provider, &mut self.token_count, &mut self.messages)
                        .with_threshold_ratio_override(self.config.compact_threshold_ratio);
                compactor.compact(&mut context).await
            };
            match compact_result {
                Ok(()) => {
                    if self.token_count != pre_token_count
                        || self.messages.len() != pre_message_count
                    {
                        self.broadcast_to_self(ThreadEvent::Compacted {
                            thread_id: self.id.to_string(),
                            new_token_count: self.token_count,
                        });
                    }
                }
                Err(e) => {
                    tracing::warn!("Compact failed: {}", e);
                }
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

        let resolved_mcp_tools = match self.resolve_mcp_tools(&effective_record).await {
            Ok(tools) => tools,
            Err(error) => {
                self.set_turn_running(false);
                return Err(error);
            }
        };

        self.messages.push(ChatMessage::user(user_input));
        match self.build_turn(effective_record, cancellation, resolved_mcp_tools) {
            Ok(turn) => Ok(turn),
            Err(error) => {
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
                self.apply_turn_output(output);
                Ok(())
            }
            Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)) => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn build_turn(
        &mut self,
        agent_record: Arc<AgentRecord>,
        cancellation: TurnCancellation,
        mcp_tools: Vec<Arc<dyn NamedTool>>,
    ) -> Result<crate::Turn, ThreadError> {
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
        tools.extend(mcp_tools);

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
            .messages(self.messages.clone())
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
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;
    use crate::compact::KeepRecentCompactor;
    use crate::config::ThreadConfigBuilder;
    use crate::error::CompactError;
    use crate::runtime::ThreadRuntimeAction;
    use crate::thread_handle::ThreadHandle;
    use argus_protocol::llm::{CompletionRequest, CompletionResponse, LlmError};
    use argus_protocol::{
        AgentId, AgentType, McpToolResolver, McpUnavailableServerSummary, ProviderId,
        ResolvedMcpTools, ThreadCommand, ThreadNoticeLevel, ThreadRuntimeState, ToolDefinition,
        ToolError, ToolExecutionContext,
    };
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

    struct FailingCompactor;

    #[async_trait]
    impl Compactor for FailingCompactor {
        async fn compact(&self, _context: &mut CompactContext<'_>) -> Result<(), CompactError> {
            Err(CompactError::Failed {
                reason: "boom".to_string(),
            })
        }

        fn name(&self) -> &'static str {
            "failing"
        }
    }

    struct SummaryProvider {
        summary: String,
    }

    #[async_trait]
    impl LlmProvider for SummaryProvider {
        fn model_name(&self) -> &str {
            "summary-provider"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            Ok(CompletionResponse {
                content: Some(self.summary.clone()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 12,
                output_tokens: 8,
                finish_reason: argus_protocol::llm::FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }
    }

    struct RecordingSummaryProvider {
        summary: String,
        captured_requests: Arc<Mutex<Vec<CompletionRequest>>>,
    }

    #[async_trait]
    impl LlmProvider for RecordingSummaryProvider {
        fn model_name(&self) -> &str {
            "recording-summary-provider"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            self.captured_requests.lock().unwrap().push(request);
            Ok(CompletionResponse {
                content: Some(self.summary.clone()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 12,
                output_tokens: 8,
                finish_reason: argus_protocol::llm::FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }
    }

    struct FailingSummaryProvider;

    #[async_trait]
    impl LlmProvider for FailingSummaryProvider {
        fn model_name(&self) -> &str {
            "failing-summary-provider"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            Err(LlmError::RequestFailed {
                provider: "failing-summary-provider".to_string(),
                reason: "summary failed".to_string(),
            })
        }
    }

    fn compact_agent_record() -> Arc<AgentRecord> {
        Arc::new(AgentRecord {
            id: AgentId::new(99),
            display_name: "Compact Context".to_string(),
            description: "Summarizes stale history".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(ProviderId::new(2)),
            model_id: Some("compact-model".to_string()),
            system_prompt:
                "你是一个有用的AI助手，负责总结对话历史，供后续 agent 无缝继续工作。只输出总结文本。"
                    .to_string(),
            tool_names: vec![],
            max_tokens: Some(256),
            temperature: Some(0.1),
            thinking_config: None,
            parent_agent_id: None,
            agent_type: AgentType::Standard,
        })
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

    fn build_test_thread(provider: Arc<dyn LlmProvider>) -> Arc<tokio::sync::RwLock<Thread>> {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());
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

    struct TestMcpTool {
        name: String,
    }

    impl TestMcpTool {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
            }
        }
    }

    #[async_trait]
    impl NamedTool for TestMcpTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: self.name.clone(),
                description: "Test MCP tool".to_string(),
                parameters: json!({"type": "object"}),
            }
        }

        async fn execute(
            &self,
            input: serde_json::Value,
            _ctx: Arc<ToolExecutionContext>,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(input)
        }
    }

    enum FakeMcpResolverPlan {
        Success {
            tools: Vec<Arc<dyn NamedTool>>,
            unavailable_servers: Vec<McpUnavailableServerSummary>,
        },
        Failure(String),
    }

    struct FakeMcpResolver {
        plan: FakeMcpResolverPlan,
    }

    impl FakeMcpResolver {
        fn ready(tools: Vec<Arc<dyn NamedTool>>) -> Self {
            Self {
                plan: FakeMcpResolverPlan::Success {
                    tools,
                    unavailable_servers: Vec::new(),
                },
            }
        }

        fn unavailable(display_name: &str, reason: &str) -> Self {
            Self {
                plan: FakeMcpResolverPlan::Success {
                    tools: Vec::new(),
                    unavailable_servers: vec![McpUnavailableServerSummary {
                        server_id: 7,
                        display_name: display_name.to_string(),
                        reason: reason.to_string(),
                    }],
                },
            }
        }

        fn failure(reason: &str) -> Self {
            Self {
                plan: FakeMcpResolverPlan::Failure(reason.to_string()),
            }
        }
    }

    #[async_trait]
    impl McpToolResolver for FakeMcpResolver {
        async fn resolve_for_agent(
            &self,
            _agent_id: AgentId,
        ) -> argus_protocol::Result<ResolvedMcpTools> {
            match &self.plan {
                FakeMcpResolverPlan::Success {
                    tools,
                    unavailable_servers,
                } => Ok(ResolvedMcpTools::new(
                    tools.iter().cloned().collect(),
                    unavailable_servers.clone(),
                )),
                FakeMcpResolverPlan::Failure(reason) => Err(argus_protocol::ArgusError::LlmError {
                    reason: reason.clone(),
                }),
            }
        }
    }

    fn repeated_test_message() -> String {
        ["test"; 10].join(" ")
    }

    fn token_count_for_messages(messages: &[ChatMessage]) -> u32 {
        messages
            .iter()
            .map(|message| {
                crate::estimate_tokens(&message.content).expect("tokenization should succeed")
            })
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
    fn build_turn_accepts_pre_resolved_mcp_tools_synchronously() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(compactor)
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");
        thread.messages.push(ChatMessage::user("hello"));

        let turn = thread
            .build_turn(
                thread.agent_record.clone(),
                TurnCancellation::new(),
                vec![Arc::new(TestMcpTool::new("mcp__test__inspect"))],
            )
            .expect("turn should build synchronously");

        assert!(format!("{turn:?}").contains("tools: 2"));
    }

    #[tokio::test]
    async fn begin_turn_injects_ready_mcp_tools_without_connecting_inline() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(compactor)
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .mcp_tool_resolver(Arc::new(FakeMcpResolver::ready(vec![Arc::new(
                TestMcpTool::new("mcp__test__inspect"),
            )])))
            .build()
            .expect("thread should build");

        let turn = thread
            .begin_turn("hello".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should build");

        assert!(format!("{turn:?}").contains("tools: 2"));
    }

    #[tokio::test]
    async fn begin_turn_skips_unavailable_mcp_tools_and_emits_notice() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(compactor)
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .mcp_tool_resolver(Arc::new(FakeMcpResolver::unavailable(
                "Slack MCP",
                "offline",
            )))
            .build()
            .expect("thread should build");
        let mut rx = thread.subscribe();

        let turn = thread
            .begin_turn("hello".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should build");

        assert!(format!("{turn:?}").contains("tools: 1"));
        let event = timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("thread should emit a notice")
            .expect("notice event should be readable");
        assert!(matches!(
            event,
            ThreadEvent::Notice {
                level: ThreadNoticeLevel::Warning,
                message,
                ..
            } if message.contains("Slack MCP") && message.contains("offline")
        ));
    }

    #[tokio::test]
    async fn begin_turn_returns_resolution_error_when_mcp_resolver_fails() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(compactor)
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .mcp_tool_resolver(Arc::new(FakeMcpResolver::failure("resolver boom")))
            .build()
            .expect("thread should build");

        let error = thread
            .begin_turn("hello".to_string(), None, TurnCancellation::new())
            .await
            .expect_err("turn should fail when mcp resolution fails");

        assert!(matches!(
            error,
            ThreadError::McpToolResolutionFailed { reason } if reason.contains("resolver boom")
        ));
        assert!(!thread.is_turn_running());
        assert_eq!(thread.history().len(), 0);
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
        let thread = build_test_thread(provider.clone());

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
    async fn user_interrupt_cancels_active_turn_and_preserves_queue() {
        let provider = Arc::new(SequencedProvider::new(
            Duration::from_millis(120),
            vec![
                ResponsePlan::Ok("first turn reply".to_string()),
                ResponsePlan::Ok("second turn reply".to_string()),
            ],
        ));
        let thread = build_test_thread(provider.clone());

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
        let thread = build_test_thread(provider.clone());

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

    #[tokio::test]
    async fn begin_turn_broadcasts_compacted_event_when_pre_turn_compaction_changes_history() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::new(0.8, 1));
        let config = ThreadConfigBuilder::default()
            .compact_threshold_ratio(0.2)
            .build()
            .expect("thread config should build");
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(SmallContextProvider {
                context_window: 100,
            }))
            .compactor(compactor)
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .config(config)
            .build()
            .expect("thread should build");
        let repeated = repeated_test_message();
        let persisted_messages = vec![
            ChatMessage::user(repeated.clone()),
            ChatMessage::user(repeated.clone()),
            ChatMessage::user(repeated.clone()),
        ];
        let expected_compacted_token_count = token_count_for_messages(&persisted_messages[..1]);
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
                new_token_count,
                ..
            } if new_token_count == expected_compacted_token_count
        ));
    }

    #[tokio::test]
    async fn begin_turn_does_not_broadcast_compacted_event_when_history_is_unchanged() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::new(0.8, 1));
        let config = ThreadConfigBuilder::default()
            .compact_threshold_ratio(0.8)
            .build()
            .expect("thread config should build");
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(SmallContextProvider {
                context_window: 100,
            }))
            .compactor(compactor)
            .agent_record(test_agent_record_without_system_prompt())
            .session_id(SessionId::new())
            .config(config)
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
            .compactor(Arc::new(KeepRecentCompactor::with_defaults()))
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
            .begin_turn(next_message, None, TurnCancellation::new())
            .await
            .expect("turn should build");

        assert_eq!(thread.token_count(), authoritative_token_count);
    }

    #[tokio::test]
    async fn finish_turn_cancelled_preserves_last_authoritative_token_count() {
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(SmallContextProvider {
                context_window: 4_096,
            }))
            .compactor(Arc::new(KeepRecentCompactor::with_defaults()))
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
            .begin_turn(next_message, None, TurnCancellation::new())
            .await
            .expect("turn should build");
        thread
            .finish_turn(Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)))
            .expect("cancelled turn should be ignored");

        assert_eq!(thread.token_count(), authoritative_token_count);
    }

    #[tokio::test]
    async fn begin_turn_uses_compact_agent_without_changing_authoritative_token_count() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::new(0.8, 1));
        let config = ThreadConfigBuilder::default()
            .compact_threshold_ratio(0.2)
            .compact_agent_id(Some(AgentId::new(99)))
            .build()
            .expect("thread config should build");
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(SmallContextProvider { context_window: 40 }))
            .compactor(compactor)
            .agent_record(test_agent_record_without_system_prompt())
            .compact_agent_record(compact_agent_record())
            .compact_agent_provider(Arc::new(SummaryProvider {
                summary: "历史摘要".to_string(),
            }))
            .session_id(SessionId::new())
            .config(config)
            .build()
            .expect("thread should build");
        let repeated = repeated_test_message();
        let persisted_messages = vec![
            ChatMessage::user(repeated.clone()),
            ChatMessage::assistant("earlier assistant"),
            ChatMessage::user("most recent tail"),
        ];
        let authoritative_token_count = token_count_for_messages(&persisted_messages);
        thread.hydrate_from_persisted_state(
            persisted_messages,
            authoritative_token_count,
            0,
            Utc::now(),
        );

        let mut rx = thread.subscribe();
        let _turn = thread
            .begin_turn(
                "real user message".to_string(),
                None,
                TurnCancellation::new(),
            )
            .await
            .expect("turn should build after compaction");

        assert_eq!(
            thread.turn_count(),
            1,
            "hidden compaction must not count as a user turn"
        );
        assert_eq!(
            thread.token_count(),
            authoritative_token_count,
            "authoritative token count should stay frozen until the visible turn completes",
        );
        assert_eq!(
            thread
                .history()
                .iter()
                .filter(|message| message.content == "real user message")
                .count(),
            1,
            "the live user input must appear exactly once",
        );

        let messages = thread.history();
        assert_eq!(messages[0].role, argus_protocol::llm::Role::User);
        assert_eq!(
            messages[0].metadata.as_ref().and_then(|m| m.mode),
            Some(argus_protocol::llm::ChatMessageMetadataMode::CompactionPrompt)
        );
        assert_eq!(messages[1].role, argus_protocol::llm::Role::Assistant);
        assert_eq!(messages[1].content, "历史摘要");
        assert_eq!(
            messages[1].metadata.as_ref().and_then(|m| m.mode),
            Some(argus_protocol::llm::ChatMessageMetadataMode::CompactionSummary)
        );
        assert_eq!(messages[2].role, argus_protocol::llm::Role::User);
        assert_eq!(
            messages[2].metadata.as_ref().and_then(|m| m.mode),
            Some(argus_protocol::llm::ChatMessageMetadataMode::CompactionReplay)
        );
        assert_eq!(messages[3].content, "most recent tail");
        assert_eq!(messages[4].content, "real user message");

        let first_event = timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("compaction started event should arrive")
            .expect("event should be readable");
        assert!(matches!(first_event, ThreadEvent::CompactionStarted { .. }));

        let second_event = timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("compaction finished event should arrive")
            .expect("event should be readable");
        assert!(matches!(
            second_event,
            ThreadEvent::CompactionFinished { .. }
        ));
    }

    #[tokio::test]
    async fn begin_turn_compact_agent_failure_preserves_history_and_continues_visible_turn() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::new(0.8, 1));
        let config = ThreadConfigBuilder::default()
            .compact_threshold_ratio(0.2)
            .compact_agent_id(Some(AgentId::new(99)))
            .build()
            .expect("thread config should build");
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(SmallContextProvider { context_window: 40 }))
            .compactor(compactor)
            .agent_record(test_agent_record_without_system_prompt())
            .compact_agent_record(compact_agent_record())
            .compact_agent_provider(Arc::new(FailingSummaryProvider))
            .session_id(SessionId::new())
            .config(config)
            .build()
            .expect("thread should build");
        let persisted_messages = vec![
            ChatMessage::user(repeated_test_message()),
            ChatMessage::assistant("assistant tail"),
        ];
        let authoritative_token_count = token_count_for_messages(&persisted_messages);
        thread.hydrate_from_persisted_state(
            persisted_messages.clone(),
            authoritative_token_count,
            0,
            Utc::now(),
        );

        let mut rx = thread.subscribe();
        let _turn = thread
            .begin_turn("follow-up".to_string(), None, TurnCancellation::new())
            .await
            .expect("visible turn should still build");

        assert_eq!(thread.turn_count(), 1);
        assert_eq!(thread.token_count(), authoritative_token_count);
        assert_eq!(thread.history().len(), persisted_messages.len() + 1);
        assert_eq!(thread.history()[0].content, persisted_messages[0].content);
        assert_eq!(thread.history()[1].content, persisted_messages[1].content);
        assert_eq!(thread.history()[2].content, "follow-up");
        assert!(
            thread
                .history()
                .iter()
                .all(|message| message.metadata.is_none()),
            "failed compaction must not leave synthetic messages behind",
        );

        let first_event = timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("compaction started event should arrive")
            .expect("event should be readable");
        assert!(matches!(first_event, ThreadEvent::CompactionStarted { .. }));

        let second_event = timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("compaction failed event should arrive")
            .expect("event should be readable");
        assert!(matches!(second_event, ThreadEvent::CompactionFailed { .. }));
    }

    #[tokio::test]
    async fn compact_agent_runtime_prompt_includes_handoff_details() {
        let captured_requests = Arc::new(Mutex::new(Vec::new()));
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::new(0.8, 1));
        let config = ThreadConfigBuilder::default()
            .compact_threshold_ratio(0.2)
            .compact_agent_id(Some(AgentId::new(99)))
            .build()
            .expect("thread config should build");
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(SmallContextProvider { context_window: 40 }))
            .compactor(compactor)
            .agent_record(test_agent_record_without_system_prompt())
            .compact_agent_record(compact_agent_record())
            .compact_agent_provider(Arc::new(RecordingSummaryProvider {
                summary: "历史摘要".to_string(),
                captured_requests: Arc::clone(&captured_requests),
            }))
            .session_id(SessionId::new())
            .config(config)
            .build()
            .expect("thread should build");
        let persisted_messages = vec![
            ChatMessage::user("完成了 provider 绑定"),
            ChatMessage::assistant("修改了 thread.rs 和 manager.rs"),
            ChatMessage::user("接下来补默认 compact agent"),
            ChatMessage::assistant("记住用户偏好：自动绑定 builtin compact agent"),
        ];
        let authoritative_token_count = token_count_for_messages(&persisted_messages);
        thread.hydrate_from_persisted_state(
            persisted_messages,
            authoritative_token_count,
            0,
            Utc::now(),
        );

        let _turn = thread
            .begin_turn("follow-up".to_string(), None, TurnCancellation::new())
            .await
            .expect("visible turn should still build");

        let captured = captured_requests.lock().unwrap();
        let request = captured
            .last()
            .expect("compact agent request should be captured");
        let prompt = request
            .messages
            .last()
            .expect("request should contain runtime prompt")
            .content
            .clone();

        assert!(prompt.contains("修改了哪些文件"));
        assert!(prompt.contains("接下来需要做什么"));
        assert!(prompt.contains("另一个 agent 可以阅读并继续工作"));
        assert!(prompt.contains("不要调用任何工具"));
    }
}
