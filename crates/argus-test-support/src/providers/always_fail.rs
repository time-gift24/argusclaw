//! Provider that always fails with a retryable error.
//!
//! Useful for testing retry exhaustion and error reporting.

use std::time::Duration;

use async_trait::async_trait;
use rust_decimal::Decimal;

use argus_protocol::llm::{
    CompletionRequest, CompletionResponse, LlmError, LlmEventStream, LlmProvider,
    ToolCompletionRequest, ToolCompletionResponse,
};

/// Provider that always fails with a retryable error.
pub struct AlwaysFailProvider {
    _private: (),
}

impl AlwaysFailProvider {
    /// Create a new provider that always fails.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for AlwaysFailProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmProvider for AlwaysFailProvider {
    fn model_name(&self) -> &str {
        "always-fail"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Err(LlmError::RateLimited {
            provider: "always-fail".to_string(),
            retry_after: Some(Duration::from_millis(300)),
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        Err(LlmError::RateLimited {
            provider: "always-fail".to_string(),
            retry_after: Some(Duration::from_millis(300)),
        })
    }

    async fn stream_complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        Err(LlmError::RateLimited {
            provider: "always-fail".to_string(),
            retry_after: Some(Duration::from_millis(300)),
        })
    }

    async fn stream_complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        Err(LlmError::RateLimited {
            provider: "always-fail".to_string(),
            retry_after: Some(Duration::from_millis(300)),
        })
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        Err(LlmError::RateLimited {
            provider: "always-fail".to_string(),
            retry_after: None,
        })
    }

    async fn model_metadata(&self) -> Result<argus_protocol::llm::ModelMetadata, LlmError> {
        Err(LlmError::RateLimited {
            provider: "always-fail".to_string(),
            retry_after: None,
        })
    }

    fn effective_model_name(&self, requested_model: Option<&str>) -> String {
        requested_model.unwrap_or("always-fail").to_string()
    }

    fn active_model_name(&self) -> String {
        "always-fail".to_string()
    }

    fn set_model(&self, _model: &str) -> Result<(), LlmError> {
        Err(LlmError::RateLimited {
            provider: "always-fail".to_string(),
            retry_after: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_always_fail() {
        let provider = AlwaysFailProvider::new();

        for i in 1..=5 {
            let result = provider.complete(CompletionRequest::new(vec![])).await;
            assert!(result.is_err(), "Call {} should fail", i);
        }
    }
}
