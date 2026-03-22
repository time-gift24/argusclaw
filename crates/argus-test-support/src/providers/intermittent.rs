//! Provider that simulates intermittent failures.
//!
//! Pattern: Succeeds once, then fails 3 times, then succeeds again.
//! Useful for testing retry recovery from transient failures.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use rust_decimal::Decimal;

use argus_protocol::llm::{
    CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmEventStream, LlmProvider,
    ToolCompletionRequest, ToolCompletionResponse,
};

/// Provider that succeeds on first call, fails on next 3 calls, then succeeds.
pub struct IntermittentFailureProvider {
    call_count: AtomicUsize,
}

impl IntermittentFailureProvider {
    /// Create a new provider with intermittent failure pattern.
    ///
    /// # Pattern
    /// - Call 1: Success
    /// - Call 2-4: Failure (RateLimited)
    /// - Call 5+: Success
    pub fn new() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
        }
    }

    /// Get current call count.
    pub fn calls(&self) -> usize {
        self.call_count.load(Ordering::Relaxed)
    }

    /// Check if current call should fail.
    fn should_fail(&self) -> bool {
        let count = self.call_count.fetch_add(1, Ordering::Relaxed);
        // Fail on calls 2, 3, 4 (1-indexed: 2, 3, 4)
        matches!(count, 1..=3)
    }
}

impl Default for IntermittentFailureProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmProvider for IntermittentFailureProvider {
    fn model_name(&self) -> &str {
        "intermittent-failure"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        if self.should_fail() {
            return Err(LlmError::RateLimited {
                provider: "intermittent-failure".to_string(),
                retry_after: Some(Duration::from_millis(300)),
            });
        }

        Ok(CompletionResponse {
            content: "Success after intermittent failures".to_string(),
            reasoning_content: None,
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            reasoning_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        if self.should_fail() {
            return Err(LlmError::RateLimited {
                provider: "intermittent-failure".to_string(),
                retry_after: Some(Duration::from_millis(300)),
            });
        }

        Ok(ToolCompletionResponse {
            content: Some("Success after intermittent failures".to_string()),
            reasoning_content: None,
            tool_calls: vec![],
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            reasoning_tokens: 0,
        })
    }

    async fn stream_complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        if self.should_fail() {
            return Err(LlmError::RateLimited {
                provider: "intermittent-failure".to_string(),
                retry_after: Some(Duration::from_millis(300)),
            });
        }

        // Simple stream that emits a finish event
        let stream = futures_util::stream::once(async move {
            Ok(argus_protocol::llm::LlmStreamEvent::Finished {
                finish_reason: FinishReason::Stop,
            })
        });

        Ok(Box::pin(stream))
    }

    async fn stream_complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        if self.should_fail() {
            return Err(LlmError::RateLimited {
                provider: "intermittent-failure".to_string(),
                retry_after: Some(Duration::from_millis(300)),
            });
        }

        let stream = futures_util::stream::once(async move {
            Ok(argus_protocol::llm::LlmStreamEvent::Finished {
                finish_reason: FinishReason::Stop,
            })
        });

        Ok(Box::pin(stream))
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        Ok(vec!["intermittent-failure".to_string()])
    }

    async fn model_metadata(&self) -> Result<argus_protocol::llm::ModelMetadata, LlmError> {
        Ok(argus_protocol::llm::ModelMetadata {
            id: "intermittent-failure".to_string(),
            context_length: Some(128000),
        })
    }

    fn effective_model_name(&self, requested_model: Option<&str>) -> String {
        requested_model
            .unwrap_or("intermittent-failure")
            .to_string()
    }

    fn active_model_name(&self) -> String {
        "intermittent-failure".to_string()
    }

    fn set_model(&self, _model: &str) -> Result<(), LlmError> {
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_intermittent_failure_pattern() {
        let provider = IntermittentFailureProvider::new();

        // Call 1: Success
        assert!(provider
            .complete(CompletionRequest::new(vec![]))
            .await
            .is_ok());

        // Calls 2-4: Failures
        for i in 2..=4 {
            let result = provider.complete(CompletionRequest::new(vec![])).await;
            assert!(result.is_err(), "Call {} should fail", i);
            assert!(matches!(result.unwrap_err(), LlmError::RateLimited { .. }));
        }

        // Call 5: Success
        assert!(provider
            .complete(CompletionRequest::new(vec![]))
            .await
            .is_ok());

        assert_eq!(provider.calls(), 5);
    }
}
