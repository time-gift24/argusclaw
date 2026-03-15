//! Protocol types shared across modules.
//!
//! This module contains types that need to be shared between multiple modules
//! to avoid circular dependencies (e.g., between `approval` and `tool`).

mod approval;
mod hooks;
mod risk_level;
mod thread_event;
mod thread_id;
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
pub use thread_event::ThreadEvent;
pub use thread_id::ThreadId;
pub use token_usage::TokenUsage;

// Convenience re-export
pub use crate::llm::LlmStreamEvent;
