//! Provider manager for LLM provider lookup and instantiation.
//!
//! This module provides `ProviderManager` which handles:
//! - Looking up provider records from a repository
//! - Building LLM provider instances from records
//! - Testing provider connections

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;

use argus_auth::{TokenContext, TokenLLMProvider, TokenSource, UserCredentialTokenSource};
use argus_protocol::Result;
use argus_protocol::llm::{
    ChatMessage, FinishReason, LlmError, LlmProvider, LlmProviderId, LlmProviderKind,
    LlmProviderRecord, LlmProviderRepository, LlmStreamEvent, ProviderSecretStatus,
    ProviderTestResult, ProviderTestStatus, ToolCall, ToolCompletionRequest,
    ToolCompletionResponse, ToolDefinition,
};
use futures_util::StreamExt;

use crate::providers::{
    OpenAiCompatibleConfig, OpenAiCompatibleFactoryConfig, create_openai_compatible_provider,
};
use crate::retry::{RetryConfig, RetryProvider};

/// Manager for LLM provider lookup and instantiation.
///
/// This is the main entry point for obtaining LLM provider instances
/// from stored configuration.
pub struct ProviderManager {
    repository: Arc<dyn LlmProviderRepository>,
    token_context: Option<Arc<TokenContext>>,
}

impl ProviderManager {
    /// Create a new provider manager with the given repository (no token auth support).
    #[must_use]
    pub fn new(repository: Arc<dyn LlmProviderRepository>) -> Self {
        Self { repository, token_context: None }
    }

    /// Create a new provider manager with token context (supports token-based auth).
    #[must_use]
    pub fn with_token_context(
        repository: Arc<dyn LlmProviderRepository>,
        token_context: Arc<TokenContext>,
    ) -> Self {
        Self {
            repository,
            token_context: Some(token_context),
        }
    }

    /// List all provider records.
    pub async fn list_providers(&self) -> Result<Vec<LlmProviderRecord>> {
        self.repository.list_providers().await
    }

    /// Get a provider instance by ID (using the default model).
    pub async fn get_provider(&self, id: &LlmProviderId) -> Result<Arc<dyn LlmProvider>> {
        let record = self
            .repository
            .get_provider(id)
            .await?
            .ok_or_else(|| argus_protocol::ArgusError::ProviderNotFound(id.into_inner()))?;

        let default_model = record.default_model.clone();
        self.build_provider_with_model(record, &default_model).await
    }

    /// Get a provider instance by ID with a specific model.
    pub async fn get_provider_with_model(
        &self,
        id: &LlmProviderId,
        model: &str,
    ) -> Result<Arc<dyn LlmProvider>> {
        let record = self
            .repository
            .get_provider(id)
            .await?
            .ok_or_else(|| argus_protocol::ArgusError::ProviderNotFound(id.into_inner()))?;

        if !record.models.contains(&model.to_string()) {
            return Err(argus_protocol::ArgusError::LlmError {
                reason: format!("model {} not available on provider {}", model, id),
            });
        }

        self.build_provider_with_model(record, model).await
    }

    /// Get the default provider instance.
    pub async fn get_default_provider(&self) -> Result<Arc<dyn LlmProvider>> {
        let record = self
            .repository
            .get_default_provider()
            .await?
            .ok_or(argus_protocol::ArgusError::ProviderNotFound(0))?;

        let default_model = record.default_model.clone();
        self.build_provider_with_model(record, &default_model).await
    }

    /// Upsert a provider record.
    pub async fn upsert_provider(&self, record: LlmProviderRecord) -> Result<LlmProviderId> {
        let record = LlmProviderRecord {
            secret_status: ProviderSecretStatus::Ready,
            ..record
        };
        self.repository.upsert_provider(&record).await
    }

    /// Delete a provider by ID.
    pub async fn delete_provider(&self, id: &LlmProviderId) -> Result<bool> {
        self.repository.delete_provider(id).await
    }

    /// Import multiple provider records.
    pub async fn import_providers(&self, records: Vec<LlmProviderRecord>) -> Result<()> {
        for record in records {
            self.upsert_provider(record).await?;
        }

        Ok(())
    }

