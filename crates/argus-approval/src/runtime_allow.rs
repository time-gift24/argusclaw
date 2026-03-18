//! Runtime approval state - tracks which tools are allowed without approval.
//!
//! This state is NOT persisted and resets on application restart.
//!
//! # Example
//!
//! ```ignore
//! use argus_approval::RuntimeAllowList;
//!
//! let mut allow_list = RuntimeAllowList::new();
//!
//! // Allow a specific tool
//! allow_list.allow_tool("shell_exec");
//! assert!(allow_list.is_allowed("shell_exec"));
//! assert!(!allow_list.is_allowed("file_write"));
//!
//! // Allow all tools
//! allow_list.allow_all();
//! assert!(allow_list.is_allowed("any_tool"));
//!
//! // Reset to default state
//! allow_list.reset();
//! assert!(!allow_list.is_allowed("shell_exec"));
//! ```

use std::collections::HashSet;

/// Tracks which tools have been marked as "allowed" at runtime.
///
/// This is used to bypass approval for tools that the user has explicitly
/// approved for the rest of the session.
#[derive(Debug, Clone)]
pub struct RuntimeAllowList {
    /// Specific tools that are allowed without approval.
    allowed_tools: HashSet<String>,
    /// If true, ALL tools are allowed without approval.
    allow_all: bool,
}

impl Default for RuntimeAllowList {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeAllowList {
    /// Create a new empty allow list.
    pub fn new() -> Self {
        Self {
            allowed_tools: HashSet::new(),
            allow_all: false,
        }
    }

    /// Allow a specific tool for this session.
    ///
    /// After calling this, `is_allowed(tool_name)` will return `true`.
    pub fn allow_tool(&mut self, tool_name: &str) {
        self.allowed_tools.insert(tool_name.to_string());
    }

    /// Allow ALL tools for this session.
    ///
    /// After calling this, `is_allowed(any_tool)` will return `true`.
    pub fn allow_all(&mut self) {
        self.allow_all = true;
    }

    /// Check if a tool is allowed (no approval needed).
    ///
    /// Returns `true` if:
    /// - `allow_all()` has been called, OR
    /// - The tool was added via `allow_tool()`
    pub fn is_allowed(&self, tool_name: &str) -> bool {
        self.allow_all || self.allowed_tools.contains(tool_name)
    }

    /// Revoke permission for a specific tool.
    ///
    /// If `allow_all` is set, this does NOT revoke the tool - use
    /// `revoke_all()` first to disable allow-all mode.
    pub fn revoke_tool(&mut self, tool_name: &str) {
        self.allowed_tools.remove(tool_name);
    }

    /// Revoke "allow all" mode - back to specific tool permissions.
    ///
    /// After calling this, only tools in `allowed_tools` will be allowed.
    pub fn revoke_all(&mut self) {
        self.allow_all = false;
    }

    /// Reset to default state (no tools allowed).
    ///
    /// Clears both the allow list and the allow-all flag.
    pub fn reset(&mut self) {
        self.allowed_tools.clear();
        self.allow_all = false;
    }

    /// Check if "allow all" mode is active.
    pub fn is_allow_all(&self) -> bool {
        self.allow_all
    }

    /// Get the list of specifically allowed tools.
    pub fn allowed_tools(&self) -> impl Iterator<Item = &String> {
        self.allowed_tools.iter()
    }

