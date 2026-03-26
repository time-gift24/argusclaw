//! ProviderResolver trait - abstracts LLM provider resolution.
//!
//! This trait lives in argus-protocol to avoid circular dependencies
//! between argus-job, argus-session, and argus-wing.

use std::sync::Arc;

use crate::{LlmProvider, ProviderId, Result};
use async_trait::async_trait;

/// Trait for resolving LLM providers by ID.
///
/// Implemented by the application layer (argus-wing) to provide
/// provider instances to session and job layers.
#[async_trait]
pub trait ProviderResolver: Send + Sync {
    /// Resolve a provider by its ID.
    async fn resolve(&self, id: ProviderId) -> Result<Arc<dyn LlmProvider>>;

    /// Get the default provider.
    async fn default_provider(&self) -> Result<Arc<dyn LlmProvider>>;

    /// Resolve a provider by its ID and use a specific model.
    async fn resolve_with_model(&self, id: ProviderId, model: &str)
    -> Result<Arc<dyn LlmProvider>>;
}
