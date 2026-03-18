//! Integration tests for argus-turn

use std::sync::Arc;

use tokio::sync::broadcast;

use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmProvider,
    Role, ToolCompletionRequest, ToolCompletionResponse, ToolDefinition,
};
use argus_protocol::tool::{NamedTool, ToolError};
use argus_turn::{TurnBuilder, TurnConfig};
use rust_decimal::Decimal;

/// Mock provider for integration tests
struct MockProvider {
    response: String,
}

impl MockProvider {
    fn new(response: String) -> Self {
        Self { response }
    }
}

#[async_trait::async_trait]
impl LlmProvider for MockProvider {
    fn model_name(&self) -> &str {
        "mock"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionResponse, LlmError> {
        Ok(CompletionResponse {
            content: self.response.clone(),
            reasoning_content: None,
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        Ok(ToolCompletionResponse {
            content: Some(self.response.clone()),
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

/// Echo tool for integration tests
struct EchoTool;

#[async_trait::async_trait]
impl NamedTool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo".to_string(),
            description: "Echo back the input".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        Ok(args)
    }
}

#[tokio::test]
async fn test_turn_integration_simple() {
    let provider = Arc::new(MockProvider::new("Hello, world!".to_string()));

    let (stream_tx, _) = broadcast::channel(256);
    let (thread_event_tx, _) = broadcast::channel(256);

    let turn = TurnBuilder::default()
        .turn_number(1)
        .thread_id("test-thread".to_string())
        .messages(vec![ChatMessage::user("Hello")])
        .provider(provider)
        .tools(vec![])
        .hooks(vec![])
        .config(TurnConfig::default())
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .build()
        .unwrap();

    let output = turn.execute().await.unwrap();

    // Should have user + assistant messages
    assert_eq!(output.messages.len(), 2);
    assert_eq!(output.messages[0].role, Role::User);
    assert_eq!(output.messages[1].role, Role::Assistant);
    assert_eq!(output.messages[1].content, "Hello, world!");
}

#[tokio::test]
async fn test_turn_integration_with_tool() {
    let provider = Arc::new(MockProvider::new(
        "I'll use the echo tool".to_string(),
    ));

    let (stream_tx, _) = broadcast::channel(256);
    let (thread_event_tx, _) = broadcast::channel(256);

    let turn = TurnBuilder::default()
        .turn_number(1)
        .thread_id("test-thread".to_string())
        .messages(vec![ChatMessage::user("Test")])
        .provider(provider)
        .tools(vec![Arc::new(EchoTool)])
        .hooks(vec![])
        .config(TurnConfig::default())
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .build()
        .unwrap();

    let output = turn.execute().await.unwrap();

    // Should have messages
    assert!(!output.messages.is_empty());
    // Should have tracked tokens
    assert!(output.token_usage.total_tokens > 0);
}

#[tokio::test]
async fn test_turn_builder_validation() {
    let provider = Arc::new(MockProvider::new("test".to_string()));

    let (stream_tx, _) = broadcast::channel(256);
    let (thread_event_tx, _) = broadcast::channel(256);

    // Should fail without required fields
    let result = TurnBuilder::default()
        .messages(vec![ChatMessage::user("Hello")])
        .build();

    assert!(result.is_err());

    // Should succeed with all required fields
    let result = TurnBuilder::default()
        .turn_number(1)
        .thread_id("test".to_string())
        .messages(vec![ChatMessage::user("Hello")])
        .provider(provider)
        .tools(vec![])
        .hooks(vec![])
        .config(TurnConfig::default())
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .build();

    assert!(result.is_ok());
}
