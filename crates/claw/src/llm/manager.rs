use std::sync::Arc;

use crate::db::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderRecord, LlmProviderRepository, LlmProviderSummary,
};
use crate::error::AgentError;
use crate::llm::LlmProvider;
#[cfg(feature = "dev")]
use crate::llm::{ChatMessage, CompletionRequest, LlmEventStream};

pub struct LLMManager {
    repository: Arc<dyn LlmProviderRepository>,
}

impl LLMManager {
    #[must_use]
    pub fn new(repository: Arc<dyn LlmProviderRepository>) -> Self {
        Self { repository }
    }

    pub async fn list_providers(&self) -> Result<Vec<LlmProviderSummary>, AgentError> {
        let providers = self.repository.list_providers().await?;
        Ok(providers.into_iter().map(Into::into).collect())
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

        self.build_provider(record)
    }

    pub async fn get_default_provider(&self) -> Result<Arc<dyn LlmProvider>, AgentError> {
        let record = self
            .repository
            .get_default_provider()
            .await?
            .ok_or(AgentError::DefaultProviderNotConfigured)?;

        self.build_provider(record)
    }

    #[cfg(feature = "dev")]
    pub async fn upsert_provider(&self, record: LlmProviderRecord) -> Result<(), AgentError> {
        self.repository.upsert_provider(&record).await?;
        Ok(())
    }

    #[cfg(feature = "dev")]
    pub async fn import_providers(
        &self,
        records: Vec<LlmProviderRecord>,
    ) -> Result<(), AgentError> {
        for record in records {
            self.upsert_provider(record).await?;
        }

        Ok(())
    }

    #[cfg(feature = "dev")]
    pub async fn get_provider_record(
        &self,
        id: &LlmProviderId,
    ) -> Result<LlmProviderRecord, AgentError> {
        self.repository
            .get_provider(id)
            .await?
            .ok_or_else(|| AgentError::ProviderNotFound { id: id.to_string() })
    }

    #[cfg(feature = "dev")]
    pub async fn get_default_provider_record(&self) -> Result<LlmProviderRecord, AgentError> {
        self.repository
            .get_default_provider()
            .await?
            .ok_or(AgentError::DefaultProviderNotConfigured)
    }

    #[cfg(feature = "dev")]
    pub async fn set_default_provider(&self, id: &LlmProviderId) -> Result<(), AgentError> {
        self.repository.set_default_provider(id).await?;
        Ok(())
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

    fn build_provider(
        &self,
        record: LlmProviderRecord,
    ) -> Result<Arc<dyn LlmProvider>, AgentError> {
        match record.kind {
            LlmProviderKind::OpenAiCompatible => {
                #[cfg(feature = "openai-compatible")]
                {
                    let mut config = crate::llm::providers::OpenAiCompatibleConfig::new(
                        record.base_url,
                        record.api_key.expose_secret().to_string(),
                        record.model,
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
