//! Argus LLM crate - LLM provider implementations.
//!
//! This crate provides LLM provider implementations based on the `LlmProvider` trait
//! defined in argus-protocol.

pub mod providers;
pub mod retry;
pub mod secret;
pub mod manager;

pub use providers::{
    OpenAiCompatibleConfig, OpenAiCompatibleFactoryConfig, create_openai_compatible_provider,
};
pub use retry::{RetryConfig, RetryProvider};
pub use secret::{
    ApiKeyCipher, EncryptedSecret, FileKeyMaterialSource, KeyMaterialSource,
    SecretError, StaticKeyMaterialSource,
};
pub use manager::ProviderManager;
