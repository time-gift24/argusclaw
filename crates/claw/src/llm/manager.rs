use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;

use crate::db::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderRecord, LlmProviderRepository, LlmProviderSummary,
    ProviderSecretStatus, ProviderTestResult, ProviderTestStatus,
};
use crate::error::AgentError;
use crate::llm::ChatMessage;
use crate::llm::LlmError;
use crate::llm::LlmProvider;
use crate::llm::provider::CompletionRequest;
#[cfg(feature = "dev")]
use crate::llm::provider::LlmEventStream;

pub struct LLMManager {
    repository: Arc<dyn LlmProviderRepository>,
}

impl LLMManager {
    #[must_use]
    pub fn new(repository: Arc<dyn LlmProviderRepository>) -> Self {
        Self { repository }
    }

    pub async fn list_providers(&self) -> Result<Vec<LlmProviderSummary>, AgentError> {
        self.repository.list_providers().await.map_err(Into::into)
    }

    pub async fn get_provider(
        &self,
        id: &LlmProviderId,
    ) -> Result<Arc<dyn LlmProvider>, AgentError> {
        let record = self
            .repository
            .get_provider(id)
            .await?
            .ok_or_else(|| AgentError::ProviderNotFound { id: id.to_string() })?;

        let default_model = record.default_model.clone();
        self.build_provider_with_model(record, &default_model)
    }

    pub async fn get_provider_with_model(
        &self,
        id: &LlmProviderId,
        model: &str,
    ) -> Result<Arc<dyn LlmProvider>, AgentError> {
        let record = self
            .repository
            .get_provider(id)
            .await?
            .ok_or_else(|| AgentError::ProviderNotFound { id: id.to_string() })?;

        if !record.models.contains(&model.to_string()) {
            return Err(AgentError::ModelNotAvailable {
                provider: id.to_string(),
                model: model.to_string(),
            });
        }

        self.build_provider_with_model(record, model)
    }

    pub async fn get_default_provider(&self) -> Result<Arc<dyn LlmProvider>, AgentError> {
        let record = self
            .repository
            .get_default_provider()
            .await?
            .ok_or(AgentError::DefaultProviderNotConfigured)?;

        let default_model = record.default_model.clone();
        self.build_provider_with_model(record, &default_model)
    }

    pub async fn upsert_provider(&self, record: LlmProviderRecord) -> Result<(), AgentError> {
        let record = LlmProviderRecord {
            secret_status: ProviderSecretStatus::Ready,
            ..record
        };
        self.repository.upsert_provider(&record).await?;
        Ok(())
    }

    pub async fn delete_provider(&self, id: &LlmProviderId) -> Result<bool, AgentError> {
        self.repository
            .delete_provider(id)
            .await
            .map_err(Into::into)
    }

    pub async fn import_providers(
        &self,
        records: Vec<LlmProviderRecord>,
    ) -> Result<(), AgentError> {
        for record in records {
            self.upsert_provider(record).await?;
        }

        Ok(())
    }

    pub async fn get_provider_record(
        &self,
        id: &LlmProviderId,
    ) -> Result<LlmProviderRecord, AgentError> {
        self.repository
            .get_provider(id)
            .await?
            .ok_or_else(|| AgentError::ProviderNotFound { id: id.to_string() })
    }

    pub async fn get_provider_summary(
        &self,
        id: &LlmProviderId,
    ) -> Result<LlmProviderSummary, AgentError> {
        self.repository
            .get_provider_summary(id)
            .await?
            .ok_or_else(|| AgentError::ProviderNotFound { id: id.to_string() })
    }

    pub async fn get_default_provider_record(&self) -> Result<LlmProviderRecord, AgentError> {
        self.repository
            .get_default_provider()
            .await?
            .ok_or(AgentError::DefaultProviderNotConfigured)
    }

    pub async fn set_default_provider(&self, id: &LlmProviderId) -> Result<(), AgentError> {
        self.repository.set_default_provider(id).await?;
        Ok(())
    }

