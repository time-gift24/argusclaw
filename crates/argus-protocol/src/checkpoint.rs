//! Checkpoint and persistence types for thread history and rollback.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::llm::ChatMessage;
use crate::token_usage::TokenUsage;

/// Complete snapshot of a turn's execution, stored in turn_data JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnSnapshot {
    /// Messages added in this turn
    pub messages: Vec<ChatMessage>,

    /// Tool call details
    pub tool_calls: Vec<ToolCallDetail>,

    /// LLM raw response
    pub llm_response: Option<LlmResponseSnapshot>,

    /// Turn execution config (JSON serialized)
    pub config: String,

    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

/// Detailed information about a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallDetail {
    /// Tool call ID
    pub id: String,

    /// Tool name
    pub name: String,

    /// Tool arguments (JSON string)
    pub arguments: String,

    /// Tool result
    pub result: ToolResultSnapshot,

    /// Duration in milliseconds
    pub duration_ms: u64,
}

/// Snapshot of tool execution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolResultSnapshot {
    /// Successful execution with output
    Success { output: String },
    /// Tool execution failed
    Error { error: String },
    /// Tool execution timed out
    Timeout,
}

/// Snapshot of LLM response for debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponseSnapshot {
    /// Model name
    pub model: String,

    /// Raw response text
    pub raw_response: String,

    /// Finish reason
    pub finish_reason: Option<String>,
}

/// Summary of a checkpoint for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointSummary {
    /// Turn sequence number
    pub turn_seq: u32,

    /// Model used
    pub model: String,

    /// Input tokens
    pub input_tokens: u32,

    /// Output tokens
    pub output_tokens: u32,

    /// Latency in milliseconds
    pub latency_ms: u64,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Number of messages in this turn
    pub message_count: u32,
}

/// Detailed checkpoint information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointDetail {
    /// Turn sequence number
    pub turn_seq: u32,

    /// Model used
    pub model: String,

    /// Token usage
    pub token_usage: TokenUsage,

    /// Latency in milliseconds
    pub latency_ms: u64,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Messages in this turn
    pub messages: Vec<ChatMessage>,

    /// Tool calls (as JSON string for compatibility)
    pub tool_calls: String,

    /// LLM response (if available)
    pub llm_response: Option<String>,
}

/// Comparison between two checkpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointComparison {
    /// Turn A details
    pub turn_a: CheckpointDetail,

    /// Turn B details
    pub turn_b: CheckpointDetail,

    /// Token difference
    pub token_diff: TokenDiff,

    /// Message difference
    pub message_diff: MessageDiff,
}

/// Token difference between two checkpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenDiff {
    /// Input token delta (B - A)
    pub input_delta: i32,

    /// Output token delta (B - A)
    pub output_delta: i32,

    /// Total token delta (B - A)
    pub total_delta: i32,
}

/// Message difference between two checkpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDiff {
    /// Number of messages in turn A
    pub count_a: usize,

    /// Number of messages in turn B
    pub count_b: usize,

    /// Message count delta (B - A)
    pub count_delta: isize,
}

/// Thread state summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadState {
    /// Thread ID
    pub thread_id: crate::ThreadId,

    /// Thread title
    pub title: Option<String>,

    /// Number of messages
    pub message_count: usize,

    /// Number of turns
    pub turn_count: u32,

    /// Total token count
    pub token_count: u32,

    /// Last turn sequence number
    pub last_turn_seq: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{ChatMessage, Role};

    #[test]
    fn test_tool_result_snapshot_success() {
        let result = ToolResultSnapshot::Success {
            output: "Test output".to_string(),
        };
        assert!(matches!(result, ToolResultSnapshot::Success { .. }));
    }

    #[test]
    fn test_tool_result_snapshot_error() {
        let result = ToolResultSnapshot::Error {
            error: "Test error".to_string(),
        };
        assert!(matches!(result, ToolResultSnapshot::Error { .. }));
    }

    #[test]
    fn test_tool_result_snapshot_timeout() {
        let result = ToolResultSnapshot::Timeout;
        assert!(matches!(result, ToolResultSnapshot::Timeout));
    }

    #[test]
    fn test_llm_response_snapshot_serialization() {
        let snapshot = LlmResponseSnapshot {
            model: "gpt-4".to_string(),
            raw_response: "Test response".to_string(),
            finish_reason: Some("stop".to_string()),
        };

        let serialized = serde_json::to_string(&snapshot).unwrap();
        let deserialized: LlmResponseSnapshot = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.model, "gpt-4");
        assert_eq!(deserialized.raw_response, "Test response");
        assert_eq!(deserialized.finish_reason, Some("stop".to_string()));
    }

    #[test]
    fn test_token_diff_calculation() {
        let diff = TokenDiff {
            input_delta: 100,
            output_delta: 50,
            total_delta: 150,
        };

        assert_eq!(diff.input_delta, 100);
        assert_eq!(diff.output_delta, 50);
        assert_eq!(diff.total_delta, 150);
    }

    #[test]
    fn test_message_diff_calculation() {
        let diff = MessageDiff {
            count_a: 10,
            count_b: 15,
            count_delta: 5,
        };

        assert_eq!(diff.count_a, 10);
        assert_eq!(diff.count_b, 15);
        assert_eq!(diff.count_delta, 5);
    }

    #[test]
    fn test_thread_state_creation() {
        let state = ThreadState {
            thread_id: crate::ThreadId::new(),
            title: Some("Test Thread".to_string()),
            message_count: 100,
            turn_count: 10,
            token_count: 5000,
            last_turn_seq: Some(10),
        };

        assert_eq!(state.title, Some("Test Thread".to_string()));
        assert_eq!(state.message_count, 100);
        assert_eq!(state.turn_count, 10);
        assert_eq!(state.token_count, 5000);
        assert_eq!(state.last_turn_seq, Some(10));
    }

    #[test]
    fn test_checkpoint_summary_serialization() {
        let summary = CheckpointSummary {
            turn_seq: 5,
            model: "gpt-4".to_string(),
            input_tokens: 100,
            output_tokens: 50,
            latency_ms: 1000,
            created_at: Utc::now(),
            message_count: 3,
        };

        let serialized = serde_json::to_string(&summary).unwrap();
        let deserialized: CheckpointSummary = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.turn_seq, 5);
        assert_eq!(deserialized.model, "gpt-4");
        assert_eq!(deserialized.input_tokens, 100);
        assert_eq!(deserialized.output_tokens, 50);
    }
}

