#[cfg(feature = "openai-compatible")]
pub mod openai_compatible;

#[cfg(feature = "openai-compatible")]
pub use openai_compatible::{OpenAiCompatibleConfig, OpenAiCompatibleProvider};
