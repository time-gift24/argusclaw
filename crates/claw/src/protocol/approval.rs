//! Approval protocol types.
//!
//! Re-exports shared types from argus_protocol and provides claw-specific types.

// Re-export shared types from argus_protocol
pub use argus_protocol::{
    ApprovalRequest, ApprovalResponse,
};

// Claw-specific constants
/// Maximum length of tool names (chars).
pub const MAX_TOOL_NAME_LEN: usize = 64;

/// Maximum length of an action summary (chars).
pub const MAX_ACTION_LEN: usize = 512;

/// Minimum approval timeout in seconds.
pub const MIN_TIMEOUT_SECS: u64 = 10;

/// Maximum approval timeout in seconds.
pub const MAX_TIMEOUT_SECS: u64 = 300;

/// Max pending requests per agent.
pub const MAX_PENDING_PER_AGENT: usize = 5;

// Claw-specific event type (uses argus_protocol types)
use serde::{Deserialize, Serialize};

/// Approval event for subscribers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalEvent {
    /// A new approval request was created.
    RequestCreated(ApprovalRequest),
    /// An approval request was resolved.
    Resolved(ApprovalResponse),
}
