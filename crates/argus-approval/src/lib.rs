//! Approval management for ArgusWing agents.
//!
//! This crate provides approval management functionality that gates dangerous
//! operations behind human approval. It integrates with the hook system to
//! intercept tool calls before execution.
//!
//! # Overview
//!
//! When an agent attempts a dangerous operation (e.g., `shell_exec`), the system
//! creates an [`ApprovalRequest`] and pauses execution until a human operator
//! responds. The [`ApprovalPolicy`] configures which tools require approval.
//!
//! # Runtime Allow List
//!
//! The [`RuntimeAllowList`] tracks which tools have been marked as "allowed"
//! at runtime. This allows users to:
//! - Allow a specific tool for the rest of the session
//! - Allow all tools for the rest of the session
//!
//! This state is NOT persisted and resets on application restart.
//!
//! # Usage
//!
//! ```ignore
//! use argus_approval::{ApprovalManager, ApprovalPolicy, ApprovalHook, RuntimeAllowList};
//! use std::sync::{Arc, RwLock};
//!
//! // Create manager with default policy (shell_exec requires approval)
//! let policy = ApprovalPolicy::default();
//! let manager = Arc::new(ApprovalManager::new(policy.clone()));
//!
//! // Create runtime allow list
//! let allow_list = Arc::new(RwLock::new(RuntimeAllowList::new()));
//!
//! // Create approval hook
//! let hook = ApprovalHook::new(manager, policy, allow_list, "agent-1");
//! ```

mod error;
mod hook;
mod manager;
mod policy;
mod runtime_allow;

pub use error::ApprovalError;
pub use hook::ApprovalHook;
pub use manager::ApprovalManager;
pub use manager::MAX_PENDING_PER_AGENT;
pub use policy::ApprovalPolicy;
pub use policy::{MAX_ACTION_LEN, MAX_TIMEOUT_SECS, MAX_TOOL_NAME_LEN, MIN_TIMEOUT_SECS};
pub use runtime_allow::RuntimeAllowList;

// Re-export approval types from argus-protocol for convenience
pub use argus_protocol::{
    ApprovalDecision, ApprovalEvent, ApprovalRequest, ApprovalResponse,
};
