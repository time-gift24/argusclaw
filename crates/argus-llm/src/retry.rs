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
//! - Extends retry setup to `stream_complete`.

use std::collections::VecDeque;
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
    CompletionRequest, CompletionResponse, LlmError, LlmEventStream, LlmProvider, LlmStreamEvent,
    ModelMetadata,
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
    // Exponential backoff from 300ms to 5000ms
    let base_ms: u64 = 300u64.saturating_mul(2u64.saturating_pow(attempt));
    let base_ms = base_ms.min(5000); // Cap at 5000ms

    let jitter_range = base_ms / 4;
    let jitter = if jitter_range > 0 {
        let offset = rand::rng().random_range(0..=jitter_range * 2);
        offset as i64 - jitter_range as i64
    } else {
        0
    };
    let delay_ms = (base_ms as i64 + jitter).max(300) as u64;
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

type StreamRestartFuture = Pin<Box<dyn Future<Output = Result<LlmEventStream, LlmError>> + Send>>;
type StreamRestartFn = Box<dyn FnMut() -> StreamRestartFuture + Send>;

struct RetryEventStream {
    pending_events: VecDeque<LlmStreamEvent>,
    inner_stream: Option<LlmEventStream>,
    restart_stream: StreamRestartFn,
    restart_delay: Option<Pin<Box<tokio::time::Sleep>>>,
    restart_future: Option<StreamRestartFuture>,
    max_retries: u32,
    retries_used: u32,
    emitted_substantive_event: bool,
    provider_name: String,
    stream_label: &'static str,
}

impl RetryEventStream {
    fn new(
        retry_events: Vec<LlmStreamEvent>,
        inner_stream: LlmEventStream,
        restart_stream: StreamRestartFn,
        max_retries: u32,
        retries_used: u32,
        provider_name: String,
        stream_label: &'static str,
    ) -> Self {
        Self {
            pending_events: retry_events.into(),
            inner_stream: Some(inner_stream),
            restart_stream,
            restart_delay: None,
            restart_future: None,
            max_retries,
            retries_used,
            emitted_substantive_event: false,
            provider_name,
            stream_label,
        }
    }

    fn should_retry_stream_error(&self, err: &LlmError) -> bool {
        !self.emitted_substantive_event && self.retries_used < self.max_retries && is_retryable(err)
    }

    fn queue_stream_retry(&mut self, err: &LlmError) {
        let attempt = self.retries_used + 1;
        self.retries_used += 1;

        self.pending_events.push_back(LlmStreamEvent::RetryAttempt {
            attempt,
            max_retries: self.max_retries,
            error: err.to_string(),
        });

        let delay = match err {
            LlmError::RateLimited {
                retry_after: Some(duration),
                ..
            } => *duration,
            _ => retry_backoff_delay(attempt - 1),
        };

        tracing::warn!(
            provider = %self.provider_name,
            attempt,
            max_retries = self.max_retries,
            delay_ms = delay.as_millis() as u64,
            error = %err,
            "Retrying after transient early stream error{}",
            self.stream_label
        );

        self.inner_stream = None;
        self.restart_future = None;
        self.restart_delay = Some(Box::pin(tokio::time::sleep(delay)));
    }
}

fn is_substantive_stream_event(event: &LlmStreamEvent) -> bool {
    !matches!(event, LlmStreamEvent::RetryAttempt { .. })
}

impl Stream for RetryEventStream {
    type Item = Result<LlmStreamEvent, LlmError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(event) = self.pending_events.pop_front() {
                return Poll::Ready(Some(Ok(event)));
            }

