//! Thread handle actor façade.

use argus_protocol::{MailboxMessage, ThreadCommand, ThreadMailbox, ThreadRuntimeState};

use crate::command::ThreadRuntimeSnapshot;
use crate::thread::{ThreadReactor, ThreadReactorAction as ThreadRuntimeAction};

/// Handle API for interacting with a thread runtime.
#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct ThreadHandle {
    runtime: ThreadReactor,
    mailbox: ThreadMailbox,
}

#[allow(dead_code)]
impl ThreadHandle {
    /// Create a handle with a fresh runtime.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a handle around an existing runtime.
    #[must_use]
    pub(crate) fn with_runtime(runtime: ThreadReactor) -> Self {
        Self {
            runtime,
            mailbox: ThreadMailbox::default(),
        }
    }

    /// Dispatch a low-level runtime command.
    pub(crate) fn dispatch(&mut self, command: ThreadCommand) -> ThreadRuntimeAction {
        self.runtime.apply_command(command, &mut self.mailbox)
    }

    /// Mark the active turn finished so runtime can pick queued work.
    pub(crate) fn finish_active_turn(&mut self, committed: bool) -> ThreadRuntimeAction {
        self.runtime
            .finish_active_turn(committed, &mut self.mailbox)
    }

    /// Claim a queued job result from the runtime inbox.
    pub(crate) fn claim_queued_job_result(&mut self, job_id: &str) -> Option<MailboxMessage> {
        self.runtime
            .claim_queued_job_result(&mut self.mailbox, job_id)
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
