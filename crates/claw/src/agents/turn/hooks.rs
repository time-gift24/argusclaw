//! Hook system for intercepting and modifying Turn execution.
//!
//! This module re-exports hooks from the protocol module for backward compatibility.

pub use crate::protocol::{
    BeforeCallLLMContext, BeforeCallLLMResult, HookAction, HookEvent, HookHandler, HookRegistry,
    ToolHookContext,
};
