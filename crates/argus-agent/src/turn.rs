//! Turn execution helpers for managing a single turn lifecycle.
//!
//! This module owns the turn algorithm. Threads prepare snapshots of history,
//! tools, hooks, and config, then call into these helpers to execute and settle
//! one turn into a final `TurnRecord`.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use futures_util::{StreamExt, future::join_all};
use tokio::sync::{broadcast, watch};
use tokio::time::{error::Elapsed, timeout};

use super::compact::Compactor;
use super::history::TurnRecord;
use super::tool_context::{clear_current_agent_id, set_current_agent_id};
use super::{TurnConfig, TurnError, TurnStreamEvent};
use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmProvider, LlmStreamEvent,
    ToolCall, ToolDefinition,
};
use argus_protocol::tool::NamedTool;
use argus_protocol::{
    AgentRecord, HookAction, HookEvent, HookHandler, ThreadEvent, TokenUsage, ToolExecutionContext,
    ToolHookContext, ids::ThreadId, sanitize_tool_output,
};

const DEFAULT_TURN_CHANNEL_CAPACITY: usize = 256;

/// Cancellation primitive used to stop an active turn.
///
/// This token is cloneable so the runtime can hold a handle to cancel the active
/// turn while the turn itself periodically checks for cancellation.
#[derive(Clone, Debug)]
pub struct TurnCancellation {
    tx: watch::Sender<bool>,
    rx: watch::Receiver<bool>,
}

impl TurnCancellation {
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(false);
        Self { tx, rx }
    }

    pub fn cancel(&self) {
        let _ = self.tx.send(true);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        *self.rx.borrow()
    }

    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.rx.clone()
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }

        let mut rx = self.subscribe();
        loop {
            if rx.changed().await.is_err() {
                return;
            }
            if *rx.borrow() {
                return;
            }
        }
    }
}

impl Default for TurnCancellation {
    fn default() -> Self {
        Self::new()
    }
}

/// Turn identifier generator (simple counter for now).
#[cfg(test)]
fn generate_turn_id(thread_id: &str, turn_number: u32) -> String {
    format!("{}-turn-{}", thread_id, turn_number)
}

/// Result of processing an LLM response's finish_reason.
enum NextAction {
    /// Turn is complete and can return a record unless a hook continues it.
    Return,
    /// Continue with tool execution.
    ContinueWithTools { tool_calls: Vec<ToolCall> },
    /// Context length exceeded.
    LengthExceeded,
}

enum StreamingCallOutcome {
    Completed(CompletionResponse),
    Failed(argus_protocol::llm::LlmError),
}

/// Shared context carried through the turn execution call chain.
///
/// Constructed once in `execute_thread_turn` from the public API parameters,
/// then passed by reference to all internal functions.
struct TurnContext<'a> {
    thread_id: ThreadId,
    turn_number: u32,
    started_at: DateTime<Utc>,
    tools: &'a [Arc<dyn NamedTool>],
    hooks: &'a [Arc<dyn HookHandler>],
    provider: &'a dyn LlmProvider,
    config: &'a TurnConfig,
    agent_record: &'a AgentRecord,
    stream_tx: &'a broadcast::Sender<TurnStreamEvent>,
    thread_event_tx: &'a broadcast::Sender<ThreadEvent>,
    cancellation: &'a TurnCancellation,
}

impl TurnContext<'_> {
    fn thread_id_str(&self) -> String {
        self.thread_id.to_string()
    }
}

fn build_hook_context(
    ctx: &TurnContext<'_>,
    event: HookEvent,
    tool_name: String,
    tool_call_id: String,
    tool_input: serde_json::Value,
    tool_result: Option<serde_json::Value>,
    error: Option<String>,
    tool_manager: Option<Arc<dyn NamedTool>>,
) -> ToolHookContext {
    ToolHookContext {
        event,
        tool_name,
        tool_call_id,
        tool_input,
        tool_result,
        error,
        tool_manager,
        thread_event_sender: Some(ctx.thread_event_tx.clone()),
        thread_id: Some(ctx.thread_id_str()),
        turn_number: Some(ctx.turn_number),
    }
}

fn build_turn_end_hook_context(ctx: &TurnContext<'_>) -> ToolHookContext {
    build_hook_context(
        ctx,
        HookEvent::TurnEnd,
        String::new(),
        String::new(),
        serde_json::Value::Null,
        None,
        None,
        None,
    )
}

/// Result of a tool execution.
struct ToolExecutionResult {
    tool_call_id: String,
    name: String,
    content: String,
}

/// Accumulates streaming events into a complete response.
struct StreamingAccumulator {
    content: String,
    reasoning_content: String,
    tool_calls: Vec<(Option<String>, Option<String>, String)>,
    input_tokens: u32,
    output_tokens: u32,
    finish_reason: FinishReason,
}

impl StreamingAccumulator {
    fn new() -> Self {
        Self {
            content: String::new(),
            reasoning_content: String::new(),
            tool_calls: Vec::new(),
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: FinishReason::Stop,
        }
    }

    fn process(&mut self, event: LlmStreamEvent) {
        match event {
            LlmStreamEvent::ReasoningDelta { delta } => {
                self.reasoning_content.push_str(&delta);
            }
            LlmStreamEvent::ContentDelta { delta } => {
                self.content.push_str(&delta);
            }
            LlmStreamEvent::ToolCallDelta(tc) => {
                // Ensure we have enough slots
                while self.tool_calls.len() <= tc.index {
                    self.tool_calls.push((None, None, String::new()));
                }
                if let Some(id) = tc.id {
                    self.tool_calls[tc.index].0 = Some(id);
                }
                if let Some(name) = tc.name {
                    self.tool_calls[tc.index].1 = Some(name);
                }
                if let Some(args_delta) = tc.arguments_delta {
                    self.tool_calls[tc.index].2.push_str(&args_delta);
                }
            }
            LlmStreamEvent::Usage {
                input_tokens,
                output_tokens,
            } => {
                self.input_tokens = input_tokens;
                self.output_tokens = output_tokens;
            }
            LlmStreamEvent::Finished { finish_reason } => {
                self.finish_reason = finish_reason;
            }
            LlmStreamEvent::RetryAttempt { .. } => {
                // Retry events are informational and forwarded to subscribers,
                // but don't affect the accumulated response state
            }
        }
    }

    fn into_response(self) -> CompletionResponse {
        // Convert accumulated tool calls to ToolCall structs
        let tool_calls: Vec<ToolCall> = self
            .tool_calls
            .into_iter()
            .filter_map(|(id, name, args)| {
                Some(ToolCall {
                    id: id?,
                    name: name?,
                    arguments: serde_json::from_str(&args).unwrap_or(serde_json::Value::Null),
                })
            })
            .collect();

        CompletionResponse {
            content: if self.content.is_empty() {
                None
            } else {
                Some(self.content)
            },
            reasoning_content: if self.reasoning_content.is_empty() {
                None
            } else {
                Some(self.reasoning_content)
            },
            tool_calls,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            finish_reason: self.finish_reason,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }
    }
}

