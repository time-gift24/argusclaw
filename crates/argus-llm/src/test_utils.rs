//! Test utilities for injecting failures into providers.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use rust_decimal::Decimal;

use crate::retry::{RetryConfig, RetryProvider};
use argus_protocol::llm::{
    CompletionRequest, CompletionResponse, LlmError, LlmEventStream, LlmProvider,
    ToolCompletionRequest, ToolCompletionResponse,
};

/// Wrapper that injects intermittent failures into any provider.
///
/// Default pattern: Call 1 fails, calls 2-4 fail, call 5+ succeeds.
/// This ensures the first call triggers retries.
pub struct TestRetryProvider {
    inner: Arc<dyn LlmProvider>,
    call_count: AtomicUsize,
    fail_first: bool,
}

impl TestRetryProvider {
    /// Wrap a provider with test retry behavior (first call fails).
    pub fn new(inner: Arc<dyn LlmProvider>) -> Self {
        Self {
            inner,
            call_count: AtomicUsize::new(0),
            fail_first: true,
        }
    }

    /// Wrap a provider with custom fail behavior.
    pub fn with_fail_first(inner: Arc<dyn LlmProvider>, fail_first: bool) -> Self {
        Self {
            inner,
            call_count: AtomicUsize::new(0),
            fail_first,
        }
    }

    /// Check if current call should fail.
    fn should_fail(&self) -> bool {
        let count = self.call_count.fetch_add(1, Ordering::Relaxed);

        if self.fail_first {
            // Pattern: Call 1 fails, calls 2-4 fail, call 5+ succeeds
            // This ensures first call triggers retries
            matches!(count, 0..=3)
        } else {
            // Original pattern: Call 1 succeeds, calls 2-4 fail, call 5+ succeeds
            matches!(count, 1..=3)
        }
    }
}

#[async_trait]
impl LlmProvider for TestRetryProvider {
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        self.inner.cost_per_token()
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        if self.should_fail() {
            return Err(LlmError::RateLimited {
                provider: self.model_name().to_string(),
                retry_after: Some(Duration::from_millis(300)),
            });
        }
        self.inner.complete(request).await
    }

    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        if self.should_fail() {
            return Err(LlmError::RateLimited {
                provider: self.model_name().to_string(),
                retry_after: Some(Duration::from_millis(300)),
            });
        }
        self.inner.complete_with_tools(request).await
    }

    async fn stream_complete(
        &self,
        request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        if self.should_fail() {
            return Err(LlmError::RateLimited {
                provider: self.model_name().to_string(),
                retry_after: Some(Duration::from_millis(300)),
            });
        }
        self.inner.stream_complete(request).await
    }

    async fn stream_complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        if self.should_fail() {
            return Err(LlmError::RateLimited {
                provider: self.model_name().to_string(),
                retry_after: Some(Duration::from_millis(300)),
            });
        }
        self.inner.stream_complete_with_tools(request).await
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        self.inner.list_models().await
    }

    async fn model_metadata(&self) -> Result<argus_protocol::llm::ModelMetadata, LlmError> {
        self.inner.model_metadata().await
    }

    fn effective_model_name(&self, requested_model: Option<&str>) -> String {
        self.inner.effective_model_name(requested_model)
    }

    fn active_model_name(&self) -> String {
        self.inner.active_model_name()
    }

    fn set_model(&self, model: &str) -> Result<(), LlmError> {
        self.inner.set_model(model)
    }

    fn calculate_cost(&self, input_tokens: u32, output_tokens: u32) -> Decimal {
        self.inner.calculate_cost(input_tokens, output_tokens)
    }

    fn cache_write_multiplier(&self) -> Decimal {
        self.inner.cache_write_multiplier()
    }

    fn cache_read_discount(&self) -> Decimal {
        self.inner.cache_read_discount()
    }
}

/// Create a provider with test retry behavior and retry wrapper.
pub fn create_test_retry_provider(
    base_provider: Arc<dyn LlmProvider>,
    max_retries: u32,
) -> Arc<dyn LlmProvider> {
    let test_provider = Arc::new(TestRetryProvider::new(base_provider));
    Arc::new(RetryProvider::new(
        test_provider,
        RetryConfig { max_retries },
    ))
}
