pub mod error;
pub mod manager;
pub mod provider;
#[cfg(feature = "openai-compatible")]
pub mod providers;
pub mod retry;
pub mod secret;

pub use error::LlmError;
pub use manager::LLMManager;
pub use provider::{
    ChatMessage, CompletionRequest, CompletionResponse, ContentPart, FinishReason, ImageUrl,
    LlmEventStream, LlmProvider, LlmStreamEvent, ModelMetadata, ProviderCapabilities, Role,
    ThinkingConfig, ThinkingMode, ToolCall, ToolCallDelta, ToolCompletionRequest,
    ToolCompletionResponse, ToolDefinition, ToolResult,
    sanitize_tool_messages,
};
pub use retry::{RetryConfig, RetryProvider};

#[cfg(test)]
mod tests {
    #[cfg(feature = "openai-compatible")]
    #[test]
    fn openai_compatible_provider_factory_is_exposed_from_llm_module() {
        let config = crate::llm::providers::OpenAiCompatibleConfig::new(
            "https://api.example.com/v1",
            "sk-test",
            "gpt-4o-mini",
        );
        let factory_config = crate::llm::providers::OpenAiCompatibleFactoryConfig::new(config);
        let provider = crate::llm::providers::create_openai_compatible_provider(factory_config)
            .expect("provider should build");

        assert_eq!(provider.model_name(), "gpt-4o-mini");
    }
}
