//! Turn execution logic.
//!
//! This module implements the core execution loop for turn-based LLM conversations
//! with tool support. It handles the LLM -> Tool -> LLM cycle with parallel tool
//! execution and hook integration.

use std::sync::Arc;

use futures_util::{future::join_all, StreamExt};
use tokio::sync::broadcast;
use tokio::time::{error::Elapsed, timeout};

use crate::llm::{
    ChatMessage, FinishReason, LlmStreamEvent, ToolCall, ToolCompletionRequest,
    ToolCompletionResponse, ToolDefinition,
};
use crate::tool::ToolManager;

use super::hooks::{BeforeCallLLMContext, HookEvent, HookRegistry, ToolHookContext};
use super::{TokenUsage, TurnConfig, TurnError, TurnInput, TurnOutput, TurnStreamEvent};

/// Execution mode for turn processing.
#[derive(Debug, Clone, Copy)]
pub enum ExecutionMode {
    /// Non-streaming mode (wait for complete response).
    NonStreaming,
    /// Streaming mode (emit real-time events).
    Streaming,
}

/// Prepares tool definitions and optionally adds max_tool_calls system prompt.
///
/// This function:
/// 1. Resolves tool definitions from the tool manager
/// 2. Prepends a system message about max_tool_calls if configured
fn prepare_tools(
    messages: &mut Vec<ChatMessage>,
    tool_manager: &Arc<ToolManager>,
    tool_ids: &[String],
    max_tool_calls: Option<u32>,
) -> Vec<ToolDefinition> {
    // Resolve tool definitions from tool_manager
    let tools: Vec<ToolDefinition> = tool_ids
        .iter()
        .filter_map(|id| tool_manager.get(id))
        .map(|tool| tool.definition())
        .collect();

    // Add system message about max_tool_calls if configured and tools are available
    if let Some(max) = max_tool_calls
        && !tools.is_empty()
    {
        let system_content = format!(
            "IMPORTANT: You can only call at most {} tool(s) per response. \
            If you need to call multiple tools, please proceed step by step - \
            call tools one at a time and wait for the results before calling the next tool.",
            max
        );
        // Prepend system message to messages
        messages.insert(0, ChatMessage::system(system_content));
    }

    tools
}

/// Result of processing an LLM response's finish_reason.
enum NextAction {
    /// Turn is complete, return the output.
    Return(TurnOutput),
    /// Continue with tool execution.
    ContinueWithTools {
        tool_calls: Vec<ToolCall>,
        #[allow(dead_code)]
        content: Option<String>,
    },
    /// Context length exceeded.
    LengthExceeded,
}

/// Processes the LLM response and determines the next action.
fn process_finish_reason(
    response: ToolCompletionResponse,
    messages: &mut Vec<ChatMessage>,
    token_usage: &mut TokenUsage,
    max_tool_calls: Option<u32>,
) -> NextAction {
    // Track token usage
    token_usage.input_tokens += response.input_tokens;
    token_usage.output_tokens += response.output_tokens;
    token_usage.total_tokens += response.input_tokens + response.output_tokens;

    match response.finish_reason {
        FinishReason::Stop => {
            // Add assistant message to history
            if let Some(content) = &response.content
                && !content.is_empty()
            {
                messages.push(ChatMessage::assistant(content.clone()));
            }

            NextAction::Return(TurnOutput {
                messages: std::mem::take(messages),
                token_usage: token_usage.clone(),
            })
        }
        FinishReason::ToolUse => {
            // Limit tool calls based on max_tool_calls config
            let tool_calls: Vec<ToolCall> = match max_tool_calls {
                Some(max) if response.tool_calls.len() > max as usize => {
                    tracing::debug!(
                        requested = response.tool_calls.len(),
                        max_allowed = max,
                        "Limiting tool calls per iteration"
                    );
                    response.tool_calls.into_iter().take(max as usize).collect()
                }
                _ => response.tool_calls,
            };

            // Add assistant message with tool_calls to history
            let assistant_msg =
                ChatMessage::assistant_with_tool_calls(response.content.clone(), tool_calls.clone());
            messages.push(assistant_msg);

            NextAction::ContinueWithTools {
                tool_calls,
                content: response.content,
            }
        }
        FinishReason::Length => NextAction::LengthExceeded,
        FinishReason::ContentFilter | FinishReason::Unknown => {
            // For content filter or unknown reasons, return what we have
            if let Some(content) = &response.content
                && !content.is_empty()
            {
                messages.push(ChatMessage::assistant(content.clone()));
            }

            NextAction::Return(TurnOutput {
                messages: std::mem::take(messages),
                token_usage: token_usage.clone(),
            })
        }
    }
}