    /// Get a provider record by ID.
    pub async fn get_provider_record(&self, id: &LlmProviderId) -> Result<LlmProviderRecord> {
        self.repository
            .get_provider(id)
            .await?
            .ok_or_else(|| argus_protocol::ArgusError::ProviderNotFound(id.into_inner()))
    }

    /// Get the default provider record.
    pub async fn get_default_provider_record(&self) -> Result<LlmProviderRecord> {
        self.repository
            .get_default_provider()
            .await?
            .ok_or(argus_protocol::ArgusError::ProviderNotFound(0))
    }

    /// Set the default provider.
    pub async fn set_default_provider(&self, id: &LlmProviderId) -> Result<()> {
        self.repository.set_default_provider(id).await
    }

    /// Test a provider connection.
    pub async fn test_provider_connection(
        &self,
        id: &LlmProviderId,
        model: &str,
    ) -> Result<ProviderTestResult> {
        let Some(record) = self.repository.get_provider(id).await? else {
            return Ok(build_provider_test_result(
                id.to_string(),
                String::new(),
                String::new(),
                Duration::ZERO,
                ProviderTestStatus::ProviderNotFound,
                format!("provider {} not found", id),
                None,
                None,
            ));
        };

        let provider_id = record.id.to_string();
        let base_url = record.base_url.clone();
        let provider = match self.build_provider_with_model(record.clone(), model).await {
            Ok(provider) => provider,
            Err(argus_protocol::ArgusError::LlmError { reason }) => {
                return Ok(build_provider_test_result(
                    provider_id,
                    model.to_string(),
                    base_url,
                    Duration::ZERO,
                    ProviderTestStatus::UnsupportedProviderKind,
                    reason,
                    None,
                    None,
                ));
            }
            Err(e) => return Err(e),
        };

        Ok(run_provider_connection_test(provider_id, model.to_string(), base_url, provider).await)
    }

    /// Test a provider record (without saving).
    pub async fn test_provider_record(
        &self,
        record: LlmProviderRecord,
        model: &str,
    ) -> Result<ProviderTestResult> {
        let provider_id = record.id.to_string();
        let base_url = record.base_url.clone();
        let provider = match self.build_provider_with_model(record, model).await {
            Ok(provider) => provider,
            Err(argus_protocol::ArgusError::LlmError { reason }) => {
                return Ok(build_provider_test_result(
                    provider_id,
                    model.to_string(),
                    base_url,
                    Duration::ZERO,
                    ProviderTestStatus::UnsupportedProviderKind,
                    reason,
                    None,
                    None,
                ));
            }
            Err(e) => return Err(e),
        };

        Ok(run_provider_connection_test(provider_id, model.to_string(), base_url, provider).await)
    }

    async fn build_provider_with_model(
        &self,
        record: LlmProviderRecord,
        model: &str,
    ) -> Result<Arc<dyn LlmProvider>> {
        let base = self.build_base_provider(&record, model)?;

        let provider = if let (Some(credential_id), Some(ctx)) =
            (record.credential_id, &self.token_context)
        {
            match self.build_token_provider(ctx, credential_id, base.clone()).await {
                Ok(wrapped) => wrapped,
                Err(e) => {
                    tracing::warn!(%credential_id, "failed to build token provider, falling back to base: {}", e);
                    base
                }
            }
        } else {
            base
        };

        Ok(Arc::new(RetryProvider::new(provider, RetryConfig::default())))
    }

    fn build_base_provider(
        &self,
        record: &LlmProviderRecord,
        model: &str,
    ) -> Result<Arc<dyn LlmProvider>> {
        match record.kind {
            LlmProviderKind::OpenAiCompatible => {
                let mut config = OpenAiCompatibleConfig::new(
                    record.base_url.clone(),
                    record.api_key.expose_secret().to_string(),
                    model.to_string(),
                );

                for (name, value) in &record.extra_headers {
                    config = config.with_extra_header(name, value);
                }

                let factory_config = OpenAiCompatibleFactoryConfig::new(config);

                create_openai_compatible_provider(factory_config).map_err(|e| {
                    argus_protocol::ArgusError::LlmError {
                        reason: e.to_string(),
                    }
                })
            }
        }
    }

