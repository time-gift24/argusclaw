//! Protocol types shared across modules.
//!
//! This module re-exports types from argus_protocol and provides claw-specific types.
//! This avoids duplication and ensures type compatibility across crates.

// Re-export from argus_protocol for shared types
pub use argus_protocol::{
    ApprovalDecision, ApprovalRequest, ApprovalResponse, BeforeCallLLMContext,
    BeforeCallLLMResult, HookAction, HookEvent, HookHandler, HookRegistry,
    RiskLevel, ThreadEvent, ThreadId, TokenUsage, ToolHookContext,
};

// Claw-specific types (not in argus_protocol)
mod approval;
mod runtime_agent;
mod thread_snapshot;

pub use approval::{
    ApprovalEvent, MAX_ACTION_LEN, MAX_PENDING_PER_AGENT, MAX_TIMEOUT_SECS,
    MAX_TOOL_NAME_LEN, MIN_TIMEOUT_SECS,
};
pub use runtime_agent::RuntimeAgentHandle;
pub use thread_snapshot::{ThreadMessageSnapshot, ThreadSnapshot, ToolCallSnapshot};

// Convenience re-export
pub use crate::llm::LlmStreamEvent;
