//! Protocol types shared across modules.
//!
//! This module contains types that need to be shared between multiple modules
//! to avoid circular dependencies (e.g., between `approval` and `tool`).

mod hooks;
mod risk_level;

pub use hooks::{
    BeforeCallLLMContext, BeforeCallLLMResult, HookAction, HookEvent, HookHandler, HookRegistry,
    ToolHookContext,
};
pub use risk_level::RiskLevel;