    pub async fn test_provider_connection(
        &self,
        id: &LlmProviderId,
        model: &str,
    ) -> Result<ProviderTestResult, AgentError> {
        let Some(record) = self.repository.get_provider(id).await? else {
            return Ok(build_provider_test_result(
                id.to_string(),
                String::new(),
                String::new(),
                Duration::ZERO,
                ProviderTestStatus::ProviderNotFound,
                AgentError::ProviderNotFound { id: id.to_string() }.to_string(),
            ));
        };

        let provider_id = record.id.to_string();
        let base_url = record.base_url.clone();
        let provider = match self.build_provider_with_model(record.clone(), model) {
            Ok(provider) => provider,
            Err(AgentError::UnsupportedProviderKind { kind }) => {
                return Ok(build_provider_test_result(
                    provider_id,
                    model.to_string(),
                    base_url,
                    Duration::ZERO,
                    ProviderTestStatus::UnsupportedProviderKind,
                    AgentError::UnsupportedProviderKind { kind }.to_string(),
                ));
            }
            Err(AgentError::ModelNotAvailable { provider, model }) => {
                return Ok(build_provider_test_result(
                    provider.clone(),
                    model.clone(),
                    base_url,
                    Duration::ZERO,
                    ProviderTestStatus::ModelNotAvailable,
                    AgentError::ModelNotAvailable { provider, model }.to_string(),
                ));
            }
            Err(error) => return Err(error),
        };

        Ok(run_provider_connection_test(provider_id, model.to_string(), base_url, provider).await)
    }

    pub async fn test_provider_record(
        &self,
        record: LlmProviderRecord,
        model: &str,
    ) -> Result<ProviderTestResult, AgentError> {
        let provider_id = record.id.to_string();
        let base_url = record.base_url.clone();
        let provider = match self.build_provider_with_model(record, model) {
            Ok(provider) => provider,
            Err(AgentError::UnsupportedProviderKind { kind }) => {
                return Ok(build_provider_test_result(
                    provider_id,
                    model.to_string(),
                    base_url,
                    Duration::ZERO,
                    ProviderTestStatus::UnsupportedProviderKind,
                    AgentError::UnsupportedProviderKind { kind }.to_string(),
                ));
            }
            Err(error) => return Err(error),
        };

        Ok(run_provider_connection_test(provider_id, model.to_string(), base_url, provider).await)
    }

    #[cfg(feature = "dev")]
    pub async fn complete_text(
        &self,
        provider_id: Option<&LlmProviderId>,
        prompt: impl Into<String>,
    ) -> Result<String, AgentError> {
        let provider = match provider_id {
            Some(id) => self.get_provider(id).await?,
            None => self.get_default_provider().await?,
        };
        let request = CompletionRequest::new(vec![ChatMessage::user(prompt.into())]);
        let response = provider.complete(request).await?;

        Ok(response.content)
    }

    #[cfg(feature = "dev")]
    pub async fn stream_text(
        &self,
        provider_id: Option<&LlmProviderId>,
        prompt: impl Into<String>,
    ) -> Result<LlmEventStream, AgentError> {
        let provider = match provider_id {
            Some(id) => self.get_provider(id).await?,
            None => self.get_default_provider().await?,
        };
        let request = CompletionRequest::new(vec![ChatMessage::user(prompt.into())]);

        provider
            .stream_complete(request)
            .await
            .map_err(AgentError::from)
    }

    fn build_provider_with_model(
        &self,
        record: LlmProviderRecord,
        model: &str,
    ) -> Result<Arc<dyn LlmProvider>, AgentError> {
        match record.kind {
            LlmProviderKind::OpenAiCompatible => {
                #[cfg(feature = "openai-compatible")]
                {
                    let mut config = crate::llm::providers::OpenAiCompatibleConfig::new(
                        record.base_url,
                        record.api_key.expose_secret().to_string(),
                        model.to_string(),
                    );

                    for (name, value) in &record.extra_headers {
                        config = config.with_extra_header(name, value);
                    }

                    let factory_config =
                        crate::llm::providers::OpenAiCompatibleFactoryConfig::new(config);

                    crate::llm::providers::create_openai_compatible_provider(factory_config)
                        .map_err(AgentError::from)
                }

                #[cfg(not(feature = "openai-compatible"))]
                {
                    Err(AgentError::UnsupportedProviderKind {
                        kind: record.kind.to_string(),
                    })
                }
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

    use crate::db::llm::ProviderTestStatus;
    use crate::llm::LlmError;

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