    async fn build_token_provider(
        &self,
        ctx: &TokenContext,
        credential_id: i64,
        inner: Arc<dyn LlmProvider>,
    ) -> std::result::Result<Arc<dyn LlmProvider>, argus_protocol::ArgusError> {
        let cred = ctx
            .credential_store
            .get(credential_id)
            .await
            .map_err(|e| argus_protocol::ArgusError::LlmError {
                reason: e.to_string(),
            })?
            .ok_or_else(|| argus_protocol::ArgusError::LlmError {
                reason: format!("credential {} not found", credential_id),
            })?;

        let token_source: Arc<dyn TokenSource> = Arc::new(UserCredentialTokenSource::new(
            ctx.config.clone(),
            cred.username.clone(),
            cred.password.clone(),
        ));

        let wrapped = TokenLLMProvider::new(
            inner,
            token_source,
            cred.username,
            cred.password,
            ctx.config.refresh_interval,
        );

        Ok(Arc::new(wrapped))
    }
}

#[allow(clippy::too_many_arguments)]
fn build_provider_test_result(
    provider_id: String,
    model: String,
    base_url: String,
    latency: Duration,
    status: ProviderTestStatus,
    message: String,
    request: Option<String>,
    response: Option<String>,
) -> ProviderTestResult {
    ProviderTestResult {
        provider_id,
        model,
        base_url,
        checked_at: Utc::now(),
        latency_ms: duration_to_millis(latency),
        status,
        message,
        request,
        response,
    }
}

async fn run_provider_connection_test(
    provider_id: String,
    model: String,
    base_url: String,
    provider: Arc<dyn LlmProvider>,
) -> ProviderTestResult {
    let started = std::time::Instant::now();

    // 定义一个简单的 echo 工具
    let echo_tool = ToolDefinition {
        name: "echo".to_string(),
        description: "Repeat back the input text exactly as received.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "The text to echo back"
                }
            },
            "required": ["text"]
        }),
    };

    // 使用 ToolCompletionRequest，要求模型调用工具
    let request = ToolCompletionRequest::new(
        vec![ChatMessage::user(
            "Please call the echo tool with the text 'OK'",
        )],
        vec![echo_tool],
    )
    .with_model(&model)
    .with_temperature(0.0);

    let request_json = serde_json::to_string(&request).ok();

    // 尝试使用流式 API
    match provider.stream_complete_with_tools(request.clone()).await {
        Ok(stream) => {
            let mut accumulator = TestStreamingAccumulator::new();

            // 收集所有流式事件
            futures_util::pin_mut!(stream);
            while let Some(event_result) = stream.next().await {
                match event_result {
                    Ok(event) => {
                        accumulator.process(event);
                    }
                    Err(e) => {
                        tracing::warn!(%e, "stream error during provider test");
                        return build_provider_test_result(
                            provider_id,
                            model,
                            base_url,
                            started.elapsed(),
                            map_llm_error_to_test_status(&e),
                            format!("Stream error: {}", e),
                            request_json,
                            Some(accumulator.build_response_summary()),
                        );
                    }
                }
            }

            let response = accumulator.into_response();

            // 验证是否返回了工具调用
            let has_tool_calls = !response.tool_calls.is_empty();
            let response_content = if has_tool_calls {
                let tool_calls_json =
                    serde_json::to_string_pretty(&response.tool_calls).unwrap_or_default();
                format!("Tool calls received:\n{}", tool_calls_json)
            } else {
                response
                    .content
                    .unwrap_or_else(|| "No tool calls or content".to_string())
            };

            tracing::debug!(
                has_tool_calls,
                tool_calls_count = response.tool_calls.len(),
                "provider test received response via streaming"
            );

            let status = if has_tool_calls {
                ProviderTestStatus::Success
            } else {
                ProviderTestStatus::InvalidResponse
            };

            let message = if has_tool_calls {
                "Provider tool call test succeeded (streaming).".to_string()
            } else {
                "Provider did not return any tool calls.".to_string()
            };

            build_provider_test_result(
                provider_id,
                model,
                base_url,
                started.elapsed(),
                status,
                message,
                request_json,
                Some(response_content),
            )
        }
        Err(LlmError::UnsupportedCapability { .. }) => {
            // Provider 不支持流式，降级到非流式
            tracing::debug!("Provider doesn't support streaming, falling back to non-streaming");

            match provider.complete_with_tools(request.clone()).await {
                Ok(resp) => {
                    let has_tool_calls = !resp.tool_calls.is_empty();
                    let response_content = if has_tool_calls {
                        let tool_calls_json =
                            serde_json::to_string_pretty(&resp.tool_calls).unwrap_or_default();
                        format!("Tool calls received:\n{}", tool_calls_json)
                    } else {
                        resp.content
                            .unwrap_or_else(|| "No tool calls or content".to_string())
                    };

                    let status = if has_tool_calls {
                        ProviderTestStatus::Success
                    } else {
                        ProviderTestStatus::InvalidResponse
                    };

                    let message = if has_tool_calls {
                        "Provider tool call test succeeded.".to_string()
                    } else {
                        "Provider did not return any tool calls.".to_string()
                    };

                    build_provider_test_result(
                        provider_id,
                        model,
                        base_url,
                        started.elapsed(),
                        status,
                        message,
                        request_json,
                        Some(response_content),
                    )
                }
                Err(error) => {
                    tracing::warn!(%error, "provider test failed");
                    build_provider_test_result(
                        provider_id,
                        model,
                        base_url,
                        started.elapsed(),
                        map_llm_error_to_test_status(&error),
                        error.to_string(),
                        request_json,
                        None,
                    )
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, "provider test failed");
            build_provider_test_result(
                provider_id,
                model,
                base_url,
                started.elapsed(),
                map_llm_error_to_test_status(&error),
                error.to_string(),
                request_json,
                None,
            )
        }
    }
}

