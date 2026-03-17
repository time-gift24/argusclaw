pub mod manager;
pub mod secret;

pub use argus_llm::RetryConfig;
pub use argus_llm::providers::{
    OpenAiCompatibleConfig, OpenAiCompatibleFactoryConfig, create_openai_compatible_provider,
};
pub use manager::LLMManager;

// Re-export types from argus-protocol
pub use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, ContentPart, FinishReason, ImageUrl,
    LlmError, LlmEventStream, LlmProvider, LlmStreamEvent, ModelMetadata, ProviderCapabilities,
    Role, sanitize_tool_messages, ThinkingConfig, ThinkingMode, ToolCall, ToolCallDelta,
    ToolCompletionRequest, ToolCompletionResponse, ToolDefinition, ToolResult,
};

#[cfg(test)]
mod tests {
    #[test]
    fn openai_compatible_provider_factory_is_exposed_from_llm_module() {
        let config = argus_llm::OpenAiCompatibleConfig::new(
            "https://api.example.com/v1",
            "sk-test",
            "gpt-4o-mini",
        );
        let factory_config = argus_llm::OpenAiCompatibleFactoryConfig::new(config);
        let provider = argus_llm::create_openai_compatible_provider(factory_config)
            .expect("provider should build");

        assert_eq!(provider.model_name(), "gpt-4o-mini");
    }
}
