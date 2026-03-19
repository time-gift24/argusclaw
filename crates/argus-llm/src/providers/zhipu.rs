//! Zhipu AI provider implementation.
//!
//! Zhipu AI (智谱AI) provides GLM series models with OpenAI-compatible API.
//! API Docs: https://open.bigmodel.cn/dev/api

use std::sync::Arc;

use async_trait::async_trait;

use argus_protocol::llm::{
    CompletionRequest, CompletionResponse, LlmError, LlmEventStream, LlmProvider,
    ProviderCapabilities, ToolCompletionRequest, ToolCompletionResponse,
};

use crate::providers::openai_compatible::OpenAiCompatibleProvider;
use crate::retry::{RetryConfig, RetryProvider};

/// Zhipu AI provider configuration.
#[derive(Debug, Clone)]
pub struct ZhipuConfig {
    /// API key from Zhipu AI console.
    pub api_key: String,
    /// Model to use (default: glm-4).
    pub model: String,
    /// Request timeout (default: 60s).
    pub timeout: std::time::Duration,
}

impl ZhipuConfig {
    #[must_use]
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            timeout: std::time::Duration::from_secs(60),
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Factory config for creating Zhipu providers.
#[derive(Debug, Clone)]
pub struct ZhipuFactoryConfig {
    pub provider: ZhipuConfig,
    pub retry: Option<RetryConfig>,
}

impl ZhipuFactoryConfig {
    #[must_use]
    pub fn new(provider: ZhipuConfig) -> Self {
        Self {
            provider,
            retry: None,
        }
    }

    #[must_use]
    pub fn with_retry(mut self, retry: RetryConfig) -> Self {
        self.retry = Some(retry);
        self
    }
}

/// Zhipu AI base URL.
const ZHIPU_BASE_URL: &str = "https://open.bigmodel.cn/api/paas/v4";

/// Create a Zhipu AI provider.
pub fn create_zhipu_provider(config: ZhipuFactoryConfig) -> Result<Arc<dyn LlmProvider>, LlmError> {
    let provider: Arc<dyn LlmProvider> = Arc::new(ZhipuProvider::new(config.provider)?);
    if let Some(retry) = config.retry {
        Ok(Arc::new(RetryProvider::new(provider, retry)))
    } else {
        Ok(provider)
    }
}

/// Zhipu AI provider implementation.
///
/// This provider wraps OpenAiCompatibleProvider with Zhipu-specific defaults:
/// - Base URL: https://open.bigmodel.cn/api/paas/v4
/// - Auth header: Bearer token
/// - Model-specific thinking support for GLM-4.5+ series
pub struct ZhipuProvider {
    inner: OpenAiCompatibleProvider,
}

impl ZhipuProvider {
    /// Create a new Zhipu provider.
    pub fn new(config: ZhipuConfig) -> Result<Self, LlmError> {
        let inner = OpenAiCompatibleProvider::new(
            crate::providers::openai_compatible::OpenAiCompatibleConfig::new(
                ZHIPU_BASE_URL,
                config.api_key,
                config.model,
            )
            .with_timeout(config.timeout),
        )?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl LlmProvider for ZhipuProvider {
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    fn cost_per_token(&self) -> (rust_decimal::Decimal, rust_decimal::Decimal) {
        self.inner.cost_per_token()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.inner.capabilities()
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.inner.complete(request).await
    }

    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        self.inner.complete_with_tools(request).await
    }

    async fn stream_complete(&self, request: CompletionRequest) -> Result<LlmEventStream, LlmError> {
        self.inner.stream_complete(request).await
    }

    async fn stream_complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        self.inner.stream_complete_with_tools(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zhipu_provider_reports_thinking_for_glm45() {
        let provider = ZhipuProvider::new(
            ZhipuConfig::new("test-key", "glm-4.5")
                .with_timeout(std::time::Duration::from_secs(30)),
        )
        .expect("provider should build");

        assert!(
            provider.capabilities().thinking,
            "GLM-4.5 should support thinking"
        );
    }

    #[test]
    fn zhipu_provider_reports_thinking_for_glm5() {
        let provider = ZhipuProvider::new(ZhipuConfig::new("test-key", "glm-5"))
            .expect("provider should build");

        assert!(
            provider.capabilities().thinking,
            "GLM-5 should support thinking"
        );
    }

    #[test]
    fn zhipu_provider_reports_no_thinking_for_legacy_model() {
        let provider = ZhipuProvider::new(ZhipuConfig::new("test-key", "glm-3-turbo"))
            .expect("provider should build");

        assert!(
            !provider.capabilities().thinking,
            "GLM-3 should not support thinking"
        );
    }

    #[test]
    fn factory_can_wrap_provider_with_retry() {
        let config = ZhipuFactoryConfig::new(ZhipuConfig::new("test-key", "glm-4"))
            .with_retry(RetryConfig { max_retries: 2 });

        let provider = create_zhipu_provider(config).expect("factory should build provider");

        assert_eq!(provider.model_name(), "glm-4");
    }
}