/// Accumulates streaming events for provider connection test.
struct TestStreamingAccumulator {
    content: String,
    reasoning_content: String,
    tool_calls: Vec<(Option<String>, Option<String>, String)>,
    input_tokens: u32,
    output_tokens: u32,
    finish_reason: FinishReason,
}

impl TestStreamingAccumulator {
    fn new() -> Self {
        Self {
            content: String::new(),
            reasoning_content: String::new(),
            tool_calls: Vec::new(),
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: FinishReason::Stop,
        }
    }

    fn process(&mut self, event: LlmStreamEvent) {
        match event {
            LlmStreamEvent::ReasoningDelta { delta } => {
                self.reasoning_content.push_str(&delta);
            }
            LlmStreamEvent::ContentDelta { delta } => {
                self.content.push_str(&delta);
            }
            LlmStreamEvent::ToolCallDelta(tc) => {
                // Ensure we have enough slots
                while self.tool_calls.len() <= tc.index {
                    self.tool_calls.push((None, None, String::new()));
                }
                if let Some(id) = tc.id {
                    self.tool_calls[tc.index].0 = Some(id);
                }
                if let Some(name) = tc.name {
                    self.tool_calls[tc.index].1 = Some(name);
                }
                if let Some(args_delta) = tc.arguments_delta {
                    self.tool_calls[tc.index].2.push_str(&args_delta);
                }
            }
            LlmStreamEvent::Usage {
                input_tokens,
                output_tokens,
            } => {
                self.input_tokens = input_tokens;
                self.output_tokens = output_tokens;
            }
            LlmStreamEvent::Finished { finish_reason } => {
                self.finish_reason = finish_reason;
            }
            LlmStreamEvent::RetryAttempt { .. } => {
                // Retry events are informational, skip
            }
        }
    }