    /// Get the number of specifically allowed tools.
    pub fn allowed_count(&self) -> usize {
        self.allowed_tools.len()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_empty() {
        let list = RuntimeAllowList::new();
        assert!(!list.is_allowed("shell_exec"));
        assert!(!list.is_allowed("file_write"));
        assert!(!list.is_allow_all());
        assert_eq!(list.allowed_count(), 0);
    }

    #[test]
    fn default_is_empty() {
        let list = RuntimeAllowList::default();
        assert!(!list.is_allowed("any_tool"));
    }

    #[test]
    fn allow_tool() {
        let mut list = RuntimeAllowList::new();
        list.allow_tool("shell_exec");

        assert!(list.is_allowed("shell_exec"));
        assert!(!list.is_allowed("file_write"));
        assert_eq!(list.allowed_count(), 1);
    }

    #[test]
    fn allow_multiple_tools() {
        let mut list = RuntimeAllowList::new();
        list.allow_tool("shell_exec");
        list.allow_tool("file_write");
        list.allow_tool("file_delete");

        assert!(list.is_allowed("shell_exec"));
        assert!(list.is_allowed("file_write"));
        assert!(list.is_allowed("file_delete"));
        assert!(!list.is_allowed("other"));
        assert_eq!(list.allowed_count(), 3);
    }

    #[test]
    fn allow_tool_is_idempotent() {
        let mut list = RuntimeAllowList::new();
        list.allow_tool("shell_exec");
        list.allow_tool("shell_exec");

        assert_eq!(list.allowed_count(), 1);
    }

    #[test]
    fn allow_all() {
        let mut list = RuntimeAllowList::new();
        list.allow_all();

        assert!(list.is_allow_all());
        assert!(list.is_allowed("shell_exec"));
        assert!(list.is_allowed("any_random_tool"));
        assert!(list.is_allowed("future_tool"));
    }

    #[test]
    fn revoke_tool() {
        let mut list = RuntimeAllowList::new();
        list.allow_tool("shell_exec");
        assert!(list.is_allowed("shell_exec"));

        list.revoke_tool("shell_exec");
        assert!(!list.is_allowed("shell_exec"));
        assert_eq!(list.allowed_count(), 0);
    }

    #[test]
    fn revoke_nonexistent_is_noop() {
        let mut list = RuntimeAllowList::new();
        list.allow_tool("shell_exec");
        list.revoke_tool("nonexistent");

        assert_eq!(list.allowed_count(), 1);
        assert!(list.is_allowed("shell_exec"));
    }

    #[test]
    fn revoke_all_disables_allow_all() {
        let mut list = RuntimeAllowList::new();
        list.allow_all();
        assert!(list.is_allow_all());

        list.revoke_all();
        assert!(!list.is_allow_all());
        assert!(!list.is_allowed("shell_exec"));
    }

    #[test]
    fn revoke_all_preserves_specific_tools() {
        let mut list = RuntimeAllowList::new();
        list.allow_tool("shell_exec");
        list.allow_all();
        assert!(list.is_allow_all());

        list.revoke_all();
        assert!(!list.is_allow_all());
        // Note: when allow_all is set, is_allowed returns true for everything.
        // After revoke_all, only specifically allowed tools should pass.
        assert!(list.is_allowed("shell_exec"));
    }

    #[test]
    fn reset_clears_everything() {
        let mut list = RuntimeAllowList::new();
        list.allow_tool("shell_exec");
        list.allow_tool("file_write");
        list.allow_all();

        list.reset();

        assert!(!list.is_allow_all());
        assert_eq!(list.allowed_count(), 0);
        assert!(!list.is_allowed("shell_exec"));
        assert!(!list.is_allowed("file_write"));
    }

    #[test]
    fn allowed_tools_iterator() {
        let mut list = RuntimeAllowList::new();
        list.allow_tool("shell_exec");
        list.allow_tool("file_write");

        let tools: Vec<_> = list.allowed_tools().collect();
        assert_eq!(tools.len(), 2);
        assert!(tools.contains(&&"shell_exec".to_string()));
        assert!(tools.contains(&&"file_write".to_string()));
    }

    #[test]
    fn clone() {
        let mut list = RuntimeAllowList::new();
        list.allow_tool("shell_exec");
        list.allow_all();

        let cloned = list.clone();
        assert!(cloned.is_allow_all());
        assert!(cloned.is_allowed("shell_exec"));
        assert!(cloned.is_allowed("any_tool"));
    }
}
