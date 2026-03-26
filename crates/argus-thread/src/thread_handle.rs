//! Thread handle actor façade.

use argus_protocol::{ThreadCommand, ThreadJobResult, ThreadRuntimeState};

use crate::command::ThreadRuntimeSnapshot;
use crate::runtime::{ThreadRuntime, ThreadRuntimeAction};

/// Handle API for interacting with a thread runtime.
#[derive(Debug, Default)]
pub struct ThreadHandle {
    runtime: ThreadRuntime,
}

impl ThreadHandle {
    /// Create a handle with a fresh runtime.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a handle around an existing runtime.
    #[must_use]
    pub(crate) fn with_runtime(runtime: ThreadRuntime) -> Self {
        Self { runtime }
    }

    /// Dispatch a low-level runtime command.
    pub(crate) fn dispatch(&mut self, command: ThreadCommand) -> ThreadRuntimeAction {
        self.runtime.apply_command(command)
    }

    /// Mark the active turn finished so runtime can pick queued work.
    pub(crate) fn finish_active_turn(&mut self) -> ThreadRuntimeAction {
        self.runtime.finish_active_turn()
    }

    /// Claim a queued job result from the runtime inbox.
    pub(crate) fn claim_queued_job_result(&mut self, job_id: &str) -> Option<ThreadJobResult> {
        self.runtime.claim_queued_job_result(job_id)
    }

    /// Read runtime snapshot for diagnostics/testing.
    #[must_use]
    pub fn snapshot(&self) -> ThreadRuntimeSnapshot {
        self.runtime.snapshot()
    }

    /// Read runtime state.
    #[must_use]
    pub fn state(&self) -> ThreadRuntimeState {
        self.runtime.state()
    }
}
