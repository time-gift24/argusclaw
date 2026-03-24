//! ProviderResolver - re-exported from argus-protocol.
//!
//! This file re-exports the ProviderResolver trait from argus-protocol
//! to maintain backward compatibility with existing imports.
//! Implementation lives in argus-wing.

pub use argus_protocol::ProviderResolver;

// Re-export the concrete types needed by the trait
pub use argus_protocol::{LlmProvider, ProviderId};
