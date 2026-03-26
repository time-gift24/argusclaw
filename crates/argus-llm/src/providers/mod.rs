pub mod openai_compatible;

pub(crate) use openai_compatible::DEFAULT_OPENAI_COMPATIBLE_TIMEOUT;
pub use openai_compatible::{
    OpenAiCompatibleConfig, OpenAiCompatibleFactoryConfig, create_openai_compatible_provider,
};
