//! Repository trait for LLM provider storage.
//!
//! This trait defines the interface for persisting and retrieving LLM provider
//! configurations. Implementations can use different storage backends (SQLite,
//! PostgreSQL, in-memory, etc.).

use async_trait::async_trait;

use super::provider_types::{LlmProviderId, LlmProviderRecord};
use crate::Result;

/// Repository trait for LLM provider CRUD operations.
///
/// This trait is implemented by storage layers (e.g., SQLite in claw) and
/// consumed by `ProviderManager` in argus-llm.
#[async_trait]
pub trait LlmProviderRepository: Send + Sync {
    /// Upsert a provider record.
    ///
    /// Returns the provider ID (newly generated for inserts, or the existing
    /// ID for updates).
    async fn upsert_provider(&self, record: &LlmProviderRecord) -> Result<LlmProviderId>;

    /// Delete a provider by ID.
    ///
    /// Returns `true` if the provider was deleted, `false` if it didn't exist.
    async fn delete_provider(&self, id: &LlmProviderId) -> Result<bool>;

    /// Set the default provider.
    ///
    /// This unsets any previously default provider and marks the specified
    /// provider as the default.
    async fn set_default_provider(&self, id: &LlmProviderId) -> Result<()>;

    /// Get a provider record by ID (including sensitive data).
    async fn get_provider(&self, id: &LlmProviderId) -> Result<Option<LlmProviderRecord>>;

    /// List all provider records.
    async fn list_providers(&self) -> Result<Vec<LlmProviderRecord>>;

    /// Get the default provider record.
    async fn get_default_provider(&self) -> Result<Option<LlmProviderRecord>>;
}
