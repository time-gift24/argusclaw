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
