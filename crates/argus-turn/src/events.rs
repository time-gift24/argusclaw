//! Turn log events - incremental JSONL event types.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use argus_protocol::llm::ChatMessage;
use argus_protocol::token_usage::TokenUsage;

/// Single event in a turn's JSONL log.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "snake_case")]
pub enum TurnLogEvent {
    TurnStart { system_prompt: String, model: String },
    UserInput { content: String, role: String },
    LlmRequest { messages: Vec<ChatMessage>, tools: Vec<Value> },
    LlmDelta { delta: String, is_complete: bool },
    ToolCallStart { id: String, name: String, arguments: Value },
    ToolCallDelta { id: String, delta: Value },
    ToolResult {
        id: String,
        name: String,
        result: String,
        duration_ms: u64,
        error: Option<String>,
    },
    LlmResponse {
        content: String,
        reasoning_content: Option<String>,
        tool_calls: Vec<Value>,
        finish_reason: String,
    },
    TurnEnd { token_usage: TokenUsage, finish_reason: String },
    TurnError { error: String, at_iteration: Option<u32> },
}

impl TurnLogEvent {
    /// Return the snake_case type name for JSONL serialization.
    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            TurnLogEvent::TurnStart { .. } => "turn_start",
            TurnLogEvent::UserInput { .. } => "user_input",
            TurnLogEvent::LlmRequest { .. } => "llm_req",
            TurnLogEvent::LlmDelta { .. } => "llm_delta",
            TurnLogEvent::ToolCallStart { .. } => "tool_call_start",
            TurnLogEvent::ToolCallDelta { .. } => "tool_call_delta",
            TurnLogEvent::ToolResult { .. } => "tool_result",
            TurnLogEvent::LlmResponse { .. } => "llm_response",
            TurnLogEvent::TurnEnd { .. } => "turn_end",
            TurnLogEvent::TurnError { .. } => "turn_error",
        }
    }
}
