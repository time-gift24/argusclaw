//! Per-thread approval state tracking which tools have been approved.
//!
//! Replaces the old `RuntimeAllowList` from `argus-approval`.

use std::collections::HashSet;

/// Tracks approval decisions for tool execution within a single Thread.
///
/// This state is shared across all Turns in the same Thread. When a user
/// approves a tool (or all tools) for the session, the state is updated
/// here and subsequent Turns skip the approval check.
#[derive(Debug, Clone, Default)]
pub struct ApprovalState {
    /// Tool names that have been individually approved for this session.
    approved_tools: HashSet<String>,
    /// If true, all tools are pre-approved for this session.
    approve_all: bool,
}

impl ApprovalState {
    /// Create a new, empty approval state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a tool has been approved (individually or via approve-all).
    pub fn is_approved(&self, tool_name: &str) -> bool {
        self.approve_all || self.approved_tools.contains(tool_name)
    }

    /// Approve a specific tool for the rest of this session.
    pub fn approve_tool(&mut self, tool_name: &str) {
        self.approved_tools.insert(tool_name.to_owned());
    }

    /// Approve all tools for the rest of this session.
    pub fn approve_all(&mut self) {
        self.approve_all = true;
    }

    /// Reset all approval decisions.
    pub fn reset(&mut self) {
        self.approved_tools.clear();
        self.approve_all = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_no_approvals() {
        let state = ApprovalState::new();
        assert!(!state.is_approved("shell"));
        assert!(!state.is_approved("http"));
    }

    #[test]
    fn approve_specific_tool() {
        let mut state = ApprovalState::new();
        state.approve_tool("shell");
        assert!(state.is_approved("shell"));
        assert!(!state.is_approved("http"));
    }

    #[test]
    fn approve_all_grants_all() {
        let mut state = ApprovalState::new();
        state.approve_all();
        assert!(state.is_approved("shell"));
        assert!(state.is_approved("http"));
        assert!(state.is_approved("anything"));
    }

    #[test]
    fn reset_clears_everything() {
        let mut state = ApprovalState::new();
        state.approve_all();
        state.reset();
        assert!(!state.is_approved("shell"));
    }
}
