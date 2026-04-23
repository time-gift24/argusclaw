use std::sync::Arc;

use argus_llm::ProviderManager;
use argus_protocol::{LlmProvider, LlmProviderId, ProviderId, ProviderResolver, Result};

pub(crate) struct ProviderManagerResolver {
    provider_manager: Arc<ProviderManager>,
}

impl ProviderManagerResolver {
    pub(crate) fn new(provider_manager: Arc<ProviderManager>) -> Self {
        Self { provider_manager }
    }
}

#[async_trait::async_trait]
impl ProviderResolver for ProviderManagerResolver {
    async fn resolve(&self, id: ProviderId) -> Result<Arc<dyn LlmProvider>> {
        let provider_id = LlmProviderId::new(id.inner());
        self.provider_manager.get_provider(&provider_id).await
    }

    async fn default_provider(&self) -> Result<Arc<dyn LlmProvider>> {
        self.provider_manager.get_default_provider().await
    }

    async fn resolve_with_model(
        &self,
        id: ProviderId,
        model: &str,
    ) -> Result<Arc<dyn LlmProvider>> {
        let provider_id = LlmProviderId::new(id.inner());
        self.provider_manager
            .get_provider_with_model(&provider_id, model)
            .await
    }
}
