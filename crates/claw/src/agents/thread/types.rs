//! Thread core types.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agents::turn::TokenUsage;
use crate::llm::LlmStreamEvent;

/// Unique identifier for a Thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ThreadId(pub Uuid);

impl ThreadId {
    /// Create a new unique ThreadId.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ThreadId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
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

/// Thread event broadcast to subscribers (CLI, Tauri).
#[derive(Debug, Clone)]
pub enum ThreadEvent {
    /// Turn is processing, streaming LLM/tool events.
    Processing {
        thread_id: ThreadId,
        turn_number: u32,
        event: LlmStreamEvent,
    },
    /// Turn completed successfully.
    TurnCompleted {
        thread_id: ThreadId,
        turn_number: u32,
        token_usage: TokenUsage,
    },
    /// Turn failed.
    TurnFailed {
        thread_id: ThreadId,
        turn_number: u32,
        error: String,
    },
    /// Thread entered idle state.
    Idle { thread_id: ThreadId },
    /// Context was compacted.
    Compacted {
        thread_id: ThreadId,
        new_token_count: u32,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_id_new_creates_unique_ids() {
        let id1 = ThreadId::new();
        let id2 = ThreadId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn thread_id_default_creates_new_id() {
        let id = ThreadId::default();
        assert!(!id.0.is_nil());
    }

    #[test]
    fn thread_id_display() {
        let id = ThreadId::new();
        let display = format!("{}", id);
        assert!(!display.is_empty());
        assert_eq!(display.len(), 36); // UUID format: 8-4-4-4-12
    }

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

    #[test]
    fn thread_id_serde_roundtrip() {
        let id = ThreadId::new();
        let json = serde_json::to_string(&id).unwrap();
        let back: ThreadId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
