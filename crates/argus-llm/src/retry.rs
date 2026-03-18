//! Shared retry helpers and composable `RetryProvider` decorator for LLM providers.
//!
//! Derived from:
//! - Repository: https://github.com/nearai/ironclaw
//! - Upstream path: src/llm/retry.rs
//! - Upstream commit: bcef04b82108222c9041e733de459130badd4cd7
//! - License: MIT OR Apache-2.0
//!
//! Local modifications:
//! - Adapted retryability classification to ArgusClaw's reduced `LlmError` surface.
//! - Extends retry setup to `stream_complete` and `stream_complete_with_tools`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::Stream;
use rand::RngExt;
use rust_decimal::Decimal;

use argus_protocol::llm::{
    CompletionRequest, CompletionResponse, LlmError, LlmEventStream, LlmProvider, ModelMetadata,
    ToolCompletionRequest, ToolCompletionResponse, LlmStreamEvent,
};

fn is_retryable(err: &LlmError) -> bool {
    matches!(
        err,
        LlmError::RequestFailed { .. }
            | LlmError::RateLimited { .. }
            | LlmError::InvalidResponse { .. }
            | LlmError::SessionRenewalFailed { .. }
    )
}

fn retry_backoff_delay(attempt: u32) -> Duration {
    let base_ms: u64 = 1000u64.saturating_mul(2u64.saturating_pow(attempt));
    let jitter_range = base_ms / 4;
    let jitter = if jitter_range > 0 {
        let offset = rand::rng().random_range(0..=jitter_range * 2);
        offset as i64 - jitter_range as i64
    } else {
        0
    };
    let delay_ms = (base_ms as i64 + jitter).max(100) as u64;
    Duration::from_millis(delay_ms)
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self { max_retries: 3 }
    }
}

pub struct RetryProvider {
    inner: Arc<dyn LlmProvider>,
    config: RetryConfig,
}

struct RetryEventStream {
    retry_events: Vec<LlmStreamEvent>,
    inner_stream: LlmEventStream,
}

impl Stream for RetryEventStream {
    type Item = Result<LlmStreamEvent, LlmError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Yield retry events first
        if !self.retry_events.is_empty() {
            return Poll::Ready(Some(Ok(self.retry_events.remove(0))));
        }
        // Then forward inner stream
        Pin::new(&mut self.inner_stream).poll_next(cx)
    }
}

impl RetryProvider {
    pub fn new(inner: Arc<dyn LlmProvider>, config: RetryConfig) -> Self {
        Self { inner, config }
    }

