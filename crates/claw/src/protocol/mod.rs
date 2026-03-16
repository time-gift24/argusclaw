//! Protocol types shared across modules.
//!
//! This module contains types that need to be shared between multiple modules
//! to avoid circular dependencies (e.g., between `approval` and `tool`).

mod approval;
mod hooks;
mod risk_level;
mod runtime_agent;
mod thread_event;
mod thread_id;
mod thread_snapshot;
mod token_usage;

pub use approval::{
    ApprovalDecision, ApprovalEvent, ApprovalRequest, ApprovalResponse, MAX_ACTION_LEN,
    MAX_PENDING_PER_AGENT, MAX_TIMEOUT_SECS, MAX_TOOL_NAME_LEN, MIN_TIMEOUT_SECS,
};
pub use hooks::{
    BeforeCallLLMContext, BeforeCallLLMResult, HookAction, HookEvent, HookHandler, HookRegistry,
    ToolHookContext,
};
pub use risk_level::RiskLevel;
pub use runtime_agent::RuntimeAgentHandle;
pub use thread_event::ThreadEvent;
pub use thread_id::ThreadId;
pub use thread_snapshot::{ThreadMessageSnapshot, ThreadSnapshot, ToolCallSnapshot};
pub use token_usage::TokenUsage;

// Convenience re-export
pub use crate::llm::LlmStreamEvent;
