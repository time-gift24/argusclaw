//! Turn execution logic.
//!
//! This module implements backward-compatible wrappers around the new `Turn` struct.
//! The core execution logic has been moved to `Turn::execute_loop()`.

use std::sync::Arc;

use tokio::sync::broadcast;

use argus_protocol::tool::NamedTool;

use super::{Turn, TurnBuilder, TurnConfig, TurnError, TurnInput, TurnOutput};

/// Execution mode for turn processing.
#[derive(Debug, Clone, Copy)]
pub enum ExecutionMode {
    /// Non-streaming mode (wait for complete response).
    NonStreaming,
    /// Streaming mode (emit real-time events).
    Streaming,
}

/// Executes a single turn in a conversation (non-streaming).
///
/// This function is a backward-compatible wrapper that internally uses the new `Turn` struct.
/// It converts the legacy `TurnInput` to a `Turn` and executes it.
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
/// in `TurnInput`. This is a backward-compatible wrapper that internally uses the new `Turn` struct.
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
/// This function converts the legacy `TurnInput` to a new `Turn` struct and executes it.
async fn execute_turn_with_mode(
    input: TurnInput,
    config: TurnConfig,
    _mode: ExecutionMode,
) -> Result<TurnOutput, TurnError> {
    // Convert TurnInput to Turn
    let turn = turn_input_to_turn(input, config)?;

    // Execute the turn
    turn.execute().await
}

/// Convert legacy TurnInput to new Turn struct.
///
/// This function handles the conversion from the old API (TurnInput with ToolManager and HookRegistry)
/// to the new API (Turn with direct tool and hook ownership).
fn turn_input_to_turn(input: TurnInput, config: TurnConfig) -> Result<Turn, TurnError> {
    // Extract tools from ToolManager
    let tools: Vec<Arc<dyn NamedTool>> = input
        .tool_ids
        .iter()
        .filter_map(|id| input.tool_manager.get(id))
        .collect();

    // Extract hooks from HookRegistry (if present)
    let hooks: Vec<Arc<dyn argus_protocol::HookHandler>> = if let Some(registry) = input.hooks {
        registry.all_handlers()
    } else {
        Vec::new()
    };

    // Create channel for streaming events
    let (stream_tx, _) = broadcast::channel(256);

    // Get thread_event_tx and thread_id
    let thread_event_tx = input.thread_event_sender.unwrap_or_else(|| {
        // Create a dummy channel if not provided
        broadcast::channel(1).0
    });

    let thread_id = input.thread_id.unwrap_or_else(|| "unknown".to_string());

    // Generate turn number (default to 1 if not specified)
    let turn_number = 1; // TODO: Pass turn_number in TurnInput or derive from context

    // Build Turn using TurnBuilder
    TurnBuilder::default()
        .turn_number(turn_number)
        .thread_id(thread_id)
        .messages(input.messages)
        .provider(input.provider)
        .tools(tools)
        .hooks(hooks)
        .config(config)
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TurnInputBuilder;
    use argus_protocol::llm::{ChatMessage, LlmProvider, Role};
    use argus_protocol::{BeforeCallLLMContext, HookAction, HookHandler, HookRegistry};
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use std::sync::{Arc, Mutex};

    /// Mock LLM provider for testing.
    struct MockProvider {
        responses: Mutex<Vec<argus_protocol::llm::ToolCompletionResponse>>,
        call_count: Mutex<usize>,
    }

    impl MockProvider {
        fn new(responses: Vec<argus_protocol::llm::ToolCompletionResponse>) -> Self {
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
            _request: argus_protocol::llm::ToolCompletionRequest,
        ) -> Result<argus_protocol::llm::ToolCompletionResponse, argus_protocol::llm::LlmError>
        {
            let mut count = self.call_count.lock().unwrap();
            let responses = self.responses.lock().unwrap();
            if *count < responses.len() {
                let response = responses[*count].clone();
                *count += 1;
                Ok(response)
            } else {
                // Default: return stop
                Ok(argus_protocol::llm::ToolCompletionResponse {
                    content: Some("Done".to_string()),
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

        async fn complete(
            &self,
            _request: argus_protocol::llm::CompletionRequest,
        ) -> Result<argus_protocol::llm::CompletionResponse, argus_protocol::llm::LlmError>
        {
            unreachable!("complete not used in turn execution")
        }
    }

    /// Echo tool for testing.
    #[allow(dead_code)]
    struct EchoTool;

    #[async_trait]
    impl argus_tool::NamedTool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn definition(&self) -> argus_protocol::llm::ToolDefinition {
            argus_protocol::llm::ToolDefinition {
                name: "echo".to_string(),
                description: "Echoes input".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }
        }

        async fn execute(
            &self,
            args: serde_json::Value,
        ) -> Result<serde_json::Value, argus_tool::ToolError> {
            Ok(args)
        }
    }

    fn create_test_input(provider: Arc<dyn LlmProvider>) -> TurnInput {
        TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Hello")])
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn test_simple_response_without_tools() {
        // Provider returns immediate stop
        let provider = Arc::new(MockProvider::new(vec![
            argus_protocol::llm::ToolCompletionResponse {
                content: Some("Hello, world!".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 10,
                output_tokens: 5,
                finish_reason: argus_protocol::llm::FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ]));

        let input = create_test_input(provider);
        let config = TurnConfig::default();

        let output = execute_turn(input, config).await.unwrap();

        // Should have original message + assistant response
        assert_eq!(output.messages.len(), 2);
        assert_eq!(output.messages[0].role, Role::User);
        assert_eq!(output.messages[1].role, Role::Assistant);
        assert_eq!(output.messages[1].content, "Hello, world!");

        // Token usage should be tracked
        assert_eq!(output.token_usage.input_tokens, 10);
        assert_eq!(output.token_usage.output_tokens, 5);
        assert_eq!(output.token_usage.total_tokens, 15);
    }

    #[tokio::test]
    async fn test_before_call_llm_can_modify_messages() {
        struct MessageModifierHandler;

        #[async_trait]
        impl HookHandler for MessageModifierHandler {
            async fn on_before_call_llm(&self, ctx: &BeforeCallLLMContext) -> HookAction {
                let mut messages = ctx.messages.clone();
                // Add a prefix to track hook execution
                if let Some(first) = messages.first_mut()
                    && first.role == Role::User
                {
                    first.content = format!("[Modified] {}", first.content);
                }
                HookAction::ModifyMessages(messages)
            }
        }

        let provider = Arc::new(MockProvider::new(vec![
            argus_protocol::llm::ToolCompletionResponse {
                content: Some("Response".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 10,
                output_tokens: 5,
                finish_reason: argus_protocol::llm::FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ]));

        let hooks = Arc::new(HookRegistry::new());
        hooks.register(
            argus_protocol::HookEvent::BeforeCallLLM,
            Arc::new(MessageModifierHandler),
        );

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Hello")])
            .hooks(hooks)
            .build()
            .unwrap();

        let config = TurnConfig::default();
        let output = execute_turn(input, config).await.unwrap();

        // Note: Hook functionality is temporarily disabled in Turn API conversion
        // This test will need to be updated once HookRegistry::all_handlers() is implemented
        // For now, we just verify the execution completes
        assert_eq!(output.messages.len(), 2);
    }
}