    async fn retry_loop<T, F, Fut>(
        &self,
        mut op: F,
        label: &str,
    ) -> Result<(T, Vec<LlmStreamEvent>), LlmError>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, LlmError>>,
    {
        let mut last_error: Option<LlmError> = None;
        let mut retry_events = Vec::new();

        for attempt in 0..=self.config.max_retries {
            match op().await {
                Ok(response) => return Ok((response, retry_events)),
                Err(err) => {
                    if !is_retryable(&err) || attempt == self.config.max_retries {
                        return Err(err);
                    }

                    // Collect retry event
                    retry_events.push(LlmStreamEvent::RetryAttempt {
                        attempt: attempt + 1,
                        max_retries: self.config.max_retries,
                        error: err.to_string(),
                    });

                    let delay = match &err {
                        LlmError::RateLimited {
                            retry_after: Some(duration),
                            ..
                        } => *duration,
                        _ => retry_backoff_delay(attempt),
                    };

                    tracing::warn!(
                        provider = %self.inner.active_model_name(),
                        attempt = attempt + 1,
                        max_retries = self.config.max_retries,
                        delay_ms = delay.as_millis() as u64,
                        error = %err,
                        "Retrying after transient error{label}"
                    );

                    last_error = Some(err);
                    tokio::time::sleep(delay).await;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| LlmError::RequestFailed {
            provider: self.inner.active_model_name(),
            reason: "retry loop exited unexpectedly".to_string(),
        }))
    }
}

#[async_trait]
impl LlmProvider for RetryProvider {
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        self.inner.cost_per_token()
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let inner = &self.inner;
        let (response, _retry_events) = self.retry_loop(
            || {
                let req = request.clone();
                async move { inner.complete(req).await }
            },
            "",
        )
        .await?;
        Ok(response)
    }

    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let inner = &self.inner;
        let (response, _retry_events) = self.retry_loop(
            || {
                let req = request.clone();
                async move { inner.complete_with_tools(req).await }
            },
            " (tools)",
        )
        .await?;
        Ok(response)
    }

    async fn stream_complete(
        &self,
        request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let inner = &self.inner;
        let (response, retry_events) = self.retry_loop(
            || {
                let req = request.clone();
                async move { inner.stream_complete(req).await }
            },
            " (stream)",
        )
        .await?;

        Ok(Box::pin(RetryEventStream {
            retry_events,
            inner_stream: response,
        }))
    }

    async fn stream_complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let inner = &self.inner;
        let (response, retry_events) = self.retry_loop(
            || {
                let req = request.clone();
                async move { inner.stream_complete_with_tools(req).await }
            },
            " (stream tools)",
        )
        .await?;

        Ok(Box::pin(RetryEventStream {
            retry_events,
            inner_stream: response,
        }))
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        self.inner.list_models().await
    }

    async fn model_metadata(&self) -> Result<ModelMetadata, LlmError> {
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

#[cfg(test)]
mod tests {
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll};

    use futures_util::Stream;

    use super::*;
    use argus_protocol::llm::{FinishReason, LlmStreamEvent};

    const UPSTREAM_URL: &str = "https://github.com/nearai/ironclaw";
    const UPSTREAM_COMMIT: &str = "bcef04b82108222c9041e733de459130badd4cd7";
    const UPSTREAM_LICENSE: &str = "MIT OR Apache-2.0";

    struct EmptyStream;

    impl Stream for EmptyStream {
        type Item = Result<LlmStreamEvent, LlmError>;

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

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self
                .complete_failures
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                    value.checked_sub(1)
                })
                .is_ok()
            {
                return Err(LlmError::RateLimited {
                    provider: "flaky".to_string(),
                    retry_after: Some(Duration::ZERO),
                });
            }

            Ok(CompletionResponse {
                content: "ok".to_string(),
                reasoning_content: None,
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
            if self
                .stream_failures
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                    value.checked_sub(1)
                })
                .is_ok()
            {
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

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
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

    #[tokio::test]
    async fn retry_provider_emits_retry_events() {
        use futures_util::StreamExt;

        let inner = Arc::new(FlakyProvider::new(2, 2)); // Fails twice for both complete and stream
        let provider = RetryProvider::new(inner.clone(), RetryConfig { max_retries: 3 });

        let stream = provider
            .stream_complete(CompletionRequest::new(vec![]))
            .await
            .expect("should succeed after retries");

        // Collect all events
        let events: Vec<_> = futures_util::StreamExt::collect(stream).await;

        // Should have 2 retry events + finish event
        let retry_events: Vec<_> = events
            .iter()
            .filter_map(|e| e.as_ref().ok())
            .filter_map(|e| match e {
                LlmStreamEvent::RetryAttempt { .. } => Some(e),
                _ => None,
            })
            .collect();

        assert_eq!(retry_events.len(), 2, "expected 2 retry events, got {}", retry_events.len());

        // Verify first retry event structure
        if let LlmStreamEvent::RetryAttempt { attempt, max_retries, error } = retry_events[0] {
            assert_eq!(*attempt, 1, "first attempt should be 1");
            assert_eq!(*max_retries, 3, "max_retries should be 3");
            assert!(error.contains("rate limited"), "error should mention rate limiting, got: {}", error);
        } else {
            panic!("expected RetryAttempt event");
        }

        // Verify second retry event structure
        if let LlmStreamEvent::RetryAttempt { attempt, max_retries, error } = retry_events[1] {
            assert_eq!(*attempt, 2, "second attempt should be 2");
            assert_eq!(*max_retries, 3, "max_retries should be 3");
            assert!(error.contains("rate limited"), "error should mention rate limiting, got: {}", error);
        } else {
            panic!("expected RetryAttempt event");
        }
    }

    #[test]
    fn vendored_retry_file_includes_provenance_header() {
        let retry = include_str!("retry.rs");

        assert!(retry.contains(UPSTREAM_URL));
        assert!(retry.contains(UPSTREAM_COMMIT));
        assert!(retry.contains(UPSTREAM_LICENSE));
    }
}
