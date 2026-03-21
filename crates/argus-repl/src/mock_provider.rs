//! Mock LLM provider for REPL testing.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::stream;
use rust_decimal::Decimal;

use argus_protocol::llm::{
    CompletionRequest, CompletionResponse, FinishReason, LlmError,
    LlmEventStream, LlmProvider, LlmStreamEvent, ModelMetadata, ProviderCapabilities,
    ToolCompletionRequest, ToolCompletionResponse,
};

static REPL_MOCK_RESPONSES: &[&str] = &[
    "收到。这是第 1 轮对话。",
    "这是第 2 轮。上下文持续累积。",
    "这是第 3 轮。你的消息历史在增长。",
    "这是第 4 轮。我们可以继续测试。",
    "这是第 5 轮。上下文应该已经相当长了。",
];

/// Mock provider for REPL testing.
#[derive(Debug, Clone)]
pub struct ReplMockProvider {
    counter: Arc<AtomicUsize>,
}

impl ReplMockProvider {
    #[must_use]
    pub fn new() -> Self {
        Self {
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn next_response(&self) -> String {
        let idx = self.counter.fetch_add(1, Ordering::SeqCst);
        REPL_MOCK_RESPONSES[idx % REPL_MOCK_RESPONSES.len()].to_string()
    }
}

impl Default for ReplMockProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmProvider for ReplMockProvider {
    fn model_name(&self) -> &str {
        "mock"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities { thinking: true }
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionResponse, LlmError> {
        let text = self.next_response();
        let output_tokens = text.chars().count() as u32;
        Ok(CompletionResponse {
            content: text,
            reasoning_content: None,
            finish_reason: FinishReason::Stop,
            input_tokens: 10,
            output_tokens,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let text = self.next_response();
        let output_tokens = text.chars().count() as u32;
        Ok(ToolCompletionResponse {
            content: Some(text),
            reasoning_content: None,
            tool_calls: vec![],
            finish_reason: FinishReason::Stop,
            input_tokens: 10,
            output_tokens,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn stream_complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let text = self.next_response();
        let input_tokens = 10u32;
        let output_tokens = text.chars().count() as u32;

        let events = vec![
            Ok(LlmStreamEvent::ContentDelta { delta: text }),
            Ok(LlmStreamEvent::Finished { finish_reason: FinishReason::Stop }),
            Ok(LlmStreamEvent::Usage { input_tokens, output_tokens }),
        ];

        let stream: LlmEventStream = Box::pin(stream::iter(events));
        Ok(stream)
    }

    async fn model_metadata(&self) -> Result<ModelMetadata, LlmError> {
        Ok(ModelMetadata {
            id: "mock".to_string(),
            context_length: Some(128_000),
        })
    }

    fn effective_model_name(&self, requested_model: Option<&str>) -> String {
        requested_model.map(String::from).unwrap_or_else(|| "mock".to_string())
    }

    fn active_model_name(&self) -> String {
        "mock".to_string()
    }

    fn context_window(&self) -> u32 {
        128_000
    }

    fn set_model(&self, _model: &str) -> Result<(), LlmError> {
        Err(LlmError::RequestFailed {
            provider: "mock".to_string(),
            reason: "Runtime model switching not supported by mock provider".to_string(),
        })
    }

    fn calculate_cost(&self, _input_tokens: u32, _output_tokens: u32) -> Decimal {
        Decimal::ZERO
    }

    fn cache_write_multiplier(&self) -> Decimal {
        Decimal::ONE
    }

    fn cache_read_discount(&self) -> Decimal {
        Decimal::ONE
    }
}
