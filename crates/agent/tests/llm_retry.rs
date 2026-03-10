use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use agent::llm::{
    CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmEventStream, LlmProvider,
    RetryConfig, RetryProvider, ToolCompletionRequest, ToolCompletionResponse,
};
use async_trait::async_trait;
use futures_core::Stream;
use rust_decimal::Decimal;

struct EmptyStream;

impl Stream for EmptyStream {
    type Item = Result<agent::llm::LlmStreamEvent, LlmError>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

struct FlakyProvider {
    complete_failures: AtomicUsize,
    stream_failures: AtomicUsize,
    calls: AtomicUsize,
}

impl FlakyProvider {
    fn new(complete_failures: usize, stream_failures: usize) -> Self {
        Self {
            complete_failures: AtomicUsize::new(complete_failures),
            stream_failures: AtomicUsize::new(stream_failures),
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl LlmProvider for FlakyProvider {
    fn model_name(&self) -> &str {
        "flaky"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.complete_failures.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |value| value.checked_sub(1),
        ).is_ok() {
            return Err(LlmError::RateLimited {
                provider: "flaky".to_string(),
                retry_after: Some(Duration::ZERO),
            });
        }

        Ok(CompletionResponse {
            content: "ok".to_string(),
            input_tokens: 1,
            output_tokens: 1,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        unimplemented!("tool retry is covered by the same retry path");
    }

    async fn stream_complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.stream_failures.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |value| value.checked_sub(1),
        ).is_ok() {
            return Err(LlmError::RateLimited {
                provider: "flaky".to_string(),
                retry_after: Some(Duration::ZERO),
            });
        }

        Ok(Box::pin(EmptyStream))
    }
}

struct NonRetryableProvider {
    calls: AtomicUsize,
}

#[async_trait]
impl LlmProvider for NonRetryableProvider {
    fn model_name(&self) -> &str {
        "non-retryable"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Err(LlmError::AuthFailed {
            provider: "non-retryable".to_string(),
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        unreachable!()
    }
}

#[tokio::test]
async fn retry_provider_retries_transient_complete_errors_then_succeeds() {
    let inner = Arc::new(FlakyProvider::new(2, 0));
    let provider = RetryProvider::new(inner.clone(), RetryConfig { max_retries: 3 });

    let response = provider
        .complete(CompletionRequest::new(vec![]))
        .await
        .expect("transient errors should be retried");

    assert_eq!(response.content, "ok");
    assert_eq!(inner.calls(), 3);
}

#[tokio::test]
async fn retry_provider_does_not_retry_non_retryable_complete_errors() {
    let inner = Arc::new(NonRetryableProvider {
        calls: AtomicUsize::new(0),
    });
    let provider = RetryProvider::new(inner.clone(), RetryConfig { max_retries: 3 });

    let err = provider
        .complete(CompletionRequest::new(vec![]))
        .await
        .expect_err("auth failures should not be retried");

    assert!(matches!(err, LlmError::AuthFailed { .. }));
    assert_eq!(inner.calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn retry_provider_retries_transient_stream_setup_errors_then_succeeds() {
    let inner = Arc::new(FlakyProvider::new(0, 2));
    let provider = RetryProvider::new(inner.clone(), RetryConfig { max_retries: 3 });

    let _stream = provider
        .stream_complete(CompletionRequest::new(vec![]))
        .await
        .expect("transient stream setup errors should be retried");

    assert_eq!(inner.calls(), 3);
}
