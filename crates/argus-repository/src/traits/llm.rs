//! LLM provider repository trait.

use async_trait::async_trait;

use argus_protocol::llm::{
    LlmProviderId, LlmProviderRecord, LlmProviderSummary,
};
use crate::error::DbError;

/// Repository for LLM provider persistence.
#[async_trait]
pub trait LlmProviderRepository: Send + Sync {
    /// Upserts a provider record. Returns the provider ID.
    async fn upsert_provider(&self, record: &LlmProviderRecord) -> Result<LlmProviderId, DbError>;

    /// Deletes a provider by ID.
    async fn delete_provider(&self, id: &LlmProviderId) -> Result<bool, DbError>;

    /// Sets the default provider.
    async fn set_default_provider(&self, id: &LlmProviderId) -> Result<(), DbError>;

    /// Gets a provider by ID.
    async fn get_provider(&self, id: &LlmProviderId) -> Result<Option<LlmProviderRecord>, DbError>;

    /// Gets a provider summary by ID.
    async fn get_provider_summary(
        &self,
        id: &LlmProviderId,
    ) -> Result<Option<LlmProviderSummary>, DbError>;

    /// Lists all providers.
    async fn list_providers(&self) -> Result<Vec<LlmProviderSummary>, DbError>;

    /// Gets the default provider.
    async fn get_default_provider(&self) -> Result<Option<LlmProviderRecord>, DbError>;
}
