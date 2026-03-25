//! Argus LLM crate - LLM provider implementations.
//!
//! This crate provides LLM provider implementations based on the `LlmProvider` trait
//! defined in argus-protocol.

pub mod manager;
pub mod providers;
pub mod retry;
pub mod test_utils;

pub use manager::ProviderManager;
pub use providers::{
    OpenAiCompatibleConfig, OpenAiCompatibleFactoryConfig, create_openai_compatible_provider,
};
pub use retry::{RetryConfig, RetryProvider};
pub use test_utils::{TestRetryProvider, create_test_retry_provider};

// Re-export crypto types for convenience
pub use argus_crypto::{
    Cipher, CryptoError, EncryptedSecret, FileKeySource, KeyMaterialSource, StaticKeySource,
};