/// Internal owned turn snapshot used by the module tests.
#[cfg(test)]
struct Turn {
    id: String,
    turn_number: u32,
    thread_id: String,
    originating_thread_id: ThreadId,
    history: Arc<Vec<ChatMessage>>,
    messages: Vec<ChatMessage>,
    tools: Arc<Vec<Arc<dyn NamedTool>>>,
    hooks: Arc<Vec<Arc<dyn HookHandler>>>,
    started_at: DateTime<Utc>,
    provider: Arc<dyn LlmProvider>,
    config: TurnConfig,
    agent_record: Arc<AgentRecord>,
    stream_tx: broadcast::Sender<TurnStreamEvent>,
    thread_event_tx: broadcast::Sender<ThreadEvent>,
    cancellation: TurnCancellation,
    compactor: Option<Arc<dyn Compactor>>,
}

#[cfg(test)]
impl std::fmt::Debug for Turn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Turn")
            .field("id", &self.id)
            .field("turn_number", &self.turn_number)
            .field("thread_id", &self.thread_id)
            .field("history", &self.history.len())
            .field("messages", &self.messages.len())
            .field("provider", &self.provider.model_name())
            .field("tools", &self.tools.len())
            .field("hooks", &self.hooks.len())
            .field("config", &self.config)
            .finish()
    }
}

fn prompt_message(system_prompt: Option<&str>) -> Option<ChatMessage> {
    system_prompt
        .filter(|prompt| !prompt.is_empty())
        .map(ChatMessage::system)
}

fn materialize_messages(
    system_prompt: Option<&str>,
    history: &[ChatMessage],
    turn_messages: &[ChatMessage],
) -> Vec<ChatMessage> {
    let mut messages = Vec::with_capacity(
        history
            .len()
            .saturating_add(turn_messages.len())
            .saturating_add(usize::from(prompt_message(system_prompt).is_some())),
    );
    if let Some(prompt) = prompt_message(system_prompt) {
        messages.push(prompt);
    }
    messages.extend(history.iter().cloned());
    messages.extend(turn_messages.iter().cloned());
    messages
}

fn estimated_tokens(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .map(|message| (message.content.len().saturating_add(3)) / 4)
        .sum()
}

fn build_turn_record(
    turn_number: u32,
    started_at: DateTime<Utc>,
    turn_messages: Vec<ChatMessage>,
    token_usage: TokenUsage,
) -> TurnRecord {
    TurnRecord::user_turn_with_times(
        turn_number,
        turn_messages,
        token_usage,
        started_at,
        Utc::now(),
    )
}

fn finalize_turn_record(
    turn_number: u32,
    started_at: DateTime<Utc>,
    history: &[ChatMessage],
    turn_messages: Vec<ChatMessage>,
    token_usage: TokenUsage,
    compacted_during_turn: bool,
) -> TurnRecord {
    if compacted_during_turn {
        let mut checkpoint_messages = history.to_vec();
        checkpoint_messages.extend(turn_messages);
        TurnRecord::turn_checkpoint_with_times(
            turn_number,
            checkpoint_messages,
            token_usage,
            started_at,
            Utc::now(),
        )
    } else {
        build_turn_record(turn_number, started_at, turn_messages, token_usage)
    }
}

fn find_tool(tools: &[Arc<dyn NamedTool>], tool_name: &str) -> Option<Arc<dyn NamedTool>> {
    tools.iter().find(|tool| tool.name() == tool_name).cloned()
}

async fn fire_hooks(
    hooks: &[Arc<dyn HookHandler>],
    ctx: &ToolHookContext,
) -> Result<HookAction, TurnError> {
    for hook in hooks {
        let action = hook.on_tool_event(ctx).await;

        if let HookAction::Block(reason) = action {
            return Ok(HookAction::Block(reason));
        }

        if !matches!(action, HookAction::Continue) {
            return Ok(action);
        }
    }

    Ok(HookAction::Continue)
}

fn apply_tool_call_limit_message(
    system_prompt: Option<&str>,
    max_tool_calls: Option<u32>,
    has_available_tools: bool,
    request_messages: &mut Vec<ChatMessage>,
) {
    if let Some(max) = max_tool_calls
        && has_available_tools
    {
        let system_content = format!(
            "IMPORTANT: You can only call at most {} tool(s) per response. \
            If you need to call multiple tools, please proceed step by step - \
            call tools one at a time and wait for the results before calling the next tool.",
            max
        );
        let insert_index = usize::from(prompt_message(system_prompt).is_some());
        request_messages.insert(insert_index, ChatMessage::system(system_content));
    }
}

fn build_completion_request(
    request_messages: Vec<ChatMessage>,
    available_tools: &[Arc<dyn NamedTool>],
    agent_record: &AgentRecord,
) -> CompletionRequest {
    let tools: Vec<ToolDefinition> = available_tools
        .iter()
        .map(|tool| tool.definition())
        .collect();
    let mut request = CompletionRequest::new(request_messages).with_tools(tools);
    if let Some(max_tokens) = agent_record.max_tokens {
        request.max_tokens = Some(max_tokens);
    }
    if let Some(temperature) = agent_record.temperature {
        request.temperature = Some(temperature);
    }
    if let Some(thinking_config) = &agent_record.thinking_config {
        request.thinking = Some(thinking_config.clone());
    }
    request
}

fn apply_tool_call_limit(
    tool_calls: Vec<ToolCall>,
    max_tool_calls: Option<u32>,
) -> Vec<ToolCall> {
    match max_tool_calls {
        Some(max) if tool_calls.len() > max as usize => {
            tracing::debug!(
                requested = tool_calls.len(),
                max_allowed = max,
                "Limiting tool calls per iteration"
            );
            tool_calls.into_iter().take(max as usize).collect()
        }
        _ => tool_calls,
    }
}

fn push_tool_call_message(
    turn_messages: &mut Vec<ChatMessage>,
    content: Option<String>,
    tool_calls: &[ToolCall],
    reasoning_content: Option<String>,
) {
    turn_messages.push(ChatMessage::assistant_with_tool_calls_and_reasoning(
        content,
        tool_calls.to_vec(),
        reasoning_content,
    ));
}

