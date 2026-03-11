//! Integration tests for the turn module.
//!
//! These tests verify end-to-end turn execution including:
//! - Multiple tool calls in a single turn
//! - Parallel tool execution
//! - Token usage tracking
//! - Hook callbacks

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use async_trait::async_trait;
use rust_decimal::Decimal;

use claw::agents::turn::{
    BeforeCallLLMContext, HookAction, HookEvent, HookHandler, HookRegistry, ToolHookContext,
    TurnConfig, TurnInputBuilder, execute_turn,
};
use claw::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmProvider,
    ToolCall, ToolCompletionRequest, ToolCompletionResponse, ToolDefinition,
};
use claw::tool::{NamedTool, ToolError, ToolManager};

// ============================================================================
// Mock Provider
// ============================================================================

/// Mock LLM provider that returns pre-defined responses in sequence.
struct SequentialMockProvider {
    responses: Mutex<Vec<ToolCompletionResponse>>,
    call_count: Mutex<usize>,
}

impl SequentialMockProvider {
    fn new(responses: Vec<ToolCompletionResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
            call_count: Mutex::new(0),
        }
    }
}

#[async_trait]
impl LlmProvider for SequentialMockProvider {
    fn model_name(&self) -> &str {
        "mock"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        unimplemented!("complete not used in turn execution")
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let mut count = self.call_count.lock().unwrap();
        let responses = self.responses.lock().unwrap();
        let response = responses
            .get(*count)
            .cloned()
            .unwrap_or_else(|| panic!("No more responses configured for call {}", count));
        *count += 1;
        Ok(response)
    }
}

// ============================================================================
// Test Tools
// ============================================================================

/// Echo tool that returns its input arguments.
struct EchoTool;

#[async_trait]
impl NamedTool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo".to_string(),
            description: "Echoes input back".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                }
            }),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        Ok(args)
    }
}

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

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
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

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let mut count = self.count.lock().unwrap();
        *count += 1;
        Ok(serde_json::json!({
            "call_number": *count,
            "input": args
        }))
    }
}

// ============================================================================
// Test Hook Handlers
// ============================================================================

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

/// Hook handler that blocks specific tools.
struct BlockingHookHandler {
    blocked_tools: Vec<String>,
}

impl BlockingHookHandler {
    fn new(blocked_tools: Vec<String>) -> Self {
        Self { blocked_tools }
    }
}

#[async_trait]
impl HookHandler for BlockingHookHandler {
    async fn on_tool_event(&self, ctx: &ToolHookContext) -> HookAction {
        if ctx.event == HookEvent::BeforeToolCall && self.blocked_tools.contains(&ctx.tool_name) {
            HookAction::Block(format!("Tool '{}' is blocked", ctx.tool_name))
        } else {
            HookAction::Continue
        }
    }
}

/// Hook handler that modifies messages before LLM call.
struct MessageModifierHook;

#[async_trait]
impl HookHandler for MessageModifierHook {
    async fn on_before_call_llm(&self, ctx: &BeforeCallLLMContext) -> HookAction {
        let mut messages = ctx.messages.clone();
        // Add a tracking message
        if let Some(first) = messages.first_mut()
            && first.role == claw::llm::Role::User
        {
            first.content = format!("[Modified by hook] {}", first.content);
        }
        HookAction::ModifyMessages(messages)
    }
}

// ============================================================================
// Integration Tests
// ============================================================================

#[tokio::test]
async fn test_turn_with_multiple_tool_calls() {
    // Set up tool manager with echo tool
    let tool_manager = Arc::new(ToolManager::new());
    tool_manager.register(Arc::new(EchoTool));

    // Configure responses: tool call -> final response
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

    let provider = Arc::new(SequentialMockProvider::new(responses));

    let input = TurnInputBuilder::new()
        .provider(provider)
        .messages(vec![ChatMessage::user("Hello")])
        .tool_manager(tool_manager)
        .tool_ids(vec!["echo".to_string()])
        .build();

    let config = TurnConfig::default();
    let output = execute_turn(input, config).await.unwrap();

    // Verify message structure: system -> user -> assistant (tool_calls) -> tool_result -> tool_result -> assistant (final)
    assert_eq!(output.messages.len(), 6);
    assert_eq!(output.messages[0].role, claw::llm::Role::System);
    assert_eq!(output.messages[1].role, claw::llm::Role::User);
    assert_eq!(output.messages[2].role, claw::llm::Role::Assistant);
    assert!(output.messages[2].tool_calls.is_some());
    assert_eq!(output.messages[2].tool_calls.as_ref().unwrap().len(), 2);
    assert_eq!(output.messages[3].role, claw::llm::Role::Tool);
    assert_eq!(output.messages[4].role, claw::llm::Role::Tool);
    assert_eq!(output.messages[5].role, claw::llm::Role::Assistant);
    assert_eq!(output.messages[5].content, "Done!");

    // Verify token usage is accumulated across both LLM calls
    assert_eq!(output.token_usage.input_tokens, 180); // 100 + 80
    assert_eq!(output.token_usage.output_tokens, 60); // 50 + 10
    assert_eq!(output.token_usage.total_tokens, 240);
}

