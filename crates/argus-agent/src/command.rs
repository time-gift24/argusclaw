//! Thread runtime snapshot glue.

/// Internal runtime state for a loaded thread actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadRuntimeState {
    /// Runtime is idle and ready for work.
    Idle,
    /// Runtime is executing a turn.
    Running {
        /// Active turn number.
        turn_number: u32,
    },
    /// Runtime is stopping an active turn.
    Stopping {
        /// Active turn number being stopped.
        turn_number: u32,
    },
    /// Runtime is paused waiting for an approval decision.
    WaitingForApproval {
        /// Turn number blocked on approval.
        turn_number: u32,
    },
}

/// Snapshot of lightweight thread runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThreadRuntimeSnapshot {
    /// Current runtime state.
    pub(crate) state: ThreadRuntimeState,
    /// Number of queued items waiting while a turn is running.
    pub(crate) queue_depth: usize,
}

impl Default for ThreadRuntimeSnapshot {
    fn default() -> Self {
        Self {
            state: ThreadRuntimeState::Idle,
            queue_depth: 0,
        }
    }
}