fn process_finish_reason(
    response: CompletionResponse,
    turn_messages: &mut Vec<ChatMessage>,
    token_usage: &mut TokenUsage,
    max_tool_calls: Option<u32>,
) -> NextAction {
    let CompletionResponse {
        content,
        reasoning_content,
        tool_calls: response_tool_calls,
        input_tokens,
        output_tokens,
        finish_reason,
        ..
    } = response;

    token_usage.input_tokens = input_tokens;
    token_usage.output_tokens = output_tokens;
    token_usage.total_tokens = input_tokens + output_tokens;

    match finish_reason {
        FinishReason::Stop => {
            if content.as_deref().is_some_and(|value| !value.is_empty())
                || reasoning_content
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
            {
                turn_messages.push(ChatMessage::assistant_with_reasoning(
                    content.unwrap_or_default(),
                    reasoning_content,
                ));
            }

            NextAction::Return
        }
        FinishReason::ToolUse => {
            let tool_calls = apply_tool_call_limit(response_tool_calls, max_tool_calls);
            push_tool_call_message(turn_messages, content, &tool_calls, reasoning_content);
            NextAction::ContinueWithTools { tool_calls }
        }
        FinishReason::Length => NextAction::LengthExceeded,
        FinishReason::ContentFilter | FinishReason::Unknown => {
            if !response_tool_calls.is_empty() {
                let tool_calls = apply_tool_call_limit(response_tool_calls, max_tool_calls);
                push_tool_call_message(turn_messages, content, &tool_calls, reasoning_content);
                NextAction::ContinueWithTools { tool_calls }
            } else if content.as_deref().is_some_and(|value| !value.is_empty())
                || reasoning_content
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
            {
                turn_messages.push(ChatMessage::assistant_with_reasoning(
                    content.unwrap_or_default(),
                    reasoning_content,
                ));

                NextAction::Return
            } else {
                NextAction::Return
            }
        }
    }
}

async fn call_llm_streaming(
    provider: &dyn LlmProvider,
    request: CompletionRequest,
    cancellation: &TurnCancellation,
    stream_tx: &broadcast::Sender<TurnStreamEvent>,
    thread_event_tx: &broadcast::Sender<ThreadEvent>,
    thread_id: &str,
    turn_number: u32,
) -> Result<StreamingCallOutcome, TurnError> {
    if cancellation.is_cancelled() {
        return Err(TurnError::Cancelled);
    }

    match provider.stream_complete(request.clone()).await {
        Ok(mut stream) => {
            tracing::debug!(
                thread_id = %thread_id,
                turn_number = %turn_number,
                "LLM stream started"
            );
            let mut cancel_rx = cancellation.subscribe();
            let mut accumulator = StreamingAccumulator::new();
            loop {
                tokio::select! {
                    change = cancel_rx.changed() => {
                        if change.is_ok() && *cancel_rx.borrow() {
                            return Err(TurnError::Cancelled);
                        }
                    }
                    event_result = stream.next() => {
                        let Some(event_result) = event_result else {
                            break;
                        };

                        let event = match event_result {
                            Ok(event) => event,
                            Err(error) => return Ok(StreamingCallOutcome::Failed(error)),
                        };
                        if let Err(error) = thread_event_tx.send(ThreadEvent::Processing {
                            thread_id: thread_id.to_string(),
                            turn_number,
                            event: event.clone(),
                        }) {
                            tracing::warn!(
                                thread_id = %thread_id,
                                turn_number = %turn_number,
                                error = %error,
                                "Failed to send Processing event"
                            );
                        }
                        let _ = stream_tx.send(TurnStreamEvent::LlmEvent(event.clone()));
                        accumulator.process(event);
                    }
                }
            }
            let response = accumulator.into_response();
            tracing::debug!(
                thread_id = %thread_id,
                turn_number = %turn_number,
                finish_reason = ?response.finish_reason,
                tool_call_count = %response.tool_calls.len(),
                "LLM stream completed"
            );
            Ok(StreamingCallOutcome::Completed(response))
        }
        Err(argus_protocol::llm::LlmError::UnsupportedCapability { .. }) => {
            tracing::debug!("Provider doesn't support streaming, using non-streaming fallback");
            tokio::select! {
                _ = cancellation.cancelled() => Err(TurnError::Cancelled),
                result = provider.complete(request) => result.map(StreamingCallOutcome::Completed).map_err(TurnError::LlmFailed),
            }
        }
        Err(error) => Err(TurnError::LlmFailed(error)),
    }
}

async fn execute_tools_parallel(
    ctx: &TurnContext<'_>,
    tool_calls: Vec<ToolCall>,
    tool_timeout_secs: u64,
) -> Vec<ToolExecutionResult> {
    let futures: Vec<_> = tool_calls
        .into_iter()
        .map(|tool_call| execute_single_tool(ctx, tool_call, tool_timeout_secs))
        .collect();

    join_all(futures).await
}