#[tokio::test]
async fn test_parallel_tool_execution_timing() {
    // Set up tool manager with delayed tool
    let tool_manager = Arc::new(ToolManager::new());
    tool_manager.register(Arc::new(DelayedTool { delay_ms: 100 }));

    // Provider requests two tools in parallel, then stops
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

    let provider = Arc::new(SequentialMockProvider::new(responses));

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
    // Allow some margin for test overhead
    assert!(
        elapsed.as_millis() < 250,
        "Parallel execution should complete in ~100ms, took {:?}",
        elapsed
    );

    // Verify we have the correct message structure (including system message)
    assert_eq!(output.messages.len(), 6);
    assert_eq!(output.token_usage.total_tokens, 145); // 50+20+60+15
}

#[tokio::test]
async fn test_hook_callbacks_are_invoked() {
    // Set up tool manager with counter tool
    let counter_tool = Arc::new(CounterTool::new());
    let tool_manager = Arc::new(ToolManager::new());
    tool_manager.register(counter_tool.clone());

    // Set up hook registry with recording handler
    let recording_handler = Arc::new(RecordingHookHandler::new());
    let hooks = Arc::new(HookRegistry::new());
    hooks.register(HookEvent::BeforeToolCall, recording_handler.clone());
    hooks.register(HookEvent::AfterToolCall, recording_handler.clone());
    hooks.register(HookEvent::TurnEnd, recording_handler.clone());

    // Provider requests tool call then stops
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

    let provider = Arc::new(SequentialMockProvider::new(responses));

    let input = TurnInputBuilder::new()
        .provider(provider)
        .messages(vec![ChatMessage::user("Test hooks")])
        .tool_manager(tool_manager)
        .tool_ids(vec!["counter".to_string()])
        .hooks(hooks)
        .build();

    let config = TurnConfig::default();
    let output = execute_turn(input, config).await.unwrap();

    // Verify the tool was actually called
    assert_eq!(counter_tool.get_count(), 1);

    // Verify output structure (including system message for max_tool_calls)
    assert_eq!(output.messages.len(), 5);

    // Verify hooks were called in the right order
    let events = recording_handler.get_events();
    // We expect: BeforeToolCall, AfterToolCall, TurnEnd
    assert_eq!(events.len(), 3, "Expected 3 events, got: {:?}", events);
    assert_eq!(events[0].0, HookEvent::BeforeToolCall);
    assert_eq!(events[0].1, "counter");
    assert_eq!(events[1].0, HookEvent::AfterToolCall);
    assert_eq!(events[1].1, "counter");
    assert_eq!(events[2].0, HookEvent::TurnEnd);
}

#[tokio::test]
async fn test_hook_can_block_tool_execution() {
    // Set up tool manager with counter tool
    let counter_tool = Arc::new(CounterTool::new());
    let tool_manager = Arc::new(ToolManager::new());
    tool_manager.register(counter_tool.clone());

    // Set up hook registry with blocking handler
    let hooks = Arc::new(HookRegistry::new());
    hooks.register(
        HookEvent::BeforeToolCall,
        Arc::new(BlockingHookHandler::new(vec!["counter".to_string()])),
    );

    // Provider requests tool call then stops
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
            content: Some("After blocked tool".to_string()),
            reasoning_content: None,
            tool_calls: vec![],
            input_tokens: 20,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        },
    ];

    let provider = Arc::new(SequentialMockProvider::new(responses));

    let input = TurnInputBuilder::new()
        .provider(provider)
        .messages(vec![ChatMessage::user("Test blocking")])
        .tool_manager(tool_manager)
        .tool_ids(vec!["counter".to_string()])
        .hooks(hooks)
        .build();

    let config = TurnConfig::default();
    let output = execute_turn(input, config).await.unwrap();

    // Tool should NOT have been executed (blocked by hook)
    assert_eq!(counter_tool.get_count(), 0);

    // Verify the tool result message contains blocked info
    // Messages: system, user, assistant(tool_calls), tool_result(blocked), assistant(final)
    assert_eq!(output.messages[3].role, claw::llm::Role::Tool);
    assert!(output.messages[3].content.contains("blocked"));
}

