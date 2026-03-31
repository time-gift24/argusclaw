//! Thread runtime actor skeleton.

use std::sync::Arc;

use crate::turn::TurnCancellation;
use argus_protocol::{
    MessageOverride, QueuedUserMessage, ThreadCommand, ThreadControlEvent, ThreadInbox,
    ThreadJobResult, ThreadRuntimeState,
};
use tokio::sync::{RwLock, mpsc};

use crate::command::ThreadRuntimeSnapshot;
use crate::error::ThreadError;
use crate::thread::Thread;
use crate::thread_handle::ThreadHandle;

/// Runtime decisions that the outer orchestrator can act on.
#[derive(Debug, Clone)]
pub(crate) enum ThreadRuntimeAction {
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

/// Lightweight thread runtime state machine.
#[derive(Debug)]
pub(crate) struct ThreadRuntime {
    state: ThreadRuntimeState,
    next_turn_number: u32,
    inbox: ThreadInbox,
    queue_depth: usize,
}

impl Default for ThreadRuntime {
    fn default() -> Self {
        Self {
            state: ThreadRuntimeState::Idle,
            next_turn_number: 1,
            inbox: ThreadInbox::default(),
            queue_depth: 0,
        }
    }
}

impl ThreadRuntime {
    /// Create a new thread runtime seeded from an already-persisted turn count.
    ///
    /// The returned runtime will start the next turn at `turn_count + 1` so
    /// runtime snapshots stay aligned with the owning [`Thread`].
    #[must_use]
    pub(crate) fn seeded_from_turn_count(turn_count: u32) -> Self {
        Self {
            state: ThreadRuntimeState::Idle,
            next_turn_number: turn_count.saturating_add(1),
            inbox: ThreadInbox::default(),
            queue_depth: 0,
        }
    }

    /// Handle a runtime command and return the immediate action.
    pub(crate) fn apply_command(&mut self, command: ThreadCommand) -> ThreadRuntimeAction {
        match command {
            ThreadCommand::EnqueueUserMessage {
                content,
                msg_override,
            } => {
                self.inbox.enqueue_user_message(content, msg_override);
                self.queue_depth = self.queue_depth.saturating_add(1);
                self.try_start_next_turn()
            }
            ThreadCommand::DeliverJobResult(result) => {
                self.inbox.deliver_job_result(result);
                self.queue_depth = self.queue_depth.saturating_add(1);
                self.try_start_next_turn()
            }
            ThreadCommand::CancelActiveTurn => self.cancel_active_turn(),
        }
    }