async fn execute_single_tool(
    ctx: &TurnContext<'_>,
    tool_call: ToolCall,
    tool_timeout_secs: u64,
) -> ToolExecutionResult {
    let thread_id = ctx.thread_id_str();
    let tool_call_id = tool_call.id.clone();
    let tool_name = tool_call.name.clone();
    let tool_input = tool_call.arguments.clone();
    let tool = find_tool(ctx.tools, &tool_name);

    let hook_ctx = build_hook_context(
        ctx,
        HookEvent::BeforeToolCall,
        tool_name.clone(),
        tool_call_id.clone(),
        tool_input.clone(),
        None,
        None,
        tool.clone(),
    );

    if let Ok(HookAction::Block(ref reason)) = fire_hooks(ctx.hooks, &hook_ctx).await {
        let content = format!("Tool call blocked: {}", reason);

        let after_ctx = build_hook_context(
            ctx,
            HookEvent::AfterToolCall,
            tool_name.clone(),
            tool_call_id.clone(),
            tool_input,
            None,
            Some(reason.clone()),
            None,
        );
        let _ = fire_hooks(ctx.hooks, &after_ctx).await;

        return ToolExecutionResult {
            tool_call_id,
            name: tool_name,
            content,
        };
    }

    if let Err(error) = ctx.thread_event_tx.send(ThreadEvent::ToolStarted {
        thread_id: thread_id.clone(),
        turn_number: ctx.turn_number,
        tool_call_id: tool_call_id.clone(),
        tool_name: tool_name.clone(),
        arguments: tool_input.clone(),
    }) {
        tracing::warn!(
            thread_id = %thread_id,
            turn_number = %ctx.turn_number,
            tool_name = %tool_name,
            error = %error,
            "Failed to send ToolStarted event"
        );
    }
    let _ = ctx.stream_tx.send(TurnStreamEvent::ToolStarted {
        tool_call_id: tool_call_id.clone(),
        tool_name: tool_name.clone(),
        arguments: tool_input.clone(),
    });

    let timeout_duration = std::time::Duration::from_secs(tool_timeout_secs);
    set_current_agent_id(ctx.agent_record.id);
    let execute_result = {
        let execute_future = async {
            if let Some(tool) = tool {
                tracing::debug!(
                    thread_id = %thread_id,
                    turn_number = %ctx.turn_number,
                    tool_call_id = %tool_call_id,
                    tool_name = %tool_name,
                    "Executing tool"
                );
                let tool_ctx = Arc::new(ToolExecutionContext {
                    thread_id: ctx.thread_id,
                    agent_id: Some(ctx.agent_record.id),
                    pipe_tx: ctx.thread_event_tx.clone(),
                });
                tool.execute(tool_input.clone(), tool_ctx).await
            } else {
                tracing::error!(
                    thread_id = %thread_id,
                    turn_number = %ctx.turn_number,
                    tool_call_id = %tool_call_id,
                    tool_name = %tool_name,
                    available_tools = ?ctx.tools.iter().map(|t: &Arc<dyn NamedTool>| t.name()).collect::<Vec<_>>(),
                    "Tool not found in registry"
                );
                Err(argus_protocol::tool::ToolError::NotFound {
                    id: tool_name.clone(),
                })
            }
        };
        timeout(timeout_duration, execute_future).await
    };
    clear_current_agent_id();

    let result = match execute_result {
        Ok(Ok(value)) => {
            tracing::info!(
                thread_id = %thread_id,
                turn_number = %ctx.turn_number,
                tool_call_id = %tool_call_id,
                tool_name = %tool_name,
                result_preview = %value.to_string().chars().take(200).collect::<String>(),
                "Tool executed successfully"
            );
            Ok(value)
        }
        Ok(Err(error)) => {
            tracing::warn!(
                thread_id = %thread_id,
                turn_number = %ctx.turn_number,
                tool_call_id = %tool_call_id,
                tool_name = %tool_name,
                error = %error,
                "Tool execution returned error"
            );
            Err(error.to_string())
        }
        Err(Elapsed { .. }) => {
            tracing::error!(
                thread_id = %thread_id,
                turn_number = %ctx.turn_number,
                tool_call_id = %tool_call_id,
                tool_name = %tool_name,
                timeout_secs = %tool_timeout_secs,
                "Tool execution timed out"
            );
            Err(format!(
                "Tool execution timed out after {}s",
                tool_timeout_secs
            ))
        }
    };

    let event_result = match &result {
        Ok(value) => Ok(value.clone()),
        Err(error) => Err(error.clone()),
    };
    if let Err(error) = ctx.thread_event_tx.send(ThreadEvent::ToolCompleted {
        thread_id: thread_id.clone(),
        turn_number: ctx.turn_number,
        tool_call_id: tool_call_id.clone(),
        tool_name: tool_name.clone(),
        result: event_result.clone(),
    }) {
        tracing::warn!(
            thread_id = %thread_id,
            turn_number = %ctx.turn_number,
            tool_name = %tool_name,
            error = %error,
            "Failed to send ToolCompleted event"
        );
    }
    let _ = ctx.stream_tx.send(TurnStreamEvent::ToolCompleted {
        tool_call_id: tool_call_id.clone(),
        tool_name: tool_name.clone(),
        result: event_result,
    });

    let (tool_result, error) = match &result {
        Ok(value) => (Some(value.clone()), None),
        Err(error) => (None, Some(error.clone())),
    };
    let after_hook_ctx = build_hook_context(
        ctx,
        HookEvent::AfterToolCall,
        tool_name.clone(),
        tool_call_id.clone(),
        tool_input,
        tool_result,
        error,
        None,
    );
    let _ = fire_hooks(ctx.hooks, &after_hook_ctx).await;

    let content = match &result {
        Ok(value) => serde_json::to_string(value).unwrap_or_else(|error| {
            format!("{{\"error\": \"Failed to serialize result: {}\"}}", error)
        }),
        Err(error) => format!("{{\"error\": \"{}\"}}", error),
    };

    ToolExecutionResult {
        tool_call_id,
        name: tool_name,
        content,
    }
}