/// Calls the LLM in streaming mode, accumulating the response.
///
/// Falls back to non-streaming if the provider doesn't support streaming.
async fn call_llm_streaming(
    provider: &Arc<dyn crate::llm::LlmProvider>,
    request: ToolCompletionRequest,
    stream_sender: Option<&broadcast::Sender<TurnStreamEvent>>,
) -> Result<ToolCompletionResponse, TurnError> {
    match provider.stream_complete_with_tools(request.clone()).await {
        Ok(mut stream) => {
            let mut accumulator = StreamingAccumulator::new();
            while let Some(event_result) = stream.next().await {
                let event = event_result.map_err(TurnError::LlmFailed)?;
                // Forward to stream_sender
                if let Some(sender) = stream_sender {
                    let _ = sender.send(TurnStreamEvent::LlmEvent(event.clone()));
                }
                accumulator.process(event);
            }
            Ok(accumulator.into_response())
        }
        Err(crate::llm::LlmError::UnsupportedCapability { .. }) => {
            // Fallback to non-streaming
            tracing::debug!("Provider doesn't support streaming, using non-streaming fallback");
            provider
                .complete_with_tools(request)
                .await
                .map_err(TurnError::LlmFailed)
        }
        Err(e) => Err(TurnError::LlmFailed(e)),
    }
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
        }
    }

    fn into_response(self) -> ToolCompletionResponse {
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

        ToolCompletionResponse {
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

/// Executes a single turn in a conversation (non-streaming).
///
/// This function runs the main LLM loop:
/// 1. Sends messages to the LLM with available tools
/// 2. If the LLM responds with text (Stop), returns the output
/// 3. If the LLM requests tool calls (ToolUse), executes tools in parallel
///    and continues the loop with tool results
/// 4. Continues until max_iterations is reached or the LLM stops
///
/// # Errors
///
/// Returns `TurnError` for:
/// - LLM failures
/// - Max iterations exceeded
/// - Tool call blocked by hooks
pub async fn execute_turn(input: TurnInput, config: TurnConfig) -> Result<TurnOutput, TurnError> {
    execute_turn_with_mode(input, config, ExecutionMode::NonStreaming).await
}

/// Executes a single turn in a conversation (streaming).
///
/// Same as `execute_turn` but emits real-time events through the `stream_sender`
/// in `TurnInput`. If the provider doesn't support streaming, falls back to
/// non-streaming mode.
///
/// # Errors
///
/// Returns `TurnError` for the same conditions as `execute_turn`.
pub async fn execute_turn_streaming(
    input: TurnInput,
    config: TurnConfig,
) -> Result<TurnOutput, TurnError> {
    execute_turn_with_mode(input, config, ExecutionMode::Streaming).await
}

/// Unified turn execution with configurable mode.
///
/// This is the core implementation that both `execute_turn` and `execute_turn_streaming`
/// delegate to.
async fn execute_turn_with_mode(
    input: TurnInput,
    config: TurnConfig,
    mode: ExecutionMode,
) -> Result<TurnOutput, TurnError> {
    let mut messages = input.messages;
    let provider = input.provider;
    let tool_manager = input.tool_manager;
    let tool_ids = input.tool_ids;
    let hooks = input.hooks;
    let thread_event_sender = input.thread_event_sender;
    let thread_id = input.thread_id;
    let stream_sender = input.stream_sender;

    // Prepare tools and system message
    let tools = prepare_tools(&mut messages, &tool_manager, &tool_ids, config.max_tool_calls);

    let max_iterations = config.max_iterations.unwrap_or(50);
    let tool_timeout_secs = config.tool_timeout_secs.unwrap_or(120);

    let mut token_usage = TokenUsage::default();

    for iteration in 0..max_iterations {
        // Fire BeforeCallLLM hook (can modify messages/tools or block)
        if let Some(ref registry) = hooks {
            let ctx = BeforeCallLLMContext {
                messages: messages.clone(),
                tools: tools.clone(),
                iteration,
            };
            let result = registry
                .fire_before_call_llm(&ctx)
                .await
                .map_err(|reason| TurnError::LlmCallBlocked { reason })?;

            // Apply any modifications from hooks
            if let Some(modified_messages) = result.messages {
                messages = modified_messages;
            }
            if let Some(_modified_tools) = result.tools {
                // TODO: Apply tool modifications for this iteration
            }
        }

        // Build the request with current messages and tools
        let request = ToolCompletionRequest::new(messages.clone(), tools.clone());

        // Call the LLM based on mode
        let response = match mode {
            ExecutionMode::Streaming => {
                call_llm_streaming(&provider, request, stream_sender.as_ref()).await?
            }
            ExecutionMode::NonStreaming => provider
                .complete_with_tools(request)
                .await
                .map_err(TurnError::LlmFailed)?,
        };

        // Process response
        match process_finish_reason(response, &mut messages, &mut token_usage, config.max_tool_calls)
        {
            NextAction::Return(output) => {
                // Fire TurnEnd hook
                if let Some(ref registry) = hooks {
                    let ctx = ToolHookContext {
                        event: HookEvent::TurnEnd,
                        tool_name: String::new(),
                        tool_call_id: String::new(),
                        tool_input: serde_json::Value::Null,
                        tool_result: None,
                        error: None,
                        tool_manager: Some(Arc::clone(&tool_manager)),
                        thread_event_sender: thread_event_sender.clone(),
                        thread_id,
                        turn_number: Some(iteration),
                    };
                    // TurnEnd is observe-only, ignore errors
                    let _ = registry.fire_tool_event(&ctx).await;
                }

                return Ok(output);
            }
            NextAction::ContinueWithTools { tool_calls, .. } => {
                // Execute tools in parallel with streaming support
                let tool_results = execute_tools_parallel(
                    tool_calls,
                    Arc::clone(&tool_manager),
                    hooks.as_ref().map(|v| v.as_ref()),
                    tool_timeout_secs,
                    thread_event_sender.clone(),
                    thread_id,
                    iteration,
                    stream_sender.clone(),
                )
                .await;

                // Add tool result messages to history
                for result in tool_results {
                    messages.push(ChatMessage::tool_result(
                        result.tool_call_id,
                        result.name,
                        result.content,
                    ));
                }

                // Continue the loop with updated messages
            }
            NextAction::LengthExceeded => {
                return Err(TurnError::ContextLengthExceeded(
                    (token_usage.input_tokens + token_usage.output_tokens) as usize,
                ));
            }
        }
    }

    // Max iterations reached
    Err(TurnError::MaxIterationsReached(max_iterations))
}

/// Result of a tool execution.
struct ToolExecutionResult {
    tool_call_id: String,
    name: String,
    content: String,
}

/// Executes multiple tool calls in parallel.
///
/// Each tool call is executed with:
/// 1. BeforeToolCall hook (can block execution)
/// 2. Tool execution with timeout
/// 3. AfterToolCall hook (observe-only)
///
/// Tool execution failures are captured as error messages, not propagated.
async fn execute_tools_parallel(
    tool_calls: Vec<ToolCall>,
    tool_manager: Arc<ToolManager>,
    hooks: Option<&HookRegistry>,
    tool_timeout_secs: u64,
    thread_event_sender: Option<broadcast::Sender<crate::protocol::ThreadEvent>>,
    thread_id: Option<crate::protocol::ThreadId>,
    turn_number: u32,
    stream_sender: Option<broadcast::Sender<TurnStreamEvent>>,
) -> Vec<ToolExecutionResult> {
    let futures: Vec<_> = tool_calls
        .into_iter()
        .map(|tool_call| {
            execute_single_tool(
                tool_call,
                Arc::clone(&tool_manager),
                hooks,
                tool_timeout_secs,
                thread_event_sender.clone(),
                thread_id,
                turn_number,
                stream_sender.clone(),
            )
        })
        .collect();

    join_all(futures).await
}

/// Executes a single tool call with hooks and timeout.
async fn execute_single_tool(
    tool_call: ToolCall,
    tool_manager: Arc<ToolManager>,
    hooks: Option<&HookRegistry>,
    tool_timeout_secs: u64,
    thread_event_sender: Option<broadcast::Sender<crate::protocol::ThreadEvent>>,
    thread_id: Option<crate::protocol::ThreadId>,
    turn_number: u32,
    stream_sender: Option<broadcast::Sender<TurnStreamEvent>>,
) -> ToolExecutionResult {
    use crate::protocol::ThreadEvent;

    let tool_call_id = tool_call.id.clone();
    let tool_name = tool_call.name.clone();
    let tool_input = tool_call.arguments.clone();

    // Fire BeforeToolCall hook
    if let Some(registry) = hooks {
        let ctx = ToolHookContext {
            event: HookEvent::BeforeToolCall,
            tool_name: tool_name.clone(),
            tool_call_id: tool_call_id.clone(),
            tool_input: tool_input.clone(),
            tool_result: None,
            error: None,
            tool_manager: Some(Arc::clone(&tool_manager)),
            thread_event_sender: thread_event_sender.clone(),
            thread_id,
            turn_number: Some(turn_number),
        };
        if let Err(reason) = registry.fire_tool_event(&ctx).await {
            // Hook blocked the tool call
            let content = format!("Tool call blocked: {}", reason);

            // Fire AfterToolCall hook with error
            let after_ctx = ToolHookContext {
                event: HookEvent::AfterToolCall,
                tool_name: tool_name.clone(),
                tool_call_id: tool_call_id.clone(),
                tool_input,
                tool_result: None,
                error: Some(reason),
                tool_manager: Some(Arc::clone(&tool_manager)),
                thread_event_sender: thread_event_sender.clone(),
                thread_id,
                turn_number: Some(turn_number),
            };
            let _ = registry.fire_tool_event(&after_ctx).await;

            return ToolExecutionResult {
                tool_call_id,
                name: tool_name,
                content,
            };
        }
    }

    // Send ToolStarted events
    if let Some(ref sender) = thread_event_sender
        && let Some(tid) = thread_id
    {
        let _ = sender.send(ThreadEvent::ToolStarted {
            thread_id: tid,
            turn_number,
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            arguments: tool_input.clone(),
        });
    }
    if let Some(ref sender) = stream_sender {
        let _ = sender.send(TurnStreamEvent::ToolStarted {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            arguments: tool_input.clone(),
        });
    }

    // Execute the tool with timeout
    let timeout_duration = std::time::Duration::from_secs(tool_timeout_secs);
    let execute_future = tool_manager.execute(&tool_name, tool_input.clone());

    let result = match timeout(timeout_duration, execute_future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(e)) => Err(e.to_string()),
        Err(Elapsed { .. }) => Err(format!(
            "Tool execution timed out after {}s",
            tool_timeout_secs
        )),
    };

    // Send ToolCompleted events
    if let Some(ref sender) = thread_event_sender
        && let Some(tid) = thread_id
    {
        let event_result = match &result {
            Ok(value) => Ok(value.clone()),
            Err(e) => Err(e.clone()),
        };
        let _ = sender.send(ThreadEvent::ToolCompleted {
            thread_id: tid,
            turn_number,
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            result: event_result,
        });
    }
    if let Some(ref sender) = stream_sender {
        let event_result = match &result {
            Ok(value) => Ok(value.clone()),
            Err(e) => Err(e.clone()),
        };
        let _ = sender.send(TurnStreamEvent::ToolCompleted {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            result: event_result,
        });
    }

    // Fire AfterToolCall hook
    if let Some(registry) = hooks {
        let (tool_result, error) = match &result {
            Ok(value) => (Some(value.clone()), None),
            Err(e) => (None, Some(e.clone())),
        };
        let ctx = ToolHookContext {
            event: HookEvent::AfterToolCall,
            tool_name: tool_name.clone(),
            tool_call_id: tool_call_id.clone(),
            tool_input,
            tool_result,
            error,
            tool_manager: Some(Arc::clone(&tool_manager)),
            thread_event_sender: thread_event_sender.clone(),
            thread_id,
            turn_number: Some(turn_number),
        };
        let _ = registry.fire_tool_event(&ctx).await;
    }

    // Convert result to string content
    let content = match result {
        Ok(value) => serde_json::to_string(&value)
            .unwrap_or_else(|e| format!("{{\"error\": \"Failed to serialize result: {}\"}}", e)),
        Err(e) => format!("{{\"error\": \"{}\"}}", e),
    };

    ToolExecutionResult {
        tool_call_id,
        name: tool_name,
        content,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::turn::{
        BeforeCallLLMContext, HookAction, HookHandler, ToolHookContext, TurnConfigBuilder,
        TurnInputBuilder,
    };
    use crate::llm::{LlmProvider, ToolCompletionResponse};
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use std::sync::{Arc, Mutex};

    /// Mock LLM provider for testing.
    struct MockProvider {
        responses: Mutex<Vec<ToolCompletionResponse>>,
        call_count: Mutex<usize>,
    }

    impl MockProvider {
        fn new(responses: Vec<ToolCompletionResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
                call_count: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        fn model_name(&self) -> &str {
            "mock"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete_with_tools(
            &self,
            _request: ToolCompletionRequest,
        ) -> Result<ToolCompletionResponse, crate::llm::LlmError> {
            let mut count = self.call_count.lock().unwrap();
            let responses = self.responses.lock().unwrap();
            if *count < responses.len() {
                let response = responses[*count].clone();
                *count += 1;
                Ok(response)
            } else {
                // Default: return stop
                Ok(ToolCompletionResponse {
                    content: Some("Done".to_string()),
                    reasoning_content: None,
                    tool_calls: Vec::new(),
                    input_tokens: 10,
                    output_tokens: 5,
                    finish_reason: FinishReason::Stop,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                })
            }
        }

        async fn complete(
            &self,
            _request: crate::llm::CompletionRequest,
        ) -> Result<crate::llm::CompletionResponse, crate::llm::LlmError> {
            unreachable!("complete not used in turn execution")
        }
    }

    /// Echo tool for testing.
    struct EchoTool;

    #[async_trait]
    impl crate::tool::NamedTool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "echo".to_string(),
                description: "Echoes input".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }
        }

        async fn execute(
            &self,
            args: serde_json::Value,
        ) -> Result<serde_json::Value, crate::tool::ToolError> {
            Ok(args)
        }
    }

    /// Handler that modifies messages.
    struct MessageModifierHandler;

    #[async_trait]
    impl HookHandler for MessageModifierHandler {
        async fn on_before_call_llm(&self, ctx: &BeforeCallLLMContext) -> HookAction {
            let mut messages = ctx.messages.clone();
            // Add a prefix to track hook execution
            if let Some(first) = messages.first_mut()
                && first.role == crate::llm::Role::User
            {
                first.content = format!("[Modified] {}", first.content);
            }
            HookAction::ModifyMessages(messages)
        }
    }

    fn create_test_input(provider: Arc<dyn LlmProvider>) -> TurnInput {
        TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Hello")])
            .build()
    }

    #[tokio::test]
    async fn test_simple_response_without_tools() {
        // Provider returns immediate stop
        let provider = Arc::new(MockProvider::new(vec![ToolCompletionResponse {
            content: Some("Hello, world!".to_string()),
            reasoning_content: None,
            tool_calls: Vec::new(),
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }]));

        let input = create_test_input(provider);
        let config = TurnConfig::default();

        let output = execute_turn(input, config).await.unwrap();

        // Should have original message + assistant response
        assert_eq!(output.messages.len(), 2);
        assert_eq!(output.messages[0].role, crate::llm::Role::User);
        assert_eq!(output.messages[1].role, crate::llm::Role::Assistant);
        assert_eq!(output.messages[1].content, "Hello, world!");

        // Token usage should be tracked
        assert_eq!(output.token_usage.input_tokens, 10);
        assert_eq!(output.token_usage.output_tokens, 5);
        assert_eq!(output.token_usage.total_tokens, 15);
    }

    #[tokio::test]
    async fn test_before_call_llm_can_modify_messages() {
        let provider = Arc::new(MockProvider::new(vec![ToolCompletionResponse {
            content: Some("Response".to_string()),
            reasoning_content: None,
            tool_calls: Vec::new(),
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }]));

        let hooks = Arc::new(HookRegistry::new());
        hooks.register(HookEvent::BeforeCallLLM, Arc::new(MessageModifierHandler));

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Hello")])
            .hooks(hooks)
            .build();

        let config = TurnConfig::default();
        let output = execute_turn(input, config).await.unwrap();

        // Message should have been modified by hook
        assert!(output.messages[0].content.contains("[Modified]"));
    }

    #[tokio::test]
    async fn test_before_call_llm_can_block() {
        struct BlockingHandler;

        #[async_trait]
        impl HookHandler for BlockingHandler {
            async fn on_before_call_llm(&self, _ctx: &BeforeCallLLMContext) -> HookAction {
                HookAction::Block("Rate limited".to_string())
            }
        }

        let provider = Arc::new(MockProvider::new(vec![ToolCompletionResponse {
            content: Some("Should not reach".to_string()),
            reasoning_content: None,
            tool_calls: Vec::new(),
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }]));

        let hooks = Arc::new(HookRegistry::new());
        hooks.register(HookEvent::BeforeCallLLM, Arc::new(BlockingHandler));

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Hello")])
            .hooks(hooks)
            .build();

        let config = TurnConfig::default();
        let result = execute_turn(input, config).await;

        assert!(matches!(result, Err(TurnError::LlmCallBlocked { .. })));
        if let Err(TurnError::LlmCallBlocked { reason }) = result {
            assert_eq!(reason, "Rate limited");
        }
    }

    #[tokio::test]
    async fn test_tool_execution_path() {
        // Provider first requests tool, then stops
        let provider = Arc::new(MockProvider::new(vec![
            ToolCompletionResponse {
                content: None,
                reasoning_content: None,
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "echo".to_string(),
                    arguments: serde_json::json!({"message": "test"}),
                }],
                input_tokens: 10,
                output_tokens: 5,
                finish_reason: FinishReason::ToolUse,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
            ToolCompletionResponse {
                content: Some("Done after tool".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 15,
                output_tokens: 10,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ]));

        let tool_manager = Arc::new(ToolManager::new());
        tool_manager.register(Arc::new(EchoTool));

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Hello")])
            .tool_manager(tool_manager)
            .tool_ids(vec!["echo".to_string()])
            .build();

        let config = TurnConfig::default();
        let output = execute_turn(input, config).await.unwrap();

        // Should have: system (max_tool_calls hint) -> user -> assistant (tool_calls) -> tool_result -> assistant (final)
        assert_eq!(output.messages.len(), 5);
        assert_eq!(output.messages[0].role, crate::llm::Role::System);
        assert_eq!(output.messages[1].role, crate::llm::Role::User);
        assert_eq!(output.messages[2].role, crate::llm::Role::Assistant);
        assert!(output.messages[2].tool_calls.is_some());
        assert_eq!(output.messages[3].role, crate::llm::Role::Tool);
        assert_eq!(output.messages[4].role, crate::llm::Role::Assistant);
        assert_eq!(output.messages[4].content, "Done after tool");

        // Token usage should accumulate
        assert_eq!(output.token_usage.input_tokens, 25);
        assert_eq!(output.token_usage.output_tokens, 15);
    }

    #[tokio::test]
    async fn test_max_iterations_limit() {
        /// Provider that always returns ToolUse.
        struct AlwaysToolUseProvider;

        #[async_trait]
        impl LlmProvider for AlwaysToolUseProvider {
            fn model_name(&self) -> &str {
                "always-tool-use"
            }

            fn cost_per_token(&self) -> (Decimal, Decimal) {
                (Decimal::ZERO, Decimal::ZERO)
            }

            async fn complete_with_tools(
                &self,
                _request: ToolCompletionRequest,
            ) -> Result<ToolCompletionResponse, crate::llm::LlmError> {
                Ok(ToolCompletionResponse {
                    content: None,
                    reasoning_content: None,
                    tool_calls: vec![ToolCall {
                        id: "call_loop".to_string(),
                        name: "echo".to_string(),
                        arguments: serde_json::json!({}),
                    }],
                    input_tokens: 10,
                    output_tokens: 5,
                    finish_reason: FinishReason::ToolUse,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                })
            }

            async fn complete(
                &self,
                _request: crate::llm::CompletionRequest,
            ) -> Result<crate::llm::CompletionResponse, crate::llm::LlmError> {
                unreachable!()
            }
        }

        let provider: Arc<dyn LlmProvider> = Arc::new(AlwaysToolUseProvider);

        let tool_manager = Arc::new(ToolManager::new());
        tool_manager.register(Arc::new(EchoTool));

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Hello")])
            .tool_manager(tool_manager)
            .tool_ids(vec!["echo".to_string()])
            .build();

        let config = TurnConfigBuilder::default()
            .max_iterations(Some(3))
            .build()
            .unwrap();

        let result = execute_turn(input, config).await;

        assert!(matches!(result, Err(TurnError::MaxIterationsReached(3))));
    }

    #[tokio::test]
    async fn test_hook_blocking_behavior() {
        /// Hook handler that blocks all tool calls.
        struct BlockingHookHandler;

        #[async_trait]
        impl HookHandler for BlockingHookHandler {
            async fn on_tool_event(&self, _ctx: &ToolHookContext) -> HookAction {
                HookAction::Block("Tool calls are disabled".to_string())
            }
        }

        // Provider requests tool use
        let provider = Arc::new(MockProvider::new(vec![
            ToolCompletionResponse {
                content: None,
                reasoning_content: None,
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "echo".to_string(),
                    arguments: serde_json::json!({"message": "test"}),
                }],
                input_tokens: 10,
                output_tokens: 5,
                finish_reason: FinishReason::ToolUse,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
            ToolCompletionResponse {
                content: Some("Done".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 10,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ]));

        let tool_manager = Arc::new(ToolManager::new());
        tool_manager.register(Arc::new(EchoTool));

        // Create hook registry with blocking handler
        let hooks = Arc::new(HookRegistry::new());
        hooks.register(HookEvent::BeforeToolCall, Arc::new(BlockingHookHandler));

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Hello")])
            .tool_manager(tool_manager)
            .tool_ids(vec!["echo".to_string()])
            .hooks(hooks)
            .build();

        let config = TurnConfig::default();
        let output = execute_turn(input, config).await.unwrap();

        // Tool should have been blocked - tool result should contain blocked message
        // Messages: system, user, assistant(tool_calls), tool_result(blocked), assistant(final)
        assert_eq!(output.messages.len(), 5);
        assert_eq!(output.messages[3].role, crate::llm::Role::Tool);
        assert!(output.messages[3].content.contains("blocked"));
    }

    // ========================================================================
    // Integration tests (migrated from tests/turn_integration_test.rs)
    // ========================================================================

    use crate::tool::NamedTool;
    use std::time::Instant;

    /// Delayed tool for testing parallel execution.
    struct DelayedTool {
        delay_ms: u64,
    }

    #[async_trait]
    impl NamedTool for DelayedTool {
        fn name(&self) -> &str {
            "delayed"
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "delayed".to_string(),
                description: "Returns after a configurable delay".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "integer" }
                    }
                }),
            }
        }

        async fn execute(
            &self,
            args: serde_json::Value,
        ) -> Result<serde_json::Value, crate::tool::ToolError> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            Ok(args)
        }
    }

    /// Counter tool that tracks how many times it was executed.
    struct CounterTool {
        count: Mutex<u32>,
    }

    impl CounterTool {
        fn new() -> Self {
            Self {
                count: Mutex::new(0),
            }
        }

        fn get_count(&self) -> u32 {
            *self.count.lock().unwrap()
        }
    }

    #[async_trait]
    impl NamedTool for CounterTool {
        fn name(&self) -> &str {
            "counter"
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "counter".to_string(),
                description: "Counts how many times it was called".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }
        }

        async fn execute(
            &self,
            args: serde_json::Value,
        ) -> Result<serde_json::Value, crate::tool::ToolError> {
            let mut count = self.count.lock().unwrap();
            *count += 1;
            Ok(serde_json::json!({
                "call_number": *count,
                "input": args
            }))
        }
    }

    /// Hook handler that records all tool events it receives.
    struct RecordingHookHandler {
        events: Mutex<Vec<(HookEvent, String)>>,
    }

    impl RecordingHookHandler {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }

        fn get_events(&self) -> Vec<(HookEvent, String)> {
            self.events.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl HookHandler for RecordingHookHandler {
        async fn on_tool_event(&self, ctx: &ToolHookContext) -> HookAction {
            let mut events = self.events.lock().unwrap();
            events.push((ctx.event, ctx.tool_name.clone()));
            HookAction::Continue
        }
    }

    #[tokio::test]
    async fn test_turn_with_multiple_tool_calls() {
        let tool_manager = Arc::new(ToolManager::new());
        tool_manager.register(Arc::new(EchoTool));

        let responses = vec![
            ToolCompletionResponse {
                content: None,
                reasoning_content: None,
                tool_calls: vec![
                    ToolCall {
                        id: "call_1".to_string(),
                        name: "echo".to_string(),
                        arguments: serde_json::json!({"message": "hello"}),
                    },
                    ToolCall {
                        id: "call_2".to_string(),
                        name: "echo".to_string(),
                        arguments: serde_json::json!({"message": "world"}),
                    },
                ],
                input_tokens: 100,
                output_tokens: 50,
                finish_reason: FinishReason::ToolUse,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
            ToolCompletionResponse {
                content: Some("Done!".to_string()),
                reasoning_content: None,
                tool_calls: vec![],
                input_tokens: 80,
                output_tokens: 10,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ];

        let provider = Arc::new(MockProvider::new(responses));

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Hello")])
            .tool_manager(tool_manager)
            .tool_ids(vec!["echo".to_string()])
            .build();

        let config = TurnConfig::default();
        let output = execute_turn(input, config).await.unwrap();

        // system -> user -> assistant (tool_calls) -> tool_result -> tool_result -> assistant (final)
        assert_eq!(output.messages.len(), 6);
        assert_eq!(output.messages[0].role, crate::llm::Role::System);
        assert_eq!(output.messages[1].role, crate::llm::Role::User);
        assert_eq!(output.messages[2].role, crate::llm::Role::Assistant);
        assert!(output.messages[2].tool_calls.is_some());
        assert_eq!(output.messages[2].tool_calls.as_ref().unwrap().len(), 2);
        assert_eq!(output.messages[3].role, crate::llm::Role::Tool);
        assert_eq!(output.messages[4].role, crate::llm::Role::Tool);
        assert_eq!(output.messages[5].role, crate::llm::Role::Assistant);
        assert_eq!(output.messages[5].content, "Done!");

        assert_eq!(output.token_usage.input_tokens, 180);
        assert_eq!(output.token_usage.output_tokens, 60);
        assert_eq!(output.token_usage.total_tokens, 240);
    }

    #[tokio::test]
    async fn test_parallel_tool_execution_timing() {
        let tool_manager = Arc::new(ToolManager::new());
        tool_manager.register(Arc::new(DelayedTool { delay_ms: 100 }));

        let responses = vec![
            ToolCompletionResponse {
                content: None,
                reasoning_content: None,
                tool_calls: vec![
                    ToolCall {
                        id: "call_1".to_string(),
                        name: "delayed".to_string(),
                        arguments: serde_json::json!({"id": 1}),
                    },
                    ToolCall {
                        id: "call_2".to_string(),
                        name: "delayed".to_string(),
                        arguments: serde_json::json!({"id": 2}),
                    },
                ],
                input_tokens: 50,
                output_tokens: 20,
                finish_reason: FinishReason::ToolUse,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
            ToolCompletionResponse {
                content: Some("Done after parallel execution".to_string()),
                reasoning_content: None,
                tool_calls: vec![],
                input_tokens: 60,
                output_tokens: 15,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ];

        let provider = Arc::new(MockProvider::new(responses));

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Test parallel")])
            .tool_manager(tool_manager)
            .tool_ids(vec!["delayed".to_string()])
            .build();

        let config = TurnConfig::default();

        let start = Instant::now();
        let output = execute_turn(input, config).await.unwrap();
        let elapsed = start.elapsed();

        // If tools run in parallel, total time should be ~100ms, not ~200ms
        assert!(
            elapsed.as_millis() < 250,
            "Parallel execution should complete in ~100ms, took {:?}",
            elapsed
        );

        assert_eq!(output.messages.len(), 6);
        assert_eq!(output.token_usage.total_tokens, 145);
    }

    #[tokio::test]
    async fn test_hook_callbacks_are_invoked() {
        let counter_tool = Arc::new(CounterTool::new());
        let tool_manager = Arc::new(ToolManager::new());
        tool_manager.register(counter_tool.clone());

        let recording_handler = Arc::new(RecordingHookHandler::new());
        let hooks = Arc::new(HookRegistry::new());
        hooks.register(HookEvent::BeforeToolCall, recording_handler.clone());
        hooks.register(HookEvent::AfterToolCall, recording_handler.clone());
        hooks.register(HookEvent::TurnEnd, recording_handler.clone());

        let responses = vec![
            ToolCompletionResponse {
                content: None,
                reasoning_content: None,
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "counter".to_string(),
                    arguments: serde_json::json!({}),
                }],
                input_tokens: 30,
                output_tokens: 10,
                finish_reason: FinishReason::ToolUse,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
            ToolCompletionResponse {
                content: Some("Complete".to_string()),
                reasoning_content: None,
                tool_calls: vec![],
                input_tokens: 20,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ];

        let provider = Arc::new(MockProvider::new(responses));

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Test hooks")])
            .tool_manager(tool_manager)
            .tool_ids(vec!["counter".to_string()])
            .hooks(hooks)
            .build();

        let config = TurnConfig::default();
        let output = execute_turn(input, config).await.unwrap();

        assert_eq!(counter_tool.get_count(), 1);
        assert_eq!(output.messages.len(), 5);

        let events = recording_handler.get_events();
        assert_eq!(events.len(), 3, "Expected 3 events, got: {:?}", events);
        assert_eq!(events[0].0, HookEvent::BeforeToolCall);
        assert_eq!(events[0].1, "counter");
        assert_eq!(events[1].0, HookEvent::AfterToolCall);
        assert_eq!(events[1].1, "counter");
        assert_eq!(events[2].0, HookEvent::TurnEnd);
    }

    #[tokio::test]
    async fn test_multiple_iterations_with_tools() {
        let tool_manager = Arc::new(ToolManager::new());
        tool_manager.register(Arc::new(EchoTool));

        let responses = vec![
            ToolCompletionResponse {
                content: None,
                reasoning_content: None,
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "echo".to_string(),
                    arguments: serde_json::json!({"step": 1}),
                }],
                input_tokens: 50,
                output_tokens: 20,
                finish_reason: FinishReason::ToolUse,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
            ToolCompletionResponse {
                content: None,
                reasoning_content: None,
                tool_calls: vec![ToolCall {
                    id: "call_2".to_string(),
                    name: "echo".to_string(),
                    arguments: serde_json::json!({"step": 2}),
                }],
                input_tokens: 60,
                output_tokens: 25,
                finish_reason: FinishReason::ToolUse,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
            ToolCompletionResponse {
                content: Some("Completed after two iterations".to_string()),
                reasoning_content: None,
                tool_calls: vec![],
                input_tokens: 70,
                output_tokens: 30,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ];

        let provider = Arc::new(MockProvider::new(responses));

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Multi-iteration test")])
            .tool_manager(tool_manager)
            .tool_ids(vec!["echo".to_string()])
            .build();

        let config = TurnConfig::default();
        let output = execute_turn(input, config).await.unwrap();

        // system -> user -> assistant(tool_calls) -> tool_result -> assistant(tool_calls) -> tool_result -> assistant(final)
        assert_eq!(output.messages.len(), 7);

        assert_eq!(output.token_usage.input_tokens, 180);
        assert_eq!(output.token_usage.output_tokens, 75);
        assert_eq!(output.token_usage.total_tokens, 255);
    }
}
