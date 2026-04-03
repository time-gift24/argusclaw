use crate::command::ThreadRuntimeSnapshot;
use argus_protocol::{
    MailboxMessage, MessageOverride, QueuedUserMessage, ThreadCommand, ThreadControlEvent,
    ThreadMailbox, ThreadRuntimeState,
};

/// Runtime decisions that the thread-owned reactor can emit.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum ThreadReactorAction {
    /// Start a new turn immediately.
    StartTurn {
        /// Turn number to execute.
        turn_number: u32,
        /// User message content.
        content: String,
        /// Optional per-message overrides.
        msg_override: Option<MessageOverride>,
    },
    /// Active turn should be stopped.
    StopTurn {
        /// Turn number being stopped.
        turn_number: u32,
    },
    /// No immediate action is required.
    Noop,
}

/// Lightweight thread-owned reactor state machine.
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct ThreadReactor {
    state: ThreadRuntimeState,
    next_turn_number: u32,
    queue_depth: usize,
}

impl Default for ThreadReactor {
    fn default() -> Self {
        Self {
            state: ThreadRuntimeState::Idle,
            next_turn_number: 1,
            queue_depth: 0,
        }
    }
}

#[allow(dead_code)]
impl ThreadReactor {
    /// Create a new thread reactor seeded from the owning thread's next turn number.
    #[must_use]
    pub(crate) fn seeded_from_next_turn_number(next_turn_number: u32) -> Self {
        Self {
            state: ThreadRuntimeState::Idle,
            next_turn_number,
            queue_depth: 0,
        }
    }

    /// Handle a runtime command and return the immediate action.
    pub(crate) fn apply_command(
        &mut self,
        command: ThreadCommand,
        mailbox: &mut ThreadMailbox,
    ) -> ThreadReactorAction {
        match command {
            ThreadCommand::EnqueueUserMessage {
                content,
                msg_override,
            } => {
                mailbox.push(ThreadControlEvent::UserMessage {
                    content,
                    msg_override,
                });
                self.queue_depth = mailbox.pending_len();
                self.try_start_next_turn(mailbox)
            }
            ThreadCommand::EnqueueMailboxMessage(message) => {
                mailbox.push(ThreadControlEvent::DeliverMailboxMessage(message));
                self.queue_depth = mailbox.pending_len();
                self.try_start_next_turn(mailbox)
            }
            ThreadCommand::CancelActiveTurn => self.cancel_active_turn(),
        }
    }

    /// Mark the current turn as finished and decide the next action.
    pub(crate) fn finish_active_turn(
        &mut self,
        mailbox: &mut ThreadMailbox,
    ) -> ThreadReactorAction {
        self.state = ThreadRuntimeState::Idle;
        self.queue_depth = mailbox.pending_len();
        self.try_start_next_turn(mailbox)
    }

    /// Return an immutable runtime snapshot.
    #[must_use]
    pub(crate) fn snapshot(&self) -> ThreadRuntimeSnapshot {
        ThreadRuntimeSnapshot {
            state: self.state,
            queue_depth: self.queue_depth,
        }
    }

    /// Return current runtime state.
    #[must_use]
    pub(crate) fn state(&self) -> ThreadRuntimeState {
        self.state
    }

    pub(crate) fn claim_queued_job_result(
        &mut self,
        mailbox: &mut ThreadMailbox,
        job_id: &str,
    ) -> Option<MailboxMessage> {
        let claimed = mailbox.claim_job_result(job_id);
        if claimed.is_some() {
            self.queue_depth = mailbox.pending_len();
        }
        claimed
    }

    pub(crate) fn mark_waiting_for_approval(&mut self, turn_number: u32) {
        if matches!(self.state, ThreadRuntimeState::Running { turn_number: active } if active == turn_number)
        {
            self.state = ThreadRuntimeState::WaitingForApproval { turn_number };
        }
    }

    pub(crate) fn mark_running_after_approval(&mut self, turn_number: u32) {
        if matches!(self.state, ThreadRuntimeState::WaitingForApproval { turn_number: active } if active == turn_number)
        {
            self.state = ThreadRuntimeState::Running { turn_number };
        }
    }

    fn try_start_next_turn(&mut self, mailbox: &mut ThreadMailbox) -> ThreadReactorAction {
        if !matches!(self.state, ThreadRuntimeState::Idle) {
            return ThreadReactorAction::Noop;
        }

        match self.take_next_turn_message(mailbox) {
            Some(message) => self.start_turn(message),
            None => ThreadReactorAction::Noop,
        }
    }

    fn start_turn(&mut self, message: QueuedUserMessage) -> ThreadReactorAction {
        let turn_number = self.next_turn_number;
        self.next_turn_number = self.next_turn_number.saturating_add(1);
        self.state = ThreadRuntimeState::Running { turn_number };

        ThreadReactorAction::StartTurn {
            turn_number,
            content: message.content,
            msg_override: message.msg_override,
        }
    }

    fn cancel_active_turn(&mut self) -> ThreadReactorAction {
        match self.state {
            ThreadRuntimeState::Running { turn_number }
            | ThreadRuntimeState::WaitingForApproval { turn_number } => {
                self.state = ThreadRuntimeState::Stopping { turn_number };
                ThreadReactorAction::StopTurn { turn_number }
            }
            ThreadRuntimeState::Idle | ThreadRuntimeState::Stopping { .. } => {
                ThreadReactorAction::Noop
            }
        }
    }

    fn take_next_turn_message(&mut self, mailbox: &mut ThreadMailbox) -> Option<QueuedUserMessage> {
        let message = mailbox.take_next_turn_message();
        self.queue_depth = mailbox.pending_len();
        message
    }
}
