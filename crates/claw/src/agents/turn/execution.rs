//! Turn execution logic.
//!
//! This module implements the core execution loop for turn-based LLM conversations
//! with tool support. It handles the LLM -> Tool -> LLM cycle with parallel tool
//! execution and hook integration.

use std::sync::Arc;

use futures_util::future::join_all;
use tokio::time::{error::Elapsed, timeout};

use crate::llm::{ChatMessage, FinishReason, ToolCall, ToolCompletionRequest, ToolDefinition};
use crate::tool::ToolManager;

use super::hooks::{HookContext, HookEvent, HookRegistry};
use super::{TokenUsage, TurnConfig, TurnError, TurnInput, TurnOutput};

/// Executes a single turn in a conversation.
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
    let mut messages = input.messages;
    let provider = input.provider;
    let tool_manager = input.tool_manager;
    let tool_ids = input.tool_ids;
    let hooks = input.hooks;

    // Resolve tool definitions from tool_manager
    let tools: Vec<ToolDefinition> = tool_ids
        .iter()
        .filter_map(|id| tool_manager.get(id))
        .map(|tool| tool.definition())
        .collect();

    let max_iterations = config.max_iterations.unwrap_or(50);
    let tool_timeout_secs = config.tool_timeout_secs.unwrap_or(120);

    let mut token_usage = TokenUsage::default();

    for _iteration in 0..max_iterations {
        // Build the request with current messages and tools
        let request = ToolCompletionRequest::new(messages.clone(), tools.clone());

        // Call the LLM
        let response = provider
            .complete_with_tools(request)
            .await
            .map_err(TurnError::LlmFailed)?;

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

                // Fire TurnEnd hook
                if let Some(ref registry) = hooks {
                    let ctx = HookContext {
                        event: HookEvent::TurnEnd,
                        tool_name: String::new(),
                        tool_call_id: String::new(),
                        tool_input: serde_json::Value::Null,
                        tool_result: None,
                        error: None,
                    };
                    // TurnEnd is observe-only, ignore errors
                    let _ = registry.fire(&ctx).await;
                }

                return Ok(TurnOutput {
                    messages,
                    token_usage,
                });
            }
            FinishReason::ToolUse => {
                // Add assistant message with tool_calls to history
                let assistant_msg = ChatMessage::assistant_with_tool_calls(
                    response.content.clone(),
                    response.tool_calls.clone(),
                );
                messages.push(assistant_msg);

                // Execute tools in parallel
                let tool_results = execute_tools_parallel(
                    response.tool_calls,
                    &tool_manager,
                    hooks.as_ref().map(Arc::as_ref),
                    tool_timeout_secs,
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
            FinishReason::Length => {
                // Context length exceeded - for now, return an error
                // In the future, this could be handled with continuation
                return Err(TurnError::ContextLengthExceeded(
                    (token_usage.input_tokens + token_usage.output_tokens) as usize,
                ));
            }
            FinishReason::ContentFilter | FinishReason::Unknown => {
                // For content filter or unknown reasons, return what we have
                if let Some(content) = &response.content
                    && !content.is_empty()
                {
                    messages.push(ChatMessage::assistant(content.clone()));
                }

                return Ok(TurnOutput {
                    messages,
                    token_usage,
                });
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
    tool_manager: &ToolManager,
    hooks: Option<&HookRegistry>,
    tool_timeout_secs: u64,
) -> Vec<ToolExecutionResult> {
    let futures: Vec<_> = tool_calls
        .into_iter()
        .map(|tool_call| async move {
            execute_single_tool(tool_call, tool_manager, hooks, tool_timeout_secs).await
        })
        .collect();

    join_all(futures).await
}

/// Executes a single tool call with hooks and timeout.
async fn execute_single_tool(
    tool_call: ToolCall,
    tool_manager: &ToolManager,
    hooks: Option<&HookRegistry>,
    tool_timeout_secs: u64,
) -> ToolExecutionResult {
    let tool_call_id = tool_call.id.clone();
    let tool_name = tool_call.name.clone();
    let tool_input = tool_call.arguments.clone();

    // Fire BeforeToolCall hook
    if let Some(registry) = hooks {
        let ctx = HookContext {
            event: HookEvent::BeforeToolCall,
            tool_name: tool_name.clone(),
            tool_call_id: tool_call_id.clone(),
            tool_input: tool_input.clone(),
            tool_result: None,
            error: None,
        };
        if let Err(reason) = registry.fire(&ctx).await {
            // Hook blocked the tool call
            let content = format!("Tool call blocked: {}", reason);

            // Fire AfterToolCall hook with error
            let after_ctx = HookContext {
                event: HookEvent::AfterToolCall,
                tool_name: tool_name.clone(),
                tool_call_id: tool_call_id.clone(),
                tool_input,
                tool_result: None,
                error: Some(reason),
            };
            let _ = registry.fire(&after_ctx).await;

            return ToolExecutionResult {
                tool_call_id,
                name: tool_name,
                content,
            };
        }
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

    // Fire AfterToolCall hook
    if let Some(registry) = hooks {
        let (tool_result, error) = match &result {
            Ok(value) => (Some(value.clone()), None),
            Err(e) => (None, Some(e.clone())),
        };
        let ctx = HookContext {
            event: HookEvent::AfterToolCall,
            tool_name: tool_name.clone(),
            tool_call_id: tool_call_id.clone(),
            tool_input,
            tool_result,
            error,
        };
        let _ = registry.fire(&ctx).await;
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
    use crate::agents::turn::{HookHandler, TurnConfigBuilder, TurnInputBuilder};
    use crate::llm::{LlmProvider, ToolCompletionResponse};
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use std::sync::Mutex;

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

    /// Delayed tool for testing parallel execution.
    struct DelayedTool {
        delay_ms: u64,
    }

    #[async_trait]
    impl crate::tool::NamedTool for DelayedTool {
        fn name(&self) -> &str {
            "delayed"
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "delayed".to_string(),
                description: "Delayed response".to_string(),
                parameters: serde_json::json!({"type": "object"}),
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

    /// Failing tool for testing error handling.
    struct FailingTool;

    #[async_trait]
    impl crate::tool::NamedTool for FailingTool {
        fn name(&self) -> &str {
            "failing"
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "failing".to_string(),
                description: "Always fails".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }
        }

        async fn execute(
            &self,
            _args: serde_json::Value,
        ) -> Result<serde_json::Value, crate::tool::ToolError> {
            Err(crate::tool::ToolError::ExecutionFailed {
                tool_name: "failing".to_string(),
                reason: "intentional failure".to_string(),
            })
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

        // Should have: user -> assistant (tool_calls) -> tool_result -> assistant (final)
        assert_eq!(output.messages.len(), 4);
        assert_eq!(output.messages[0].role, crate::llm::Role::User);
        assert_eq!(output.messages[1].role, crate::llm::Role::Assistant);
        assert!(output.messages[1].tool_calls.is_some());
        assert_eq!(output.messages[2].role, crate::llm::Role::Tool);
        assert_eq!(output.messages[3].role, crate::llm::Role::Assistant);
        assert_eq!(output.messages[3].content, "Done after tool");

        // Token usage should accumulate
        assert_eq!(output.token_usage.input_tokens, 25);
        assert_eq!(output.token_usage.output_tokens, 15);
    }

    #[tokio::test]
    async fn test_parallel_tool_execution() {
        use std::time::Instant;

        // Provider requests two tools
        let provider = Arc::new(MockProvider::new(vec![
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
        // Each tool delays 100ms - if parallel, total should be ~100ms, not ~200ms
        tool_manager.register(Arc::new(DelayedTool { delay_ms: 100 }));

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Hello")])
            .tool_manager(tool_manager)
            .tool_ids(vec!["delayed".to_string()])
            .build();

        let config = TurnConfig::default();

        let start = Instant::now();
        let output = execute_turn(input, config).await.unwrap();
        let elapsed = start.elapsed();

        // Should have: user, assistant(tool_calls), tool_result, tool_result, assistant(final)
        // That's 5 messages
        assert_eq!(output.messages.len(), 5);
        assert_eq!(output.messages[2].role, crate::llm::Role::Tool);
        assert_eq!(output.messages[3].role, crate::llm::Role::Tool);

        // If parallel, should take ~100ms, not ~200ms
        // Allow some margin for test overhead
        assert!(
            elapsed.as_millis() < 250,
            "Parallel execution should be faster: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_max_iterations_limit() {
        // Provider always requests tool use
        // Use AlwaysToolUseProvider directly
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

    #[tokio::test]
    async fn test_hook_blocking_behavior() {
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
        assert_eq!(output.messages.len(), 4);
        assert_eq!(output.messages[2].role, crate::llm::Role::Tool);
        assert!(output.messages[2].content.contains("blocked"));
    }

    /// Hook handler that blocks all tool calls.
    struct BlockingHookHandler;

    #[async_trait]
    impl HookHandler for BlockingHookHandler {
        async fn on_event(&self, _ctx: &HookContext) -> Result<(), String> {
            Err("Tool calls are disabled".to_string())
        }
    }

    #[tokio::test]
    async fn test_tool_execution_failure_captured() {
        // Provider requests failing tool
        let provider = Arc::new(MockProvider::new(vec![
            ToolCompletionResponse {
                content: None,
                reasoning_content: None,
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "failing".to_string(),
                    arguments: serde_json::json!({}),
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
        tool_manager.register(Arc::new(FailingTool));

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Hello")])
            .tool_manager(tool_manager)
            .tool_ids(vec!["failing".to_string()])
            .build();

        let config = TurnConfig::default();
        let output = execute_turn(input, config).await.unwrap();

        // Tool failure should be captured in tool result, not break the loop
        assert_eq!(output.messages.len(), 4);
        assert_eq!(output.messages[2].role, crate::llm::Role::Tool);
        assert!(output.messages[2].content.contains("error"));
    }

    #[tokio::test]
    async fn test_tool_not_found_captured() {
        // Provider requests non-existent tool
        let provider = Arc::new(MockProvider::new(vec![
            ToolCompletionResponse {
                content: None,
                reasoning_content: None,
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "nonexistent".to_string(),
                    arguments: serde_json::json!({}),
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
        // Don't register any tools

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Hello")])
            .tool_manager(tool_manager)
            .tool_ids(vec!["nonexistent".to_string()])
            .build();

        let config = TurnConfig::default();
        let output = execute_turn(input, config).await.unwrap();

        // Tool not found should be captured as error in tool result
        assert_eq!(output.messages.len(), 4);
        assert_eq!(output.messages[2].role, crate::llm::Role::Tool);
        assert!(output.messages[2].content.contains("error"));
    }

    #[tokio::test]
    async fn test_empty_tool_ids() {
        // Provider returns stop without any tools
        let provider = Arc::new(MockProvider::new(vec![ToolCompletionResponse {
            content: Some("Hello!".to_string()),
            reasoning_content: None,
            tool_calls: Vec::new(),
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }]));

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Hello")])
            .tool_ids(Vec::new()) // No tools
            .build();

        let config = TurnConfig::default();
        let output = execute_turn(input, config).await.unwrap();

        assert_eq!(output.messages.len(), 2);
        assert_eq!(output.messages[1].content, "Hello!");
    }
}
