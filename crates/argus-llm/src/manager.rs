//! Provider manager for LLM provider lookup and instantiation.
//!
//! This module provides `ProviderManager` which handles:
//! - Looking up provider records from a repository
//! - Building LLM provider instances from records
//! - Testing provider connections

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;

use argus_protocol::Result;
use argus_protocol::llm::{
    ChatMessage, CompletionRequest, LlmError, LlmProvider, LlmProviderId, LlmProviderKind,
    LlmProviderRecord, LlmProviderRepository, LlmProviderSummary, ProviderSecretStatus,
    ProviderTestResult, ProviderTestStatus,
};

use crate::providers::{
    OpenAiCompatibleConfig, OpenAiCompatibleFactoryConfig, create_openai_compatible_provider,
};

/// Manager for LLM provider lookup and instantiation.
///
/// This is the main entry point for obtaining LLM provider instances
/// from stored configuration.
pub struct ProviderManager {
    repository: Arc<dyn LlmProviderRepository>,
}

impl ProviderManager {
    /// Create a new provider manager with the given repository.
    #[must_use]
    pub fn new(repository: Arc<dyn LlmProviderRepository>) -> Self {
        Self { repository }
    }

    /// List all provider summaries.
    pub async fn list_providers(&self) -> Result<Vec<LlmProviderSummary>> {
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
        self.build_provider_with_model(record, &default_model)
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

        self.build_provider_with_model(record, model)
    }

    /// Get the default provider instance.
    pub async fn get_default_provider(&self) -> Result<Arc<dyn LlmProvider>> {
        let record = self
            .repository
            .get_default_provider()
            .await?
            .ok_or(argus_protocol::ArgusError::ProviderNotFound(0))?;

        let default_model = record.default_model.clone();
        self.build_provider_with_model(record, &default_model)
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

    /// Get a provider summary by ID.
    pub async fn get_provider_summary(&self, id: &LlmProviderId) -> Result<LlmProviderSummary> {
        self.repository
            .get_provider_summary(id)
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
            ));
        };

        let provider_id = record.id.to_string();
        let base_url = record.base_url.clone();
        let provider = match self.build_provider_with_model(record.clone(), model) {
            Ok(provider) => provider,
            Err(argus_protocol::ArgusError::LlmError { reason }) => {
                return Ok(build_provider_test_result(
                    provider_id,
                    model.to_string(),
                    base_url,
                    Duration::ZERO,
                    ProviderTestStatus::UnsupportedProviderKind,
                    reason,
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
        let provider = match self.build_provider_with_model(record, model) {
            Ok(provider) => provider,
            Err(argus_protocol::ArgusError::LlmError { reason }) => {
                return Ok(build_provider_test_result(
                    provider_id,
                    model.to_string(),
                    base_url,
                    Duration::ZERO,
                    ProviderTestStatus::UnsupportedProviderKind,
                    reason,
                ));
            }
            Err(e) => return Err(e),
        };

        Ok(run_provider_connection_test(provider_id, model.to_string(), base_url, provider).await)
    }

    #[cfg(feature = "dev")]
    pub async fn complete_text(
        &self,
        provider_id: Option<&LlmProviderId>,
        prompt: impl Into<String>,
    ) -> Result<String> {
        let provider = match provider_id {
            Some(id) => self.get_provider(id).await?,
            None => self.get_default_provider().await?,
        };
        let request = CompletionRequest::new(vec![ChatMessage::user(prompt.into())]);
        let response =
            provider
                .complete(request)
                .await
                .map_err(|e| argus_protocol::ArgusError::LlmError {
                    reason: e.to_string(),
                })?;

        Ok(response.content)
    }

    #[cfg(feature = "dev")]
    pub async fn stream_text(
        &self,
        provider_id: Option<&LlmProviderId>,
        prompt: impl Into<String>,
    ) -> Result<argus_protocol::llm::LlmEventStream> {
        let provider = match provider_id {
            Some(id) => self.get_provider(id).await?,
            None => self.get_default_provider().await?,
        };
        let request = CompletionRequest::new(vec![ChatMessage::user(prompt.into())]);

        provider
            .stream_complete(request)
            .await
            .map_err(|e| argus_protocol::ArgusError::LlmError {
                reason: e.to_string(),
            })
    }

    fn build_provider_with_model(
        &self,
        record: LlmProviderRecord,
        model: &str,
    ) -> Result<Arc<dyn LlmProvider>> {
        match record.kind {
            LlmProviderKind::OpenAiCompatible => {
                let mut config = OpenAiCompatibleConfig::new(
                    record.base_url,
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
}

fn build_provider_test_result(
    provider_id: String,
    model: String,
    base_url: String,
    latency: Duration,
    status: ProviderTestStatus,
    message: String,
) -> ProviderTestResult {
    ProviderTestResult {
        provider_id,
        model,
        base_url,
        checked_at: Utc::now(),
        latency_ms: duration_to_millis(latency),
        status,
        message,
    }
}

async fn run_provider_connection_test(
    provider_id: String,
    model: String,
    base_url: String,
    provider: Arc<dyn LlmProvider>,
) -> ProviderTestResult {
    let started = std::time::Instant::now();
    let request = CompletionRequest::new(vec![ChatMessage::user("Reply with exactly OK.")])
        .with_max_tokens(8)
        .with_temperature(0.0);

    match provider.complete(request).await {
        Ok(_) => build_provider_test_result(
            provider_id,
            model,
            base_url,
            started.elapsed(),
            ProviderTestStatus::Success,
            "Provider connection test succeeded.".to_string(),
        ),
        Err(error) => build_provider_test_result(
            provider_id,
            model,
            base_url,
            started.elapsed(),
            map_llm_error_to_test_status(&error),
            error.to_string(),
        ),
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
    use std::time::Duration;

    use argus_protocol::llm::{LlmError, ProviderTestStatus};

    use super::{duration_to_millis, map_llm_error_to_test_status};

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
