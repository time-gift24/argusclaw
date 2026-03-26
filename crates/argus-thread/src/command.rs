//! Thread runtime snapshot glue.

use argus_protocol::ThreadRuntimeState;

/// Snapshot of lightweight thread runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadRuntimeSnapshot {
    /// Current runtime state.
    pub state: ThreadRuntimeState,
    /// Number of queued items waiting while a turn is running.
    pub queue_depth: usize,
}

impl Default for ThreadRuntimeSnapshot {
    fn default() -> Self {
        Self {
            state: ThreadRuntimeState::Idle,
            queue_depth: 0,
        }
    }
}
