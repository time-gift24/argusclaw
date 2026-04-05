//! Thread-local context for tool execution.
//!
//! Provides access to the current agent's ID during tool execution.
//! Set by the turn executor before invoking a tool, and cleared after.

use argus_protocol::AgentId;

thread_local! {
    static CURRENT_AGENT_ID: std::cell::RefCell<Option<AgentId>> = const { std::cell::RefCell::new(None) };
}

/// Set the current agent ID for tool execution.
/// This should be called before executing tools and cleared after.
pub fn set_current_agent_id(agent_id: AgentId) {
    CURRENT_AGENT_ID.with(|cell| {
        *cell.borrow_mut() = Some(agent_id);
    });
}

/// Clear the current agent ID after tool execution.
pub fn clear_current_agent_id() {
    CURRENT_AGENT_ID.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

/// Get the current agent ID, if set by the turn executor.
pub fn current_agent_id() -> Option<AgentId> {
    CURRENT_AGENT_ID.with(|cell| *cell.borrow())
}