#[tokio::test]
async fn test_multiple_iterations_with_tools() {
    // Set up tool manager with echo tool
    let tool_manager = Arc::new(ToolManager::new());
    tool_manager.register(Arc::new(EchoTool));

    // Provider requests tool use twice before stopping
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

    let provider = Arc::new(SequentialMockProvider::new(responses));

    let input = TurnInputBuilder::new()
        .provider(provider)
        .messages(vec![ChatMessage::user("Multi-iteration test")])
        .tool_manager(tool_manager)
        .tool_ids(vec!["echo".to_string()])
        .build();

    let config = TurnConfig::default();
    let output = execute_turn(input, config).await.unwrap();

    // Verify message structure (including system message):
    // system -> user -> assistant(tool_calls) -> tool_result -> assistant(tool_calls) -> tool_result -> assistant(final)
    assert_eq!(output.messages.len(), 7);

    // Verify token usage accumulated across all three LLM calls
    assert_eq!(output.token_usage.input_tokens, 180); // 50 + 60 + 70
    assert_eq!(output.token_usage.output_tokens, 75); // 20 + 25 + 30
    assert_eq!(output.token_usage.total_tokens, 255);
}

#[tokio::test]
async fn test_simple_response_without_tools() {
    // Provider returns immediate stop without any tool calls
    let responses = vec![ToolCompletionResponse {
        content: Some("Hello, world!".to_string()),
        reasoning_content: None,
        tool_calls: vec![],
        input_tokens: 25,
        output_tokens: 15,
        finish_reason: FinishReason::Stop,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
    }];

    let provider = Arc::new(SequentialMockProvider::new(responses));

    let input = TurnInputBuilder::new()
        .provider(provider)
        .messages(vec![ChatMessage::user("Hello")])
        .build();

    let config = TurnConfig::default();
    let output = execute_turn(input, config).await.unwrap();

    // Should have: user -> assistant(final)
    assert_eq!(output.messages.len(), 2);
    assert_eq!(output.messages[0].role, claw::llm::Role::User);
    assert_eq!(output.messages[1].role, claw::llm::Role::Assistant);
    assert_eq!(output.messages[1].content, "Hello, world!");

    // Token usage
    assert_eq!(output.token_usage.input_tokens, 25);
    assert_eq!(output.token_usage.output_tokens, 15);
}

#[tokio::test]
async fn test_system_prompt_included() {
    let responses = vec![ToolCompletionResponse {
        content: Some("Response with system prompt".to_string()),
        reasoning_content: None,
        tool_calls: vec![],
        input_tokens: 30,
        output_tokens: 10,
        finish_reason: FinishReason::Stop,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
    }];

    let provider = Arc::new(SequentialMockProvider::new(responses));

    let input = TurnInputBuilder::new()
        .provider(provider)
        .messages(vec![ChatMessage::user("Test")])
        .system_prompt("You are a helpful assistant.")
        .build();

    let config = TurnConfig::default();
    let output = execute_turn(input, config).await.unwrap();

    // Note: The system prompt is not added to messages by execute_turn
    // (it's expected to be added by the caller if needed)
    assert_eq!(output.messages.len(), 2);
    assert_eq!(output.messages[1].content, "Response with system prompt");
}

#[tokio::test]
async fn test_before_call_llm_can_modify_messages() {
    let responses = vec![ToolCompletionResponse {
        content: Some("Response".to_string()),
        reasoning_content: None,
        tool_calls: vec![],
        input_tokens: 30,
        output_tokens: 10,
        finish_reason: FinishReason::Stop,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
    }];

    let provider = Arc::new(SequentialMockProvider::new(responses));

    let hooks = Arc::new(HookRegistry::new());
    hooks.register(HookEvent::BeforeCallLLM, Arc::new(MessageModifierHook));

    let input = TurnInputBuilder::new()
        .provider(provider)
        .messages(vec![ChatMessage::user("Test message modification")])
        .hooks(hooks)
        .build();

    let config = TurnConfig::default();
    let output = execute_turn(input, config).await.unwrap();

    // The hook should have modified the user message
    assert!(output.messages[0].content.contains("[Modified by hook]"));
}

#[tokio::test]
async fn test_before_call_llm_can_block() {
    struct BlockingLlmHandler;

    #[async_trait]
    impl HookHandler for BlockingLlmHandler {
        async fn on_before_call_llm(&self, _ctx: &BeforeCallLLMContext) -> HookAction {
            HookAction::Block("LLM calls are disabled".to_string())
        }
    }

    let responses = vec![ToolCompletionResponse {
        content: Some("Should not reach".to_string()),
        reasoning_content: None,
        tool_calls: vec![],
        input_tokens: 30,
        output_tokens: 10,
        finish_reason: FinishReason::Stop,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
    }];

    let provider = Arc::new(SequentialMockProvider::new(responses));

    let hooks = Arc::new(HookRegistry::new());
    hooks.register(HookEvent::BeforeCallLLM, Arc::new(BlockingLlmHandler));

    let input = TurnInputBuilder::new()
        .provider(provider)
        .messages(vec![ChatMessage::user("Test")])
        .hooks(hooks)
        .build();

    let config = TurnConfig::default();
    let result = execute_turn(input, config).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, claw::agents::turn::TurnError::LlmCallBlocked { .. }));
}
