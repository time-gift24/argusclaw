use std::sync::Arc;

use crate::db::llm::{LlmProviderId, LlmProviderKind, LlmProviderRepository, LlmProviderSummary};
use crate::error::AgentError;
use crate::llm::LlmProvider;

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

    fn build_provider(
        &self,
        record: crate::db::llm::LlmProviderRecord,
    ) -> Result<Arc<dyn LlmProvider>, AgentError> {
        match record.kind {
            LlmProviderKind::OpenAiCompatible => {
                #[cfg(feature = "openai-compatible")]
                {
                    let config = crate::llm::providers::OpenAiCompatibleConfig::new(
                        record.base_url,
                        record.api_key.expose_secret().to_string(),
                        record.model,
                    );
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
