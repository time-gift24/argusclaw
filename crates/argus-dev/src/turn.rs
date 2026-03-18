use crate::error::{DevError, Result};
use argus_protocol::llm::{ChatMessage, LlmProvider};
use argus_turn::{execute_turn, TurnConfig, TurnInputBuilder};
use std::sync::Arc;

/// Execute a single turn with given parameters.
///
/// This function provides a simplified interface for turn execution
/// in development/testing scenarios.
///
/// # Arguments
///
/// * `provider` - The LLM provider to use for completion
/// * `messages` - The conversation message history
/// * `tool_manager` - Tool manager for tool execution
/// * `tool_ids` - Optional list of specific tool IDs to use
/// * `config` - Turn execution configuration
///
/// # Returns
///
/// Returns the turn output containing updated messages and token usage.
///
/// # Errors
///
/// Returns an error if:
/// - Turn execution fails
/// - Tool execution fails
/// - LLM provider fails
pub async fn execute_turn_with_config(
    provider: Arc<dyn LlmProvider>,
    messages: Vec<ChatMessage>,
    tool_manager: Arc<argus_tool::ToolManager>,
    tool_ids: Option<Vec<String>>,
    config: TurnConfig,
) -> Result<argus_turn::TurnOutput> {
    let mut builder = TurnInputBuilder::new()
        .provider(provider)
        .messages(messages)
        .tool_manager(tool_manager);

    if let Some(ids) = tool_ids {
        builder = builder.tool_ids(ids);
    }

    let input = builder
        .build()
        .map_err(|e| DevError::InvalidConfiguration(e.to_string()))?;
    let output = execute_turn(input, config)
        .await
        .map_err(|e| DevError::TurnFailed {
            reason: e.to_string(),
        })?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_turn_with_config_signature() {
        // This test just verifies the function signature compiles correctly
        // Actual execution tests are in the integration tests
        let _ = execute_turn_with_config;
    }
}