    fn into_response(self) -> ToolCompletionResponse {
        let tool_calls: Vec<ToolCall> = self
            .tool_calls
            .into_iter()
            .filter_map(|(id, name, args)| {
                Some(ToolCall {
                    id: id?,
                    name: name?,
                    arguments: serde_json::from_str(&args).unwrap_or(serde_json::Value::Null),
                })
            })
            .collect();

        ToolCompletionResponse {
            content: if self.content.is_empty() {
                None
            } else {
                Some(self.content)
            },
            reasoning_content: if self.reasoning_content.is_empty() {
                None
            } else {
                Some(self.reasoning_content)
            },
            tool_calls,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            finish_reason: self.finish_reason,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }
    }

    fn build_response_summary(&self) -> String {
        let mut parts = Vec::new();

        if !self.content.is_empty() {
            parts.push(format!("Content: {}", self.content));
        }
        if !self.reasoning_content.is_empty() {
            parts.push(format!("Reasoning: {}", self.reasoning_content));
        }
        if !self.tool_calls.is_empty() {
            let tool_info: Vec<String> = self
                .tool_calls
                .iter()
                .map(|(id, name, _)| {
                    format!(
                        "{} ({})",
                        name.as_deref().unwrap_or("?"),
                        id.as_deref().unwrap_or("?")
                    )
                })
                .collect();
            parts.push(format!("Tools: {}", tool_info.join(", ")));
        }

        if parts.is_empty() {
            "No content received".to_string()
        } else {
            parts.join("\n")
        }
    }
}

