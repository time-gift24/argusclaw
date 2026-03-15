//! Approval types - re-exported from protocol.
//!
//! The canonical definitions live in `crate::protocol::approval`.
//! This file exists only for backward compatibility within the crate.

pub use crate::protocol::{
    ApprovalDecision, ApprovalEvent, ApprovalRequest, ApprovalResponse, MAX_ACTION_LEN,
    MAX_PENDING_PER_AGENT, MAX_TIMEOUT_SECS, MAX_TOOL_NAME_LEN, MIN_TIMEOUT_SECS,
};
