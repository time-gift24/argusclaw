//! ProviderResolver - trait for resolving LLM providers.
//!
//! This trait abstracts provider resolution to avoid circular dependencies
//! between argus-session and claw.

use std::sync::Arc;

use argus_protocol::Result;
use argus_protocol::{LlmProvider, ProviderId};
use async_trait::async_trait;

/// Trait for resolving LLM providers by ID.
///
/// This trait is implemented by the application layer (claw::LLMManager)
/// to provide provider instances to the session layer.
#[async_trait]
pub trait ProviderResolver: Send + Sync {
    /// Resolve a provider by its ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider is not found or cannot be instantiated.
    async fn resolve(&self, id: ProviderId) -> Result<Arc<dyn LlmProvider>>;

    /// Get the default provider.
    ///
    /// # Errors
    ///
    /// Returns an error if no default provider is configured.
    async fn default_provider(&self) -> Result<Arc<dyn LlmProvider>>;
}
