//! Thread runtime actor skeleton.

use std::sync::Arc;

use crate::turn::TurnCancellation;
use argus_protocol::{
    MailboxMessage, MessageOverride, QueuedUserMessage, ThreadCommand, ThreadControlEvent,
    ThreadMailbox, ThreadRuntimeState,
};
use tokio::sync::{RwLock, mpsc};

use crate::command::ThreadRuntimeSnapshot;
use crate::error::ThreadError;
use crate::thread::Thread;

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
    queue_depth: usize,
}

impl Default for ThreadRuntime {
    fn default() -> Self {
        Self {
            state: ThreadRuntimeState::Idle,
            next_turn_number: 1,
            queue_depth: 0,
        }
    }
}

impl ThreadRuntime {
    /// Create a new thread runtime seeded from the owning thread's next turn number.
    ///
    /// The returned runtime will start the next turn at `next_turn_number` so
    /// runtime snapshots stay aligned with the owning [`Thread`].
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
    ) -> ThreadRuntimeAction {
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
    ) -> ThreadRuntimeAction {
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

    fn try_start_next_turn(&mut self, mailbox: &mut ThreadMailbox) -> ThreadRuntimeAction {
        if !matches!(self.state, ThreadRuntimeState::Idle) {
            return ThreadRuntimeAction::Noop;
        }

        match self.take_next_turn_message(mailbox) {
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

    fn take_next_turn_message(&mut self, mailbox: &mut ThreadMailbox) -> Option<QueuedUserMessage> {
        let message = mailbox.take_next_turn_message();
        self.queue_depth = mailbox.pending_len();
        message
    }
}

/// Spawn the async thread runtime actor loop.
pub(crate) fn spawn_runtime_actor(thread: Arc<RwLock<Thread>>) {
    tokio::spawn(async move {
        let (mut control_rx, mailbox, next_turn_number) = {
            let mut guard = thread.write().await;
            let control_rx = match guard.take_control_rx() {
                Some(rx) => rx,
                None => {
                    tracing::warn!("thread control receiver already taken");
                    return;
                }
            };
            (
                control_rx,
                guard.mailbox(),
                guard.next_turn_number_for_runtime(),
            )
        };

        let (turn_done_tx, mut turn_done_rx) = mpsc::unbounded_channel();
        let mut runtime = ThreadRuntime::seeded_from_next_turn_number(next_turn_number);
        let mut active_turn_cancellation: Option<TurnCancellation> = None;
        let mut shutdown_requested = false;

        loop {
            tokio::select! {
                Some(control_event) = control_rx.recv() => {
                    if shutdown_requested {
                        if let ThreadControlEvent::ClaimQueuedJobResult { reply_tx, .. } = control_event {
                            let _ = reply_tx.send(None);
                        }
                        continue;
                    }

                    let runtime_action = match control_event {
                        ThreadControlEvent::UserMessage { content, msg_override } => {
                            // Production: user messages are queued FIFO while a turn is running.
                            let mut mailbox = mailbox.lock().await;
                            runtime.apply_command(
                                ThreadCommand::EnqueueUserMessage {
                                    content,
                                    msg_override,
                                },
                                &mut mailbox,
                            )
                        }
                        ThreadControlEvent::DeliverMailboxMessage(message) => {
                            // Production: mailbox messages are queued FIFO while a turn is running.
                            let mut mailbox = mailbox.lock().await;
                            runtime.apply_command(
                                ThreadCommand::EnqueueMailboxMessage(message),
                                &mut mailbox,
                            )
                        }
                        ThreadControlEvent::UserInterrupt { content } => {
                            // Production: UserInterrupt is an immediate stop signal for the active
                            // turn. The interrupt content is not currently used as redirect text.
                            let _ = content;
                            runtime.cancel_active_turn()
                        }
                        ThreadControlEvent::ClaimQueuedJobResult { job_id, reply_tx } => {
                            let claimed = {
                                let mut mailbox = mailbox.lock().await;
                                runtime.claim_queued_job_result(&mut mailbox, &job_id)
                            };
                            let _ = reply_tx.send(claimed);
                            ThreadRuntimeAction::Noop
                        }
                        ThreadControlEvent::ShutdownRuntime => {
                            shutdown_requested = true;
                            match runtime.state() {
                                ThreadRuntimeState::Idle => break,
                                _ => runtime.cancel_active_turn(),
                            }
                        }
                    };

                    process_runtime_action(
                        Arc::clone(&thread),
                        &mut runtime,
                        &mut active_turn_cancellation,
                        runtime_action,
                        &turn_done_tx,
                    )
                    .await;
                }
                Some(result) = turn_done_rx.recv() => {
                    active_turn_cancellation = None;
                    let settled_turn_number = match runtime.state() {
                        ThreadRuntimeState::Running { turn_number }
                        | ThreadRuntimeState::WaitingForApproval { turn_number }
                        | ThreadRuntimeState::Stopping { turn_number } => Some(turn_number),
                        ThreadRuntimeState::Idle => None,
                    };
                    let finish_result = {
                        let mut guard = thread.write().await;
                        guard.finish_turn(result)
                    };

                    if let Err(error) = finish_result {
                        tracing::error!("turn failed: {}", error);
                    }

                    {
                        let mut guard = mailbox.lock().await;
                        guard.clear_interrupts_for_idle_handoff();
                    }
                    if let Some(turn_number) = settled_turn_number {
                        let thread_id = {
                            let guard = thread.read().await;
                            guard.id().inner().to_string()
                        };
                        let guard = thread.read().await;
                        guard.broadcast_to_self(argus_protocol::ThreadEvent::TurnSettled {
                            thread_id,
                            turn_number,
                        });
                    }

                    if shutdown_requested {
                        break;
                    }

                    let runtime_action = {
                        let mut mailbox = mailbox.lock().await;
                        runtime.finish_active_turn(&mut mailbox)
                    };
                    process_runtime_action(
                        Arc::clone(&thread),
                        &mut runtime,
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
    use crate::compact::{CompactResult, Compactor};
    use crate::error::CompactError;
    use crate::history::TurnRecord;
    use crate::thread::ThreadBuilder;
    use argus_protocol::llm::{ChatMessage, CompletionRequest, CompletionResponse, LlmError, LlmProvider};
    use argus_protocol::{AgentId, MailboxMessage, MailboxMessageType};
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use std::sync::Arc;

    struct DummyProvider;

    #[async_trait]
    impl LlmProvider for DummyProvider {
        fn model_name(&self) -> &str {
            "dummy"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            Err(LlmError::RequestFailed {
                provider: "dummy".to_string(),
                reason: "not implemented".to_string(),
            })
        }
    }

    struct NoopCompactor;

    #[async_trait]
    impl Compactor for NoopCompactor {
        async fn compact(
            &self,
            _provider: &dyn LlmProvider,
            _messages: &[ChatMessage],
            _token_count: u32,
        ) -> Result<Option<CompactResult>, CompactError> {
            Ok(None)
        }

        fn name(&self) -> &'static str {
            "noop"
        }
    }

    fn test_agent_record() -> Arc<argus_protocol::AgentRecord> {
        Arc::new(argus_protocol::AgentRecord {
            id: AgentId::new(1),
            display_name: "Test Agent".to_string(),
            description: "A test agent".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(argus_protocol::ProviderId::new(1)),
            model_id: None,
            system_prompt: "You are a test agent.".to_string(),
            tool_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: None,
            parent_agent_id: None,
            agent_type: argus_protocol::AgentType::Standard,
        })
    }

    fn queued_job_result(job_id: &str) -> MailboxMessage {
        MailboxMessage {
            id: format!("msg-{job_id}"),
            from_thread_id: argus_protocol::ThreadId::new(),
            to_thread_id: argus_protocol::ThreadId::new(),
            from_label: "Worker".to_string(),
            message_type: MailboxMessageType::JobResult {
                job_id: job_id.to_string(),
                success: true,
                token_usage: None,
                agent_id: AgentId::new(7),
                agent_display_name: "Worker".to_string(),
                agent_description: "Background worker".to_string(),
            },
            text: format!("result for {job_id}"),
            timestamp: "2026-04-01T00:00:00Z".to_string(),
            read: false,
            summary: None,
        }
    }

    #[test]
    fn runtime_turn_numbering_starts_after_seeded_turn_count() {
        let mut runtime = ThreadRuntime::seeded_from_next_turn_number(4);
        let mut mailbox = ThreadMailbox::default();
        let action = runtime.apply_command(
            ThreadCommand::EnqueueUserMessage {
                content: "hello".to_string(),
                msg_override: None,
            },
            &mut mailbox,
        );

        assert!(matches!(
            action,
            ThreadRuntimeAction::StartTurn { turn_number: 4, .. }
        ));
    }

    #[test]
    fn runtime_turn_numbering_uses_thread_next_turn_number_when_seeded_history_exists() {
        let thread = ThreadBuilder::new()
            .provider(Arc::new(DummyProvider))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(test_agent_record())
            .session_id(argus_protocol::SessionId::new())
            .turns(vec![TurnRecord::completed(
                4,
                vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
            )])
            .build()
            .unwrap();
        let mut runtime =
            ThreadRuntime::seeded_from_next_turn_number(thread.next_turn_number_for_runtime());
        let mut mailbox = ThreadMailbox::default();
        let action = runtime.apply_command(
            ThreadCommand::EnqueueUserMessage {
                content: "next".to_string(),
                msg_override: None,
            },
            &mut mailbox,
        );

        assert!(matches!(
            action,
            ThreadRuntimeAction::StartTurn { turn_number: 5, .. }
        ));
    }

    #[test]
    fn runtime_cancelled_turn_starts_next_queued_turn_after_finish() {
        let mut runtime = ThreadRuntime::default();
        let mut mailbox = ThreadMailbox::default();

        let first = runtime.apply_command(
            ThreadCommand::EnqueueUserMessage {
                content: "first".to_string(),
                msg_override: None,
            },
            &mut mailbox,
        );
        assert!(matches!(
            first,
            ThreadRuntimeAction::StartTurn { turn_number: 1, .. }
        ));

        let second = runtime.apply_command(
            ThreadCommand::EnqueueUserMessage {
                content: "second".to_string(),
                msg_override: None,
            },
            &mut mailbox,
        );
        assert!(matches!(second, ThreadRuntimeAction::Noop));

        let stop = runtime.cancel_active_turn();
        assert!(matches!(
            stop,
            ThreadRuntimeAction::StopTurn { turn_number: 1 }
        ));
        assert_eq!(
            runtime.state(),
            ThreadRuntimeState::Stopping { turn_number: 1 }
        );

        let next = runtime.finish_active_turn(&mut mailbox);
        assert!(matches!(
            next,
            ThreadRuntimeAction::StartTurn { turn_number: 2, .. }
        ));
    }

    #[test]
    fn claiming_queued_job_result_removes_it_and_preserves_other_work() {
        let mut runtime = ThreadRuntime::default();
        let mut mailbox = ThreadMailbox::default();

        let first = runtime.apply_command(
            ThreadCommand::EnqueueUserMessage {
                content: "first".to_string(),
                msg_override: None,
            },
            &mut mailbox,
        );
        assert!(matches!(
            first,
            ThreadRuntimeAction::StartTurn { turn_number: 1, .. }
        ));

        assert!(matches!(
            runtime.apply_command(
                ThreadCommand::EnqueueMailboxMessage(queued_job_result("job-1")),
                &mut mailbox,
            ),
            ThreadRuntimeAction::Noop
        ));
        assert!(matches!(
            runtime.apply_command(
                ThreadCommand::EnqueueUserMessage {
                    content: "follow-up".to_string(),
                    msg_override: None,
                },
                &mut mailbox,
            ),
            ThreadRuntimeAction::Noop
        ));
        assert_eq!(runtime.snapshot().queue_depth, 2);

        let claimed = runtime.claim_queued_job_result(&mut mailbox, "job-1");
        assert_eq!(
            claimed.as_ref().and_then(MailboxMessage::job_id),
            Some("job-1")
        );
        assert_eq!(runtime.snapshot().queue_depth, 1);

        let next = runtime.finish_active_turn(&mut mailbox);
        assert!(matches!(
            next,
            ThreadRuntimeAction::StartTurn { content, .. } if content == "follow-up"
        ));
    }
}

#[allow(clippy::items_after_test_module)]
async fn process_runtime_action(
    thread: Arc<RwLock<Thread>>,
    runtime: &mut ThreadRuntime,
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
                        let thread_id = {
                            let guard = thread.read().await;
                            guard.id().inner().to_string()
                        };
                        {
                            let guard = thread.read().await;
                            guard.broadcast_to_self(argus_protocol::ThreadEvent::TurnFailed {
                                thread_id: thread_id.clone(),
                                turn_number,
                                error: error.to_string(),
                            });
                        }
                        tracing::error!(
                            turn_number,
                            queue_depth = runtime.snapshot().queue_depth,
                            "failed to start queued turn: {}",
                            error
                        );
                        let mailbox = {
                            let guard = thread.read().await;
                            guard.mailbox()
                        };
                        let mut mailbox = mailbox.lock().await;
                        next_action = runtime.finish_active_turn(&mut mailbox);
                        {
                            let guard = thread.read().await;
                            guard.broadcast_to_self(argus_protocol::ThreadEvent::TurnSettled {
                                thread_id: thread_id.clone(),
                                turn_number,
                            });
                        }
                        if matches!(next_action, ThreadRuntimeAction::Noop) {
                            let guard = thread.read().await;
                            guard
                                .broadcast_to_self(argus_protocol::ThreadEvent::Idle { thread_id });
                        }
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