async fn execute_loop(
    ctx: &TurnContext<'_>,
    system_prompt: Option<&str>,
    mut history: Arc<Vec<ChatMessage>>,
    mut turn_messages: Vec<ChatMessage>,
    compactor: Option<&dyn Compactor>,
) -> Result<TurnRecord, TurnError> {
    let thread_id = ctx.thread_id_str();
    let max_iterations = ctx.config.max_iterations.unwrap_or(50);
    let tool_timeout_secs = ctx.config.tool_timeout_secs.unwrap_or(120);
    let mut token_usage = TokenUsage::default();
    let mut compacted_during_turn = false;

    for iteration in 0..max_iterations {
        if ctx.cancellation.is_cancelled() {
            return Err(TurnError::Cancelled);
        }

        let mut request_messages =
            materialize_messages(system_prompt, history.as_ref(), &turn_messages);
        let request_token_count = estimated_tokens(&request_messages) as u32;
        if let Some(compactor) = compactor {
            match compactor
                .compact(&request_messages, request_token_count)
                .await
            {
                Ok(Some(result)) => {
                    compacted_during_turn = true;
                    history = Arc::new(result.messages);
                    turn_messages.clear();
                    request_messages =
                        materialize_messages(system_prompt, history.as_ref(), &turn_messages);
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(
                        thread_id = %thread_id,
                        turn_number = %ctx.turn_number,
                        iteration = %iteration,
                        error = %error,
                        "turn compaction failed; continuing without compaction"
                    );
                }
            }
        }
        apply_tool_call_limit_message(
            system_prompt,
            ctx.config.max_tool_calls,
            !ctx.tools.is_empty(),
            &mut request_messages,
        );

        tracing::debug!(
            thread_id = %thread_id,
            turn_number = %ctx.turn_number,
            iteration = %iteration,
            max_iterations = %max_iterations,
            message_count = %request_messages.len(),
            "Turn iteration started"
        );

        tracing::debug!(
            thread_id = %thread_id,
            turn_number = %ctx.turn_number,
            iteration = %iteration,
            tool_count = %ctx.tools.len(),
            message_count = %request_messages.len(),
            "Calling LLM"
        );
        let request = build_completion_request(request_messages.clone(), ctx.tools, ctx.agent_record);
        let response = match call_llm_streaming(
            ctx.provider,
            request,
            ctx.cancellation,
            ctx.stream_tx,
            ctx.thread_event_tx,
            &thread_id,
            ctx.turn_number,
        )
        .await?
        {
            StreamingCallOutcome::Completed(response) => response,
            StreamingCallOutcome::Failed(error) => return Err(TurnError::LlmFailed(error)),
        };
        tracing::debug!(
            thread_id = %thread_id,
            turn_number = %ctx.turn_number,
            iteration = %iteration,
            "LLM call completed"
        );

        let next_action = process_finish_reason(
            response,
            &mut turn_messages,
            &mut token_usage,
            ctx.config.max_tool_calls,
        );
        match next_action {
            NextAction::Return => {
                let hook_ctx = build_turn_end_hook_context(ctx);
                let turn_end_action = fire_hooks(ctx.hooks, &hook_ctx).await;

                let continue_message = match turn_end_action {
                    Ok(HookAction::ContinueWithMessage(message)) => Some(message),
                    Ok(HookAction::Continue) => None,
                    Ok(HookAction::Block(reason)) => {
                        tracing::warn!(reason = %reason, "TurnEnd hook returned Block (ignored)");
                        None
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "TurnEnd hook failed (ignored)");
                        None
                    }
                };

                if let Some(message) = continue_message {
                    turn_messages.push(ChatMessage::user(&message));

                    tracing::debug!(
                        thread_id = %thread_id,
                        turn_number = %ctx.turn_number,
                        iteration = %iteration,
                        "TurnEnd hook requested continuation with injected user message"
                    );
                    continue;
                }

                return Ok(finalize_turn_record(
                    ctx.turn_number,
                    ctx.started_at,
                    history.as_ref(),
                    turn_messages,
                    token_usage.clone(),
                    compacted_during_turn,
                ));
            }
            NextAction::ContinueWithTools { tool_calls } => {
                tracing::debug!(
                    thread_id = %thread_id,
                    turn_number = %ctx.turn_number,
                    iteration = %iteration,
                    tool_count = %tool_calls.len(),
                    "Tool calls detected, executing tools"
                );

                let tool_results: Vec<ToolExecutionResult> = tokio::select! {
                    _ = ctx.cancellation.cancelled() => {
                        return Err(TurnError::Cancelled);
                    }
                    tool_results = execute_tools_parallel(
                        ctx,
                        tool_calls,
                        tool_timeout_secs,
                    ) => tool_results,
                };

                tracing::debug!(
                    thread_id = %thread_id,
                    turn_number = %ctx.turn_number,
                    iteration = %iteration,
                    result_count = %tool_results.len(),
                    "Tools executed, adding results to message history"
                );

                for result in tool_results {
                    let (sanitized_content, warning) =
                        sanitize_tool_output(&result.content, &ctx.config.safety_config);

                    if let Some(warning) = &warning {
                        tracing::warn!(
                            thread_id = %thread_id,
                            turn_number = %ctx.turn_number,
                            tool_call_id = %result.tool_call_id,
                            tool_name = %result.name,
                            pattern = %warning.pattern,
                            original_len = warning.original_len,
                            truncated_len = warning.truncated_len,
                            "Tool output was truncated"
                        );
                    }

                    let result_len = sanitized_content.len();
                    let preview = sanitized_content.chars().take(200).collect::<String>();
                    tracing::info!(
                        thread_id = %thread_id,
                        turn_number = %ctx.turn_number,
                        tool_call_id = %result.tool_call_id,
                        tool_name = %result.name,
                        result_len,
                        result_preview = %preview,
                        "Tool result added to history"
                    );
                    turn_messages.push(ChatMessage::tool_result(
                        result.tool_call_id,
                        result.name,
                        sanitized_content,
                    ));
                }
            }
            NextAction::LengthExceeded => {
                return Err(TurnError::ContextLengthExceeded(
                    (token_usage.input_tokens + token_usage.output_tokens) as usize,
                ));
            }
        }
    }

    Err(TurnError::MaxIterationsReached(max_iterations))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_thread_turn(
    turn_number: u32,
    thread_id: String,
    originating_thread_id: ThreadId,
    history: Arc<Vec<ChatMessage>>,
    messages: Vec<ChatMessage>,
    tools: Arc<Vec<Arc<dyn NamedTool>>>,
    hooks: Arc<Vec<Arc<dyn HookHandler>>>,
    started_at: DateTime<Utc>,
    provider: Arc<dyn LlmProvider>,
    config: TurnConfig,
    agent_record: Arc<AgentRecord>,
    stream_tx: Option<broadcast::Sender<TurnStreamEvent>>,
    thread_event_tx: broadcast::Sender<ThreadEvent>,
    cancellation: TurnCancellation,
    compactor: Option<Arc<dyn Compactor>>,
) -> Result<TurnRecord, TurnError> {
    let stream_tx = match stream_tx {
        Some(stream_tx) => stream_tx,
        None => {
            let (stream_tx, _stream_rx) = broadcast::channel(DEFAULT_TURN_CHANNEL_CAPACITY);
            stream_tx
        }
    };
    let system_prompt =
        (!agent_record.system_prompt.is_empty()).then_some(agent_record.system_prompt.as_str());

    let ctx = TurnContext {
        thread_id: originating_thread_id,
        turn_number,
        started_at,
        tools: tools.as_ref(),
        hooks: hooks.as_ref(),
        provider: provider.as_ref(),
        config: &config,
        agent_record: agent_record.as_ref(),
        stream_tx: &stream_tx,
        thread_event_tx: &thread_event_tx,
        cancellation: &cancellation,
    };

    tracing::info!(
        thread_id = %thread_id,
        turn_number = %turn_number,
        tool_count = %tools.len(),
        hook_count = %hooks.len(),
        "Turn execution started"
    );

    let result = execute_loop(
        &ctx,
        system_prompt,
        history,
        messages,
        compactor.as_deref(),
    )
    .await;

    tracing::info!(
        thread_id = %thread_id,
        turn_number = %turn_number,
        result = ?result.as_ref().map(|_| "ok"),
        "Turn execution completed"
    );

    result
}

#[cfg(test)]
impl Turn {
    #[allow(clippy::too_many_arguments)]
    fn new(
        turn_number: u32,
        thread_id: String,
        originating_thread_id: ThreadId,
        history: Arc<Vec<ChatMessage>>,
        messages: Vec<ChatMessage>,
        tools: Arc<Vec<Arc<dyn NamedTool>>>,
        hooks: Arc<Vec<Arc<dyn HookHandler>>>,
        started_at: DateTime<Utc>,
        provider: Arc<dyn LlmProvider>,
        config: TurnConfig,
        agent_record: Arc<AgentRecord>,
        stream_tx: broadcast::Sender<TurnStreamEvent>,
        thread_event_tx: broadcast::Sender<ThreadEvent>,
        cancellation: TurnCancellation,
        compactor: Option<Arc<dyn Compactor>>,
    ) -> Self {
        Self {
            id: generate_turn_id(&thread_id, turn_number),
            turn_number,
            thread_id,
            originating_thread_id,
            history,
            messages,
            tools,
            hooks,
            started_at,
            provider,
            config,
            agent_record,
            stream_tx,
            thread_event_tx,
            cancellation,
            compactor,
        }
    }

    async fn execute_internal(self) -> Result<TurnRecord, TurnError> {
        let Turn {
            turn_number,
            thread_id,
            originating_thread_id,
            history,
            messages,
            tools,
            hooks,
            started_at,
            provider,
            config,
            agent_record,
            stream_tx,
            thread_event_tx,
            cancellation,
            compactor,
            ..
        } = self;
        execute_thread_turn(
            turn_number,
            thread_id,
            originating_thread_id,
            history,
            messages,
            tools,
            hooks,
            started_at,
            provider,
            config,
            agent_record,
            Some(stream_tx),
            thread_event_tx,
            cancellation,
            compactor,
        )
        .await
    }