fn duration_to_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn map_llm_error_to_test_status(error: &LlmError) -> ProviderTestStatus {
    match error {
        LlmError::AuthFailed { .. } => ProviderTestStatus::AuthFailed,
        LlmError::ModelNotAvailable { .. } => ProviderTestStatus::ModelNotAvailable,
        LlmError::RateLimited { .. } => ProviderTestStatus::RateLimited,
        LlmError::InvalidResponse { .. } => ProviderTestStatus::InvalidResponse,
        LlmError::RequestFailed { .. }
        | LlmError::ContextLengthExceeded { .. }
        | LlmError::SessionExpired { .. }
        | LlmError::SessionRenewalFailed { .. }
        | LlmError::UnsupportedCapability { .. } => ProviderTestStatus::RequestFailed,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use argus_auth::AccountManager;
    use argus_auth::credential::CredentialStore;
    use argus_auth::{TokenConfig, TokenContext};
    use argus_crypto::Cipher;
    use argus_crypto::StaticKeySource;
    use argus_protocol::llm::{
        LlmError, LlmProviderId, LlmProviderKind, LlmProviderRecord, LlmProviderRepository,
        ProviderTestStatus, SecretString,
    };
    use sqlx::SqlitePool;

    use super::{duration_to_millis, map_llm_error_to_test_status, ProviderManager};

    // Mock LlmProviderRepository for testing
    struct MockProviderRepository {
        record: LlmProviderRecord,
    }

    #[async_trait]
    impl LlmProviderRepository for MockProviderRepository {
        async fn upsert_provider(
            &self,
            _record: &LlmProviderRecord,
        ) -> Result<LlmProviderId, argus_protocol::ArgusError> {
            todo!()
        }

        async fn delete_provider(
            &self,
            _id: &LlmProviderId,
        ) -> Result<bool, argus_protocol::ArgusError> {
            todo!()
        }

        async fn set_default_provider(&self, _id: &LlmProviderId) -> Result<(), argus_protocol::ArgusError> {
            todo!()
        }

        async fn get_provider(
            &self,
            _id: &LlmProviderId,
        ) -> Result<Option<LlmProviderRecord>, argus_protocol::ArgusError> {
            Ok(Some(self.record.clone()))
        }

        async fn list_providers(&self) -> Result<Vec<LlmProviderRecord>, argus_protocol::ArgusError> {
            Ok(vec![self.record.clone()])
        }

        async fn get_default_provider(&self) -> Result<Option<LlmProviderRecord>, argus_protocol::ArgusError> {
            Ok(Some(self.record.clone()))
        }
    }

    fn make_record(credential_id: Option<i64>) -> LlmProviderRecord {
        LlmProviderRecord {
            id: LlmProviderId::new(1),
            kind: LlmProviderKind::OpenAiCompatible,
            display_name: "Test".to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            api_key: SecretString::new("sk-test"),
            models: vec!["gpt-4".to_string()],
            default_model: "gpt-4".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: argus_protocol::ProviderSecretStatus::Ready,
            credential_id,
        }
    }

    #[tokio::test]
    async fn with_token_context_creates_manager_with_token_support() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let cipher = Arc::new(Cipher::new(StaticKeySource::new(vec![0u8; 32])));
        let account_manager = Arc::new(AccountManager::new(Arc::new(pool.clone()), cipher.clone()));
        let credential_store = Arc::new(CredentialStore::new(Arc::new(pool), cipher));

        let token_context = Arc::new(TokenContext {
            account_manager,
            credential_store,
            config: TokenConfig::new(
                "https://auth.example.com/token".to_string(),
                "Authorization".to_string(),
                "Bearer ".to_string(),
            ),
        });

        let repo = Arc::new(MockProviderRepository {
            record: make_record(None),
        });
        let manager = ProviderManager::with_token_context(repo, token_context);
        assert!(manager.token_context.is_some());
    }

    #[tokio::test]
    async fn build_provider_with_model_static_key_no_token_context() {
        let repo = Arc::new(MockProviderRepository {
            record: make_record(None),
        });
        let manager = ProviderManager::new(repo);

        let result = manager.build_provider_with_model(make_record(None), "gpt-4").await;
        assert!(result.is_ok(), "static API key should build without token context");
    }

    #[tokio::test]
    async fn build_provider_with_model_with_credential_falls_back_when_not_found() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let cipher = Arc::new(Cipher::new(StaticKeySource::new(vec![0u8; 32])));
        let account_manager = Arc::new(AccountManager::new(Arc::new(pool.clone()), cipher.clone()));
        let credential_store = Arc::new(CredentialStore::new(Arc::new(pool), cipher));

        let token_context = Arc::new(TokenContext {
            account_manager,
            credential_store,
            config: TokenConfig::new(
                "https://auth.example.com/token".to_string(),
                "Authorization".to_string(),
                "Bearer ".to_string(),
            ),
        });

        let repo = Arc::new(MockProviderRepository {
            record: make_record(Some(999)), // non-existent credential
        });
        let manager = ProviderManager::with_token_context(repo, token_context);

        // Credential 999 doesn't exist, so it should fall back to base provider
        let result = manager
            .build_provider_with_model(make_record(Some(999)), "gpt-4")
            .await;
        assert!(
            result.is_ok(),
            "should fall back to base provider when credential not found"
        );
    }

    #[test]
    fn duration_to_millis_saturates_at_u64_max() {
        let duration = Duration::from_millis(u64::MAX).saturating_add(Duration::from_millis(1));

        assert_eq!(duration_to_millis(duration), u64::MAX);
    }

    #[test]
    fn provider_test_status_maps_from_llm_errors() {
        assert_eq!(
            map_llm_error_to_test_status(&LlmError::AuthFailed {
                provider: "openai-compatible".to_string(),
                reason: "auth error".to_string(),
            }),
            ProviderTestStatus::AuthFailed
        );
        assert_eq!(
            map_llm_error_to_test_status(&LlmError::ModelNotAvailable {
                provider: "openai-compatible".to_string(),
                model: "gpt-4.1".to_string(),
            }),
            ProviderTestStatus::ModelNotAvailable
        );
        assert_eq!(
            map_llm_error_to_test_status(&LlmError::RateLimited {
                provider: "openai-compatible".to_string(),
                retry_after: None,
            }),
            ProviderTestStatus::RateLimited
        );
        assert_eq!(
            map_llm_error_to_test_status(&LlmError::InvalidResponse {
                provider: "openai-compatible".to_string(),
                reason: "bad payload".to_string(),
            }),
            ProviderTestStatus::InvalidResponse
        );
        assert_eq!(
            map_llm_error_to_test_status(&LlmError::RequestFailed {
                provider: "openai-compatible".to_string(),
                reason: "boom".to_string(),
            }),
            ProviderTestStatus::RequestFailed
        );
    }
}
