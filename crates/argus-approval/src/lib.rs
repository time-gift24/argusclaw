//! Approval management for ArgusWing agents.
//!
//! This crate provides approval management functionality that gates dangerous
//! operations behind human approval. Each tool declares whether it requires
//! approval via `NamedTool::requires_approval()`, and the approval check is
//! performed inline in the Turn execution loop.
//!
//! # Overview
//!
//! When an agent attempts a dangerous operation (e.g., `shell`), the system
//! creates an [`ApprovalRequest`] and pauses execution until a human operator
//! responds. The [`ApprovalPolicy`] configures which tools require approval.
//!
//! # Usage
//!
//! ```ignore
//! use argus_approval::{ApprovalManager, ApprovalPolicy};
//! use std::sync::Arc;
//!
//! // Create manager with default policy
//! let policy = ApprovalPolicy::default();
//! let manager = Arc::new(ApprovalManager::new(policy.clone()));
//! ```

mod error;
mod manager;
mod policy;

pub use error::ApprovalError;
pub use manager::ApprovalManager;
pub use manager::MAX_PENDING_PER_AGENT;
pub use policy::ApprovalPolicy;
pub use policy::{MAX_ACTION_LEN, MAX_TIMEOUT_SECS, MAX_TOOL_NAME_LEN, MIN_TIMEOUT_SECS};

// Re-export approval types from argus-protocol for convenience
pub use argus_protocol::{ApprovalDecision, ApprovalEvent, ApprovalRequest, ApprovalResponse};