    /// Execute the turn and return the completed turn record.
    pub async fn execute(self) -> Result<TurnRecord, TurnError> {
        self.execute_internal().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use crate::TurnRecordKind;
    use crate::compact::{CompactResult, Compactor};
    use crate::error::CompactError;
    use crate::thread::ThreadBuilder;
    use argus_protocol::llm::{CompletionRequest, CompletionResponse, LlmError};
    use argus_protocol::tool::{NamedTool, ToolError};
    use argus_protocol::{
        AgentId, AgentType, HookAction, HookEvent, HookHandler, HookRegistry, ProviderId,
        SessionId, ThreadEvent, TokenUsage, ToolExecutionContext, ToolHookContext,
    };
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use tokio::sync::{Notify, broadcast, oneshot};

    #[test]
    fn test_generate_turn_id() {
        let id = generate_turn_id("thread-123", 5);
        assert_eq!(id, "thread-123-turn-5");
    }

    #[test]
    fn test_turn_debug_format() {
        let (stream_tx, _): (broadcast::Sender<TurnStreamEvent>, _) = broadcast::channel(256);
        let (thread_event_tx, _): (broadcast::Sender<ThreadEvent>, _) = broadcast::channel(256);
        let _ = (stream_tx, thread_event_tx);
    }

    #[derive(Debug)]
    struct SequencedProvider {
        responses: Mutex<Vec<CompletionResponse>>,
    }

    impl SequencedProvider {
        fn new(responses: Vec<CompletionResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
            }
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
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                return Ok(CompletionResponse {
                    content: Some("done".to_string()),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    input_tokens: 1,
                    output_tokens: 1,
                    finish_reason: FinishReason::Stop,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                });
            }
            Ok(responses.remove(0))
        }
    }

    #[derive(Debug)]
    struct BlockingFirstResponseProvider {
        responses: Mutex<Vec<CompletionResponse>>,
        first_call_started: Mutex<Option<oneshot::Sender<()>>>,
        release_first_call: Arc<Notify>,
        gate_first_call: Mutex<bool>,
    }

    #[derive(Debug)]
    struct SmallWindowSequencedProvider {
        responses: Mutex<Vec<CompletionResponse>>,
        context_window: u32,
    }

    impl SmallWindowSequencedProvider {
        fn new(responses: Vec<CompletionResponse>, context_window: u32) -> Self {
            Self {
                responses: Mutex::new(responses),
                context_window,
            }
        }
    }

    #[async_trait]
    impl LlmProvider for SmallWindowSequencedProvider {
        fn model_name(&self) -> &str {
            "small-window-sequenced"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            let mut responses = self.responses.lock().unwrap();
            Ok(responses.remove(0))
        }

        fn context_window(&self) -> u32 {
            self.context_window
        }
    }

    impl BlockingFirstResponseProvider {
        fn new(
            responses: Vec<CompletionResponse>,
            first_call_started: oneshot::Sender<()>,
            release_first_call: Arc<Notify>,
        ) -> Self {
            Self {
                responses: Mutex::new(responses),
                first_call_started: Mutex::new(Some(first_call_started)),
                release_first_call,
                gate_first_call: Mutex::new(true),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for BlockingFirstResponseProvider {
        fn model_name(&self) -> &str {
            "blocking-first"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            let should_gate = {
                let mut gate_first_call = self.gate_first_call.lock().unwrap();
                let should_gate = *gate_first_call;
                *gate_first_call = false;
                should_gate
            };

            if should_gate {
                if let Some(sender) = self.first_call_started.lock().unwrap().take() {
                    let _ = sender.send(());
                }
                self.release_first_call.notified().await;
            }

            let mut responses = self.responses.lock().unwrap();
            Ok(responses.remove(0))
        }
    }

    struct VersionedTool {
        version: &'static str,
    }

    #[async_trait]
    impl NamedTool for VersionedTool {
        fn name(&self) -> &str {
            "late_echo"
        }

        fn definition(&self) -> argus_protocol::llm::ToolDefinition {
            argus_protocol::llm::ToolDefinition {
                name: "late_echo".to_string(),
                description: format!("Return the current tool version {}", self.version),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            }
        }

        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: Arc<ToolExecutionContext>,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({ "version": self.version }))
        }
    }

    struct ContinueOnceTurnEndHook {
        used: Mutex<bool>,
    }

    impl ContinueOnceTurnEndHook {
        fn new() -> Self {
            Self {
                used: Mutex::new(false),
            }
        }
    }

    #[async_trait]
    impl HookHandler for ContinueOnceTurnEndHook {
        async fn on_tool_event(&self, ctx: &ToolHookContext) -> HookAction {
            if ctx.event == HookEvent::TurnEnd {
                let mut used = self.used.lock().unwrap();
                if !*used {
                    *used = true;
                    return HookAction::ContinueWithMessage("continue".to_string());
                }
            }
            HookAction::Continue
        }
    }

    struct AlwaysContinueTurnEndHook;

    #[async_trait]
    impl HookHandler for AlwaysContinueTurnEndHook {
        async fn on_tool_event(&self, ctx: &ToolHookContext) -> HookAction {
            if ctx.event == HookEvent::TurnEnd {
                return HookAction::ContinueWithMessage("continue".to_string());
            }
            HookAction::Continue
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

    #[derive(Debug, Clone)]
    struct TurnCompactorCall {
        messages: Vec<String>,
        turn_messages: Vec<String>,
    }

    #[derive(Debug)]
    struct RecordingTurnCompactor {
        calls: Arc<Mutex<Vec<TurnCompactorCall>>>,
        results: CompactResultQueue,
    }

    type CompactResultQueue = Arc<Mutex<VecDeque<Result<Option<CompactResult>, CompactError>>>>;

    #[async_trait]
    impl Compactor for RecordingTurnCompactor {
        async fn compact(
            &self,
            messages: &[ChatMessage],
            _token_count: u32,
        ) -> Result<Option<CompactResult>, CompactError> {
            self.calls.lock().unwrap().push(TurnCompactorCall {
                messages: messages
                    .iter()
                    .map(|message| message.content.clone())
                    .collect(),
                turn_messages: messages
                    .iter()
                    .filter(|message| message.role != argus_protocol::llm::Role::System)
                    .map(|message| message.content.clone())
                    .collect(),
            });
            self.results.lock().unwrap().pop_front().unwrap_or(Ok(None))
        }

        fn name(&self) -> &'static str {
            "recording-turn"
        }
    }
    fn make_agent_record() -> Arc<AgentRecord> {
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

    fn make_agent_record_with_tools(tool_names: Vec<&str>) -> Arc<AgentRecord> {
        Arc::new(AgentRecord {
            tool_names: tool_names.into_iter().map(str::to_string).collect(),
            ..(*make_agent_record()).clone()
        })
    }

    fn make_turn_channels() -> (
        broadcast::Sender<TurnStreamEvent>,
        broadcast::Sender<ThreadEvent>,
    ) {
        (broadcast::channel(256).0, broadcast::channel(256).0)
    }

    fn make_turn(
        provider: Arc<dyn LlmProvider>,
        hooks: Vec<Arc<dyn HookHandler>>,
        max_iterations: u32,
    ) -> Turn {
        make_turn_with(
            provider,
            vec![],
            hooks,
            max_iterations,
            TurnCancellation::default(),
            None,
            vec![ChatMessage::user("start")],
        )
    }

    fn make_turn_with(
        provider: Arc<dyn LlmProvider>,
        tools: Vec<Arc<dyn NamedTool>>,
        hooks: Vec<Arc<dyn HookHandler>>,
        max_iterations: u32,
        cancellation: TurnCancellation,
        compactor: Option<Arc<dyn Compactor>>,
        messages: Vec<ChatMessage>,
    ) -> Turn {
        let (stream_tx, thread_event_tx) = make_turn_channels();
        Turn::new(
            1,
            "thread-test".to_string(),
            ThreadId::new(),
            Arc::new(Vec::new()),
            messages,
            Arc::new(tools),
            Arc::new(hooks),
            Utc::now(),
            provider,
            TurnConfig {
                max_iterations: Some(max_iterations),
                ..TurnConfig::default()
            },
            make_agent_record(),
            stream_tx,
            thread_event_tx,
            cancellation,
            compactor,
        )
    }

    fn build_thread_with_live_sources(
        provider: Arc<dyn LlmProvider>,
        agent_record: Arc<AgentRecord>,
        tool_manager: Arc<argus_tool::ToolManager>,
        hooks: Arc<HookRegistry>,
    ) -> crate::thread::Thread {
        let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
        ThreadBuilder::new()
            .provider(provider)
            .compactor(compactor)
            .agent_record(agent_record)
            .session_id(SessionId::new())
            .tool_manager(tool_manager)
            .hooks(hooks)
            .build()
            .expect("thread should build")
    }

    #[tokio::test]
    async fn shared_history_turn_sees_late_tool_overwrite_after_thread_creation() {
        let provider = Arc::new(SequencedProvider::new(vec![
            CompletionResponse {
                content: Some("tool call".to_string()),
                reasoning_content: None,
                tool_calls: vec![argus_protocol::llm::ToolCall {
                    id: "call-late-tool".to_string(),
                    name: "late_echo".to_string(),
                    arguments: serde_json::json!({}),
                }],
                input_tokens: 1,
                output_tokens: 1,
                finish_reason: FinishReason::ToolUse,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
            CompletionResponse {
                content: Some("done".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 1,
                output_tokens: 1,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ]));
        let tool_manager = Arc::new(argus_tool::ToolManager::new());
        tool_manager.register(Arc::new(VersionedTool { version: "v1" }));
        let hooks = Arc::new(HookRegistry::new());
        let mut thread = build_thread_with_live_sources(
            provider,
            make_agent_record_with_tools(vec!["late_echo"]),
            Arc::clone(&tool_manager),
            hooks,
        );

        tool_manager.register(Arc::new(VersionedTool { version: "v2" }));

        let record = thread
            .execute_turn("start".to_string(), None, TurnCancellation::default())
            .await
            .expect("turn should execute");

        let tool_results = record
            .messages
            .iter()
            .filter(|message| message.role == argus_protocol::llm::Role::Tool)
            .map(|message| message.content.clone())
            .collect::<Vec<_>>();

        assert_eq!(tool_results, vec![r#"{"version":"v2"}"#.to_string()]);
    }

    #[tokio::test]
    async fn shared_history_turn_sees_late_hook_registration_after_thread_creation() {
        let provider = Arc::new(SequencedProvider::new(vec![
            CompletionResponse {
                content: Some("first".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 1,
                output_tokens: 1,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
            CompletionResponse {
                content: Some("second".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 1,
                output_tokens: 1,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ]));
        let tool_manager = Arc::new(argus_tool::ToolManager::new());
        let hooks = Arc::new(HookRegistry::new());
        let mut thread = build_thread_with_live_sources(
            provider,
            make_agent_record(),
            tool_manager,
            Arc::clone(&hooks),
        );

        hooks.register(HookEvent::TurnEnd, Arc::new(ContinueOnceTurnEndHook::new()));

        let record = thread
            .execute_turn("start".to_string(), None, TurnCancellation::default())
            .await
            .expect("turn should execute");

        assert!(
            record
                .messages
                .iter()
                .any(|message| message.content == "second")
        );
    }

    #[tokio::test]
    async fn shared_history_active_turn_does_not_see_late_hook_registration() {
        let (first_call_started_tx, first_call_started_rx) = oneshot::channel();
        let release_first_call = Arc::new(Notify::new());
        let provider = Arc::new(BlockingFirstResponseProvider::new(
            vec![
                CompletionResponse {
                    content: Some("first".to_string()),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    input_tokens: 1,
                    output_tokens: 1,
                    finish_reason: FinishReason::Stop,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                },
                CompletionResponse {
                    content: Some("second".to_string()),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    input_tokens: 1,
                    output_tokens: 1,
                    finish_reason: FinishReason::Stop,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                },
            ],
            first_call_started_tx,
            Arc::clone(&release_first_call),
        ));
        let tool_manager = Arc::new(argus_tool::ToolManager::new());
        let hooks = Arc::new(HookRegistry::new());
        let thread = build_thread_with_live_sources(
            provider,
            make_agent_record(),
            tool_manager,
            Arc::clone(&hooks),
        );
        let thread = Arc::new(tokio::sync::Mutex::new(thread));
        let execution = {
            let thread = Arc::clone(&thread);
            tokio::spawn(async move {
                thread
                    .lock()
                    .await
                    .execute_turn("start".to_string(), None, TurnCancellation::default())
                    .await
            })
        };

        first_call_started_rx
            .await
            .expect("turn should start the first provider call");

        hooks.register(HookEvent::TurnEnd, Arc::new(ContinueOnceTurnEndHook::new()));
        release_first_call.notify_waiters();

        let record = execution
            .await
            .expect("turn task should complete")
            .expect("turn should execute");

        assert!(
            record
                .messages
                .iter()
                .all(|message| message.content != "second")
        );
    }

    #[tokio::test]
    async fn turn_end_continue_with_message_triggers_next_iteration() {
        let provider = Arc::new(SequencedProvider::new(vec![
            CompletionResponse {
                content: Some("first".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 1,
                output_tokens: 1,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
            CompletionResponse {
                content: Some("second".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 1,
                output_tokens: 1,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ]));

        let turn = make_turn(provider, vec![Arc::new(ContinueOnceTurnEndHook::new())], 5);
        let record = turn.execute().await.expect("turn should succeed");

        assert!(record.messages.iter().any(|m| m.content == "first"));
        assert!(record.messages.iter().any(|m| m.content == "continue"));
        assert!(record.messages.iter().any(|m| m.content == "second"));
    }

    #[tokio::test]
    async fn execute_returns_user_turn_when_no_internal_compaction_happens() {
        let provider = Arc::new(SequencedProvider::new(vec![CompletionResponse {
            content: Some("done".to_string()),
            reasoning_content: None,
            tool_calls: Vec::new(),
            input_tokens: 2,
            output_tokens: 1,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }]));

        let turn = make_turn(provider, vec![], 5);
        let record = turn.execute().await.expect("turn should succeed");

        assert!(matches!(record.kind, TurnRecordKind::UserTurn));
        assert_eq!(record.turn_number, 1);
        assert_eq!(record.token_usage.total_tokens, 3);
        let final_contents: Vec<_> = record
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect();
        assert_eq!(final_contents, vec!["start", "done"]);
    }

    #[tokio::test]
    async fn execute_loop_turn_compactor_recomputes_from_latest_turn_messages() {
        let provider = Arc::new(SmallWindowSequencedProvider::new(
            vec![
                CompletionResponse {
                    content: Some("first".to_string()),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    input_tokens: 1,
                    output_tokens: 1,
                    finish_reason: FinishReason::Stop,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                },
                CompletionResponse {
                    content: Some("second".to_string()),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    input_tokens: 1,
                    output_tokens: 1,
                    finish_reason: FinishReason::Stop,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                },
            ],
            4,
        ));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let compactor = Arc::new(RecordingTurnCompactor {
            calls: Arc::clone(&calls),
            results: Arc::new(Mutex::new(VecDeque::from(vec![Ok(None), Ok(None)]))),
        });
        let turn = make_turn_with(
            provider,
            vec![],
            vec![Arc::new(ContinueOnceTurnEndHook::new())],
            5,
            TurnCancellation::default(),
            Some(compactor),
            vec![ChatMessage::user(
                "this input is long enough to trigger turn compaction checks",
            )],
        );

        let record = turn.execute().await.expect("turn should succeed");

        assert!(matches!(record.kind, TurnRecordKind::UserTurn));
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(
            calls[0]
                .messages
                .iter()
                .any(|content| content == "You are a test agent."),
            "compactor should receive the full materialized message list, including the prompt"
        );
        assert!(
            calls[1]
                .turn_messages
                .iter()
                .any(|content| content == "continue"),
            "second turn-compactor pass should see the injected continuation message"
        );
        assert!(
            calls[1]
                .turn_messages
                .iter()
                .any(|content| content == "first"),
            "second turn-compactor pass should see the prior assistant response"
        );
    }

    #[tokio::test]
    async fn execute_returns_turn_checkpoint_when_internal_compaction_happens() {
        let provider = Arc::new(SmallWindowSequencedProvider::new(
            vec![
                CompletionResponse {
                    content: Some("first".to_string()),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    input_tokens: 1,
                    output_tokens: 1,
                    finish_reason: FinishReason::Stop,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                },
                CompletionResponse {
                    content: Some("second".to_string()),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    input_tokens: 1,
                    output_tokens: 1,
                    finish_reason: FinishReason::Stop,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                },
            ],
            4,
        ));
        let compactor = Arc::new(RecordingTurnCompactor {
            calls: Arc::new(Mutex::new(Vec::new())),
            results: Arc::new(Mutex::new(VecDeque::from(vec![
                Ok(Some(CompactResult {
                    messages: vec![
                        ChatMessage::user("latest input"),
                        ChatMessage::user("summary one"),
                    ],
                    token_usage: TokenUsage {
                        input_tokens: 4,
                        output_tokens: 1,
                        total_tokens: 5,
                    },
                })),
                Ok(Some(CompactResult {
                    messages: vec![
                        ChatMessage::user("latest input"),
                        ChatMessage::user("summary two"),
                    ],
                    token_usage: TokenUsage {
                        input_tokens: 5,
                        output_tokens: 2,
                        total_tokens: 7,
                    },
                })),
            ]))),
        });
        let turn = make_turn_with(
            provider,
            vec![],
            vec![Arc::new(ContinueOnceTurnEndHook::new())],
            5,
            TurnCancellation::default(),
            Some(compactor),
            vec![ChatMessage::user(
                "this input is long enough to trigger turn compaction checks",
            )],
        );

        let record = turn.execute().await.expect("turn should succeed");

        assert!(matches!(record.kind, TurnRecordKind::TurnCheckpoint));
        assert_eq!(record.turn_number, 1);
        assert_eq!(record.token_usage.total_tokens, 2);
        let final_contents: Vec<_> = record
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect();
        assert_eq!(
            final_contents,
            vec!["latest input", "summary two", "second"]
        );
    }

    #[tokio::test]
    async fn execute_loop_continues_when_turn_compaction_fails() {
        let provider = Arc::new(SmallWindowSequencedProvider::new(
            vec![CompletionResponse {
                content: Some("done".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 1,
                output_tokens: 1,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            }],
            4,
        ));
        let compactor = Arc::new(RecordingTurnCompactor {
            calls: Arc::new(Mutex::new(Vec::new())),
            results: Arc::new(Mutex::new(VecDeque::from(vec![Err(
                CompactError::Failed {
                    reason: "boom".to_string(),
                },
            )]))),
        });
        let turn = make_turn_with(
            provider,
            vec![],
            vec![],
            5,
            TurnCancellation::default(),
            Some(compactor),
            vec![ChatMessage::user(
                "this input is long enough to trigger turn compaction checks",
            )],
        );

        let record = turn.execute().await.expect("turn should still succeed");

        assert!(matches!(record.kind, TurnRecordKind::UserTurn));
        let final_contents: Vec<_> = record
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect();
        assert!(final_contents.contains(&"done"));
    }

    #[tokio::test]
    async fn turn_end_always_continue_hits_max_iterations() {
        let provider = Arc::new(SequencedProvider::new(vec![CompletionResponse {
            content: Some("loop".to_string()),
            reasoning_content: None,
            tool_calls: Vec::new(),
            input_tokens: 1,
            output_tokens: 1,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }]));

        let turn = make_turn(provider, vec![Arc::new(AlwaysContinueTurnEndHook)], 2);
        let result = turn.execute().await;

        assert!(matches!(result, Err(TurnError::MaxIterationsReached(2))));
    }

    #[tokio::test]
    async fn execute_keeps_only_latest_token_usage_from_provider_responses() {
        let provider = Arc::new(SequencedProvider::new(vec![
            CompletionResponse {
                content: Some("first".to_string()),
                reasoning_content: None,
                tool_calls: vec![argus_protocol::llm::ToolCall {
                    id: "call-123".to_string(),
                    name: "echo".to_string(),
                    arguments: serde_json::json!({"message": "test message"}),
                }],
                input_tokens: 11,
                output_tokens: 7,
                finish_reason: FinishReason::ToolUse,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
            CompletionResponse {
                content: Some("done".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 17,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ]));

        let turn = make_turn(provider, vec![], 5);
        let record = turn.execute().await.expect("turn should succeed");

        assert_eq!(record.token_usage.input_tokens, 17);
        assert_eq!(record.token_usage.output_tokens, 5);
        assert_eq!(record.token_usage.total_tokens, 22);
    }
}
