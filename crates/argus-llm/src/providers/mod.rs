pub mod openai_compatible;

pub use openai_compatible::{
    OpenAiCompatibleConfig, OpenAiCompatibleFactoryConfig, create_openai_compatible_provider,
};
pub(crate) use openai_compatible::DEFAULT_OPENAI_COMPATIBLE_TIMEOUT;
