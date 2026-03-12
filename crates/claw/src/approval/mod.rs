//! Approval module for gating dangerous agent operations behind human approval.
//!
//! # Overview
//!
//! When an agent attempts a dangerous operation (e.g., `shell_exec`), the system
//! creates an [`ApprovalRequest`] and pauses execution until a human operator
//! responds. The [`ApprovalPolicy`] configures which tools require approval.
//!
//! # Usage
//!
//! ```ignore
//! use claw::approval::{ApprovalManager, ApprovalPolicy, ApprovalRequest};
//!
//! // Create manager with default policy (shell_exec requires approval)
//! let manager = ApprovalManager::new(ApprovalPolicy::default());
//!
//! // Check if tool needs approval
//! if manager.requires_approval("shell_exec") {
//!     let request = ApprovalRequest::new(
//!         "agent-001".to_string(),
//!         "shell_exec".to_string(),
//!         "rm -rf /tmp/cache".to_string(),
//!         60, // timeout in seconds
//!     );
//!
//!     // This will block until approved, denied, or timed out
//!     let decision = manager.request_approval(request).await;
//! }
//! ```
//!
//! # CLI Testing
//!
//! ```bash
//! # View current policy
//! cargo run --features dev -- approval policy
//!
//! # Test approval flow (will timeout after 10s if not resolved)
//! cargo run --features dev -- approval test --tool shell_exec --timeout 10
//!
//! # Resolve in another terminal
//! cargo run --features dev -- approval resolve --id <uuid> --approve
//! ```

mod error;
mod manager;
mod policy;
mod types;

mod approval_hook;

pub use approval_hook::ApprovalHook;
pub use error::ApprovalError;
pub use manager::ApprovalManager;
pub use policy::ApprovalPolicy;
pub use types::{
    ApprovalDecision, ApprovalEvent, ApprovalRequest, ApprovalResponse, MAX_ACTION_LEN,
    MAX_PENDING_PER_AGENT, MAX_TIMEOUT_SECS, MAX_TOOL_NAME_LEN, MIN_TIMEOUT_SECS,
};
// Re-export RiskLevel from protocol for backward compatibility
pub use crate::protocol::RiskLevel;
