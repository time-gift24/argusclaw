#[cfg(feature = "openai-compatible")]
use agent::llm::providers::{
    OpenAiCompatibleConfig, OpenAiCompatibleFactoryConfig, create_openai_compatible_provider,
};

#[cfg(feature = "openai-compatible")]
#[test]
fn openai_compatible_provider_factory_is_exposed_from_llm_module() {
    let config =
        OpenAiCompatibleConfig::new("https://api.example.com/v1", "sk-test", "gpt-4o-mini");
    let factory_config = OpenAiCompatibleFactoryConfig::new(config);
    let provider =
        create_openai_compatible_provider(factory_config).expect("provider should build");

    assert_eq!(provider.model_name(), "gpt-4o-mini");
}