            if let Some(delay) = self.restart_delay.as_mut() {
                match delay.as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(()) => {
                        self.restart_delay = None;
                        self.restart_future = Some((self.restart_stream)());
                        continue;
                    }
                }
            }

            if let Some(future) = self.restart_future.as_mut() {
                match future.as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(stream)) => {
                        self.restart_future = None;
                        self.inner_stream = Some(stream);
                        continue;
                    }
                    Poll::Ready(Err(err)) => {
                        self.restart_future = None;
                        if self.should_retry_stream_error(&err) {
                            self.queue_stream_retry(&err);
                            continue;
                        }
                        return Poll::Ready(Some(Err(err)));
                    }
                }
            }

            let Some(inner_stream) = self.inner_stream.as_mut() else {
                return Poll::Ready(None);
            };

            match inner_stream.as_mut().poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Ok(event))) => {
                    if is_substantive_stream_event(&event) {
                        self.emitted_substantive_event = true;
                    }
                    return Poll::Ready(Some(Ok(event)));
                }
                Poll::Ready(Some(Err(err))) => {
                    if self.should_retry_stream_error(&err) {
                        self.queue_stream_retry(&err);
                        continue;
                    }
                    return Poll::Ready(Some(Err(err)));
                }
                Poll::Ready(None) => return Poll::Ready(None),
            }
        }
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
    ) -> Result<(T, Vec<LlmStreamEvent>), (LlmError, Vec<LlmStreamEvent>)>
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
                        return Err((err, retry_events));
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

        Err((
            last_error.unwrap_or_else(|| LlmError::RequestFailed {
                provider: self.inner.active_model_name(),
                reason: "retry loop exited unexpectedly".to_string(),
            }),
            retry_events,
        ))
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
        let (response, _retry_events) = self
            .retry_loop(
                || {
                    let req = request.clone();
                    async move { inner.complete(req).await }
                },
                "",
            )
            .await
            .map_err(|(err, _events)| err)?;
        Ok(response)
    }

    async fn stream_complete(
        &self,
        request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let inner = self.inner.clone();
        let result = self
            .retry_loop(
                || {
                    let req = request.clone();
                    let inner = inner.clone();
                    async move { inner.stream_complete(req).await }
                },
                " (stream)",
            )
            .await;

        match result {
            Ok((stream, retry_events)) => {
                let setup_retry_count = retry_events.len() as u32;
                let restart_inner = self.inner.clone();
                let restart_request = request.clone();

                Ok(Box::pin(RetryEventStream::new(
                    retry_events,
                    stream,
                    Box::new(move || {
                        let inner = restart_inner.clone();
                        let request = restart_request.clone();
                        Box::pin(async move { inner.stream_complete(request).await })
                    }),
                    self.config.max_retries,
                    setup_retry_count,
                    self.inner.active_model_name(),
                    " (stream)",
                )))
            }
            Err((err, retry_events)) => {
                // Even on failure, return retry events in the stream before the error
                use futures_util::{StreamExt, stream};

                let retry_event_stream = stream::iter(retry_events.into_iter().map(Ok));
                let error_stream = stream::once(async move { Err(err) });
                let combined_stream = retry_event_stream.chain(error_stream);
                Ok(Box::pin(combined_stream))
            }
        }
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

    use futures_util::{Stream, StreamExt, stream};

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
                content: Some("ok".to_string()),
                reasoning_content: None,
                tool_calls: vec![],
                input_tokens: 1,
                output_tokens: 1,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
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

    struct RecoverableStreamProvider {
        calls: AtomicUsize,
        fail_before_first_event: AtomicUsize,
        fail_after_first_event: bool,
    }

    impl RecoverableStreamProvider {
        fn fail_before_first_event(times: usize) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                fail_before_first_event: AtomicUsize::new(times),
                fail_after_first_event: false,
            }
        }

        fn fail_after_first_event() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                fail_before_first_event: AtomicUsize::new(0),
                fail_after_first_event: true,
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl LlmProvider for RecoverableStreamProvider {
        fn model_name(&self) -> &str {
            "recoverable-stream"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            unreachable!()
        }

        async fn stream_complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<LlmEventStream, LlmError> {
            self.calls.fetch_add(1, Ordering::Relaxed);

            if self
                .fail_before_first_event
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                    value.checked_sub(1)
                })
                .is_ok()
            {
                return Ok(Box::pin(stream::iter(vec![Err(LlmError::RequestFailed {
                    provider: "recoverable-stream".to_string(),
                    reason: "connection reset".to_string(),
                })])));
            }

            if self.fail_after_first_event {
                return Ok(Box::pin(stream::iter(vec![
                    Ok(LlmStreamEvent::ContentDelta {
                        delta: "partial".to_string(),
                    }),
                    Err(LlmError::RequestFailed {
                        provider: "recoverable-stream".to_string(),
                        reason: "connection reset".to_string(),
                    }),
                ])));
            }

            Ok(Box::pin(stream::iter(vec![
                Ok(LlmStreamEvent::ContentDelta {
                    delta: "ok".to_string(),
                }),
                Ok(LlmStreamEvent::Finished {
                    finish_reason: FinishReason::Stop,
                }),
            ])))
        }
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
                reason: "auth error".to_string(),
            })
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

        assert_eq!(response.content, Some("ok".to_string()));
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
            .filter(|e| matches!(e, LlmStreamEvent::RetryAttempt { .. }))
            .collect();

        assert_eq!(
            retry_events.len(),
            2,
            "expected 2 retry events, got {}",
            retry_events.len()
        );

        // Verify first retry event structure
        if let LlmStreamEvent::RetryAttempt {
            attempt,
            max_retries,
            error,
        } = retry_events[0]
        {
            assert_eq!(*attempt, 1, "first attempt should be 1");
            assert_eq!(*max_retries, 3, "max_retries should be 3");
            assert!(
                error.contains("rate limited"),
                "error should mention rate limiting, got: {}",
                error
            );
        } else {
            panic!("expected RetryAttempt event");
        }

        // Verify second retry event structure
        if let LlmStreamEvent::RetryAttempt {
            attempt,
            max_retries,
            error,
        } = retry_events[1]
        {
            assert_eq!(*attempt, 2, "second attempt should be 2");
            assert_eq!(*max_retries, 3, "max_retries should be 3");
            assert!(
                error.contains("rate limited"),
                "error should mention rate limiting, got: {}",
                error
            );
        } else {
            panic!("expected RetryAttempt event");
        }
    }

    #[tokio::test]
    async fn retry_provider_retries_stream_errors_before_first_event() {
        let inner = Arc::new(RecoverableStreamProvider::fail_before_first_event(1));
        let provider = RetryProvider::new(inner.clone(), RetryConfig { max_retries: 3 });

        let events = provider
            .stream_complete(CompletionRequest::new(vec![]))
            .await
            .expect("stream should recover")
            .collect::<Vec<_>>()
            .await;

        assert_eq!(
            inner.calls(),
            2,
            "should reconnect once after early stream failure"
        );
        assert_eq!(events.len(), 3, "retry event + content + finish");
        assert!(matches!(
            &events[0],
            Ok(LlmStreamEvent::RetryAttempt { attempt: 1, .. })
        ));
        assert!(matches!(
            &events[1],
            Ok(LlmStreamEvent::ContentDelta { delta }) if delta == "ok"
        ));
        assert!(matches!(
            &events[2],
            Ok(LlmStreamEvent::Finished {
                finish_reason: FinishReason::Stop
            })
        ));
    }

    #[tokio::test]
    async fn retry_provider_does_not_retry_stream_errors_after_output_started() {
        let inner = Arc::new(RecoverableStreamProvider::fail_after_first_event());
        let provider = RetryProvider::new(inner.clone(), RetryConfig { max_retries: 3 });

        let events = provider
            .stream_complete(CompletionRequest::new(vec![]))
            .await
            .expect("stream should start")
            .collect::<Vec<_>>()
            .await;

        assert_eq!(
            inner.calls(),
            1,
            "should not reconnect after content started"
        );
        assert!(matches!(
            &events[0],
            Ok(LlmStreamEvent::ContentDelta { delta }) if delta == "partial"
        ));
        assert!(matches!(
            &events[1],
            Err(LlmError::RequestFailed { provider, reason })
                if provider == "recoverable-stream" && reason == "connection reset"
        ));
    }

    #[test]
    fn vendored_retry_file_includes_provenance_header() {
        let retry = include_str!("retry.rs");

        assert!(retry.contains(UPSTREAM_URL));
        assert!(retry.contains(UPSTREAM_COMMIT));
        assert!(retry.contains(UPSTREAM_LICENSE));
    }
}
