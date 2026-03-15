//! Thread event types broadcast to subscribers.

use super::{ApprovalRequest, ApprovalResponse, ThreadId, TokenUsage};
use crate::llm::LlmStreamEvent;

/// Thread event broadcast to subscribers (CLI, Tauri).
#[derive(Debug, Clone)]
pub enum ThreadEvent {
    /// Turn is processing, streaming LLM/tool events.
    Processing {
        thread_id: ThreadId,
        turn_number: u32,
        event: LlmStreamEvent,
    },
    /// Tool execution started.
    ToolStarted {
        thread_id: ThreadId,
        turn_number: u32,
        tool_call_id: String,
        tool_name: String,
        arguments: serde_json::Value,
    },
    /// Tool execution completed.
    ToolCompleted {
        thread_id: ThreadId,
        turn_number: u32,
        tool_call_id: String,
        tool_name: String,
        result: Result<serde_json::Value, String>,
    },
    /// Turn completed successfully.
    TurnCompleted {
        thread_id: ThreadId,
        turn_number: u32,
        token_usage: TokenUsage,
    },
    /// Turn failed.
    TurnFailed {
        thread_id: ThreadId,
        turn_number: u32,
        error: String,
    },
    /// Thread entered idle state.
    Idle { thread_id: ThreadId },
    /// Context was compacted.
    Compacted {
        thread_id: ThreadId,
        new_token_count: u32,
    },
    /// Waiting for approval - tool execution paused for human confirmation.
    WaitingForApproval {
        thread_id: ThreadId,
        turn_number: u32,
        request: ApprovalRequest,
    },
    /// Approval resolved (approved/denied/timeout).
    ApprovalResolved {
        thread_id: ThreadId,
        turn_number: u32,
        response: ApprovalResponse,
    },
}
