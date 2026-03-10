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
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rand::Rng;
use rust_decimal::Decimal;

use crate::llm::error::LlmError;
use crate::llm::provider::{
    CompletionRequest, CompletionResponse, LlmEventStream, LlmProvider, ModelMetadata,
    ToolCompletionRequest, ToolCompletionResponse,
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
        let offset = rand::thread_rng().gen_range(0..=jitter_range * 2);
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

impl RetryProvider {
    pub fn new(inner: Arc<dyn LlmProvider>, config: RetryConfig) -> Self {
        Self { inner, config }
    }

    async fn retry_loop<T, F, Fut>(&self, mut op: F, label: &str) -> Result<T, LlmError>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, LlmError>>,
    {
        let mut last_error: Option<LlmError> = None;

        for attempt in 0..=self.config.max_retries {
            match op().await {
                Ok(response) => return Ok(response),
                Err(err) => {
                    if !is_retryable(&err) || attempt == self.config.max_retries {
                        return Err(err);
                    }

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
        self.retry_loop(
            || {
                let req = request.clone();
                async move { inner.complete(req).await }
            },
            "",
        )
        .await
    }

    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let inner = &self.inner;
        self.retry_loop(
            || {
                let req = request.clone();
                async move { inner.complete_with_tools(req).await }
            },
            " (tools)",
        )
        .await
    }

    async fn stream_complete(
        &self,
        request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let inner = &self.inner;
        self.retry_loop(
            || {
                let req = request.clone();
                async move { inner.stream_complete(req).await }
            },
            " (stream)",
        )
        .await
    }

    async fn stream_complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let inner = &self.inner;
        self.retry_loop(
            || {
                let req = request.clone();
                async move { inner.stream_complete_with_tools(req).await }
            },
            " (stream tools)",
        )
        .await
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
