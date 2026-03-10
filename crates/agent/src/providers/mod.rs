#[cfg(feature = "openai-compatible")]
pub mod openai_compatible;

#[cfg(feature = "openai-compatible")]
pub use openai_compatible::{
    OpenAiCompatibleConfig, OpenAiCompatibleFactoryConfig, OpenAiCompatibleProvider,
    create_openai_compatible_provider,
};
