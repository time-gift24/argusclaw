//! Thread core types.

use std::path::PathBuf;

use argus_protocol::{AgentId, SessionId, ThreadId, llm::ChatMessage};
use chrono::{DateTime, Utc};

/// Information about a Thread for listing and display.
#[derive(Debug, Clone)]
pub struct ThreadInfo {
    /// Thread ID.
    pub id: String,
    /// Number of messages in the current history view.
    pub message_count: usize,
    /// Current token count.
    pub token_count: u32,
    /// Number of turns completed.
    pub turn_count: u32,
    /// Number of plan items.
    pub plan_item_count: usize,
}

/// Eventually consistent runtime view exported by a loaded thread owner.
#[derive(Debug, Clone)]
pub struct ThreadRuntimeSnapshot {
    /// Strongly typed thread identifier.
    pub id: ThreadId,
    /// Owning session identifier for the runtime.
    pub session_id: SessionId,
    /// Optional persisted title.
    pub title: Option<String>,
    /// Last updated timestamp tracked by the runtime.
    pub updated_at: DateTime<Utc>,
    /// Visible committed history.
    pub history: Vec<ChatMessage>,
    /// Current committed turn count.
    pub turn_count: u32,
    /// Current token count.
    pub token_count: u32,
    /// Current plan item count.
    pub plan_item_count: usize,
    /// Best-effort runtime state.
    pub state: ThreadState,
    /// Cached provider/model label for UI and scheduler reads.
    pub provider_model: String,
    /// Cached agent display name for scheduler/job labels.
    pub agent_display_name: String,
    /// Cached agent identifier for runtime summaries.
    pub agent_id: AgentId,
    /// Cached agent description for job result shaping.
    pub agent_description: String,
    /// Cached system prompt from the frozen agent snapshot.
    pub agent_system_prompt: String,
    /// Cached trace base directory when tracing is enabled.
    pub trace_base_dir: Option<PathBuf>,
    /// Cached estimated memory bytes for pool summaries.
    pub estimated_memory_bytes: u64,
}

/// Thread state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThreadState {
    /// Thread is idle and ready to accept new messages.
    #[default]
    Idle,
    /// Thread is processing a Turn.
    Processing,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_state_default_is_idle() {
        assert_eq!(ThreadState::default(), ThreadState::Idle);
    }

    #[test]
    fn thread_state_equality() {
        assert_eq!(ThreadState::Idle, ThreadState::Idle);
        assert_eq!(ThreadState::Processing, ThreadState::Processing);
        assert_ne!(ThreadState::Idle, ThreadState::Processing);
    }
}