    /// Mark the current turn as finished and decide the next action.
    pub(crate) fn finish_active_turn(&mut self) -> ThreadRuntimeAction {
        self.state = ThreadRuntimeState::Idle;
        self.try_start_next_turn()
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

    pub(crate) fn claim_queued_job_result(&mut self, job_id: &str) -> Option<ThreadJobResult> {
        let claimed = self.inbox.claim_job_result(job_id);
        if claimed.is_some() {
            self.queue_depth = self.queue_depth.saturating_sub(1);
        }
        claimed
    }

    fn try_start_next_turn(&mut self) -> ThreadRuntimeAction {
        if !matches!(self.state, ThreadRuntimeState::Idle) {
            return ThreadRuntimeAction::Noop;
        }

        match self.take_next_turn_message() {
            Some(message) => self.start_turn(message),
            None => ThreadRuntimeAction::Noop,
        }
    }

    fn start_turn(&mut self, message: QueuedUserMessage) -> ThreadRuntimeAction {
        let turn_number = self.next_turn_number;
        self.next_turn_number = self.next_turn_number.saturating_add(1);
        self.state = ThreadRuntimeState::Running { turn_number };

        ThreadRuntimeAction::StartTurn {
            turn_number,
            content: message.content,
            msg_override: message.msg_override,
        }
    }

    fn cancel_active_turn(&mut self) -> ThreadRuntimeAction {
        match self.state {
            ThreadRuntimeState::Running { turn_number }
            | ThreadRuntimeState::WaitingForApproval { turn_number } => {
                self.state = ThreadRuntimeState::Stopping { turn_number };
                ThreadRuntimeAction::StopTurn { turn_number }
            }
            ThreadRuntimeState::Idle | ThreadRuntimeState::Stopping { .. } => {
                ThreadRuntimeAction::Noop
            }
        }
    }

    fn take_next_turn_message(&mut self) -> Option<QueuedUserMessage> {
        let message = self.inbox.take_next_turn_message();
        if message.is_some() {
            self.queue_depth = self.queue_depth.saturating_sub(1);
        }
        message
    }
}

/// Spawn the async thread runtime actor loop.
pub(crate) fn spawn_runtime_actor(thread: Arc<RwLock<Thread>>) {
    tokio::spawn(async move {
        let (mut control_rx, mailbox, seeded_turn_count) = {
            let mut guard = thread.write().await;
            let control_rx = match guard.take_control_rx() {
                Some(rx) => rx,
                None => {
                    tracing::warn!("thread control receiver already taken");
                    return;
                }
            };
            (control_rx, guard.mailbox(), guard.turn_count())
        };

        let (turn_done_tx, mut turn_done_rx) = mpsc::unbounded_channel();
        let mut runtime_handle =
            ThreadHandle::with_runtime(ThreadRuntime::seeded_from_turn_count(seeded_turn_count));
        let mut active_turn_cancellation: Option<TurnCancellation> = None;

        loop {
            tokio::select! {
                Some(control_event) = control_rx.recv() => {
                    let runtime_action = match control_event {
                        ThreadControlEvent::UserMessage { content, msg_override } => {
                            // Production: user messages are queued FIFO while a turn is running.
                            runtime_handle.dispatch(ThreadCommand::EnqueueUserMessage {
                                content,
                                msg_override,
                            })
                        }
                        ThreadControlEvent::JobResult(result) => {
                            // Production: job results are queued FIFO while a turn is running.
                            runtime_handle.dispatch(ThreadCommand::DeliverJobResult(result))
                        }
                        ThreadControlEvent::UserInterrupt { content } => {
                            // Production: UserInterrupt is an immediate stop signal for the active
                            // turn. The interrupt content is not currently used as redirect text.
                            let _ = content;
                            runtime_handle.dispatch(ThreadCommand::CancelActiveTurn)
                        }
                        ThreadControlEvent::ClaimQueuedJobResult { job_id, reply_tx } => {
                            let claimed = runtime_handle.claim_queued_job_result(&job_id);
                            let _ = reply_tx.send(claimed);
                            ThreadRuntimeAction::Noop
                        }
                    };

                    process_runtime_action(
                        Arc::clone(&thread),
                        &mut runtime_handle,
                        &mut active_turn_cancellation,
                        runtime_action,
                        &turn_done_tx,
                    )
                    .await;
                }
                Some(result) = turn_done_rx.recv() => {
                    active_turn_cancellation = None;
                    let finish_result = {
                        let mut guard = thread.write().await;
                        guard.finish_turn(result).await
                    };

                    if let Err(error) = finish_result {
                        tracing::error!("turn failed: {}", error);
                    }

                    {
                        let mut guard = mailbox.lock().await;
                        guard.clear_interrupts_for_idle_handoff();
                    }

                    let runtime_action = runtime_handle.finish_active_turn();
                    process_runtime_action(
                        Arc::clone(&thread),
                        &mut runtime_handle,
                        &mut active_turn_cancellation,
                        runtime_action,
                        &turn_done_tx,
                    )
                    .await;
                }
                else => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_protocol::{AgentId, ThreadJobResult};

    fn queued_job_result(job_id: &str) -> ThreadJobResult {
        ThreadJobResult {
            job_id: job_id.to_string(),
            success: true,
            message: format!("result for {job_id}"),
            token_usage: None,
            agent_id: AgentId::new(7),
            agent_display_name: "Worker".to_string(),
            agent_description: "Background worker".to_string(),
        }
    }

    #[test]
    fn runtime_turn_numbering_starts_after_seeded_turn_count() {
        let mut runtime = ThreadRuntime::seeded_from_turn_count(3);
        let action = runtime.apply_command(ThreadCommand::EnqueueUserMessage {
            content: "hello".to_string(),
            msg_override: None,
        });

        assert!(matches!(
            action,
            ThreadRuntimeAction::StartTurn { turn_number: 4, .. }
        ));
    }

    #[test]
    fn runtime_cancelled_turn_starts_next_queued_turn_after_finish() {
        let mut runtime = ThreadRuntime::default();

        let first = runtime.apply_command(ThreadCommand::EnqueueUserMessage {
            content: "first".to_string(),
            msg_override: None,
        });
        assert!(matches!(
            first,
            ThreadRuntimeAction::StartTurn { turn_number: 1, .. }
        ));

        let second = runtime.apply_command(ThreadCommand::EnqueueUserMessage {
            content: "second".to_string(),
            msg_override: None,
        });
        assert!(matches!(second, ThreadRuntimeAction::Noop));

        let stop = runtime.apply_command(ThreadCommand::CancelActiveTurn);
        assert!(matches!(
            stop,
            ThreadRuntimeAction::StopTurn { turn_number: 1 }
        ));
        assert_eq!(
            runtime.state(),
            ThreadRuntimeState::Stopping { turn_number: 1 }
        );

        let next = runtime.finish_active_turn();
        assert!(matches!(
            next,
            ThreadRuntimeAction::StartTurn { turn_number: 2, .. }
        ));
    }

    #[test]
    fn claiming_queued_job_result_removes_it_and_preserves_other_work() {
        let mut runtime = ThreadRuntime::default();

        let first = runtime.apply_command(ThreadCommand::EnqueueUserMessage {
            content: "first".to_string(),
            msg_override: None,
        });
        assert!(matches!(
            first,
            ThreadRuntimeAction::StartTurn { turn_number: 1, .. }
        ));

        assert!(matches!(
            runtime.apply_command(ThreadCommand::DeliverJobResult(queued_job_result("job-1"))),
            ThreadRuntimeAction::Noop
        ));
        assert!(matches!(
            runtime.apply_command(ThreadCommand::EnqueueUserMessage {
                content: "follow-up".to_string(),
                msg_override: None,
            }),
            ThreadRuntimeAction::Noop
        ));
        assert_eq!(runtime.snapshot().queue_depth, 2);

        let claimed = runtime.claim_queued_job_result("job-1");
        assert_eq!(
            claimed.as_ref().map(|result| result.job_id.as_str()),
            Some("job-1")
        );
        assert_eq!(runtime.snapshot().queue_depth, 1);

        let next = runtime.finish_active_turn();
        assert!(matches!(
            next,
            ThreadRuntimeAction::StartTurn { content, .. } if content == "follow-up"
        ));
    }
}

#[allow(clippy::items_after_test_module)]
async fn process_runtime_action(
    thread: Arc<RwLock<Thread>>,
    runtime_handle: &mut ThreadHandle,
    active_turn_cancellation: &mut Option<TurnCancellation>,
    action: ThreadRuntimeAction,
    turn_done_tx: &mpsc::UnboundedSender<std::result::Result<crate::TurnOutput, ThreadError>>,
) {
    let mut next_action = action;
    loop {
        match next_action {
            ThreadRuntimeAction::StartTurn {
                turn_number,
                content,
                msg_override,
            } => {
                match start_turn_task(
                    Arc::clone(&thread),
                    content,
                    msg_override,
                    turn_done_tx.clone(),
                )
                .await
                {
                    Ok(cancellation) => {
                        *active_turn_cancellation = Some(cancellation);
                    }
                    Err(error) => {
                        tracing::error!(
                            turn_number,
                            queue_depth = runtime_handle.snapshot().queue_depth,
                            "failed to start queued turn: {}",
                            error
                        );
                        next_action = runtime_handle.finish_active_turn();
                        continue;
                    }
                }
                break;
            }
            ThreadRuntimeAction::StopTurn { turn_number } => {
                if let Some(cancellation) = active_turn_cancellation.as_ref() {
                    tracing::info!(turn_number, "cancelling active turn");
                    cancellation.cancel();
                } else {
                    tracing::warn!(turn_number, "stop-turn requested but no active turn handle");
                }
                break;
            }
            ThreadRuntimeAction::Noop => break,
        }
    }
}

#[allow(clippy::items_after_test_module)]
async fn start_turn_task(
    thread: Arc<RwLock<Thread>>,
    content: String,
    msg_override: Option<MessageOverride>,
    turn_done_tx: mpsc::UnboundedSender<std::result::Result<crate::TurnOutput, ThreadError>>,
) -> std::result::Result<TurnCancellation, ThreadError> {
    let cancellation = TurnCancellation::new();
    let turn = {
        let mut guard = thread.write().await;
        guard
            .begin_turn(content, msg_override, cancellation.clone())
            .await?
    };

    tokio::spawn(async move {
        let result = turn.execute().await.map_err(ThreadError::TurnFailed);
        let _ = turn_done_tx.send(result);
    });

    Ok(cancellation)
}
