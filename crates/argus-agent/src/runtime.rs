//! Thread runtime actor skeleton.

use std::sync::Arc;

use crate::turn::TurnCancellation;
use crate::turn_log_store::persist_turn_log_snapshot;
use argus_protocol::{MessageOverride, ThreadCommand, ThreadControlEvent, ThreadRuntimeState};
use tokio::sync::{RwLock, mpsc};

use crate::error::ThreadError;
use crate::thread::Thread;
pub(crate) use crate::thread::{
    ThreadReactor as ThreadRuntime, ThreadReactorAction as ThreadRuntimeAction,
};

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
        sync_runtime_snapshot(&thread, &runtime).await;
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
                            let action = runtime.apply_command(
                                ThreadCommand::EnqueueUserMessage {
                                    content,
                                    msg_override,
                                },
                                &mut mailbox,
                            );
                            drop(mailbox);
                            sync_runtime_snapshot(&thread, &runtime).await;
                            action
                        }
                        ThreadControlEvent::DeliverMailboxMessage(message) => {
                            // Production: mailbox messages are queued FIFO while a turn is running.
                            let mut mailbox = mailbox.lock().await;
                            let action = runtime.apply_command(
                                ThreadCommand::EnqueueMailboxMessage(message),
                                &mut mailbox,
                            );
                            drop(mailbox);
                            sync_runtime_snapshot(&thread, &runtime).await;
                            action
                        }
                        ThreadControlEvent::UserInterrupt { content } => {
                            // Production: UserInterrupt is an immediate stop signal for the active
                            // turn. The interrupt content is not currently used as redirect text.
                            let _ = content;
                            let mut mailbox = mailbox.lock().await;
                            let action =
                                runtime.apply_command(ThreadCommand::CancelActiveTurn, &mut mailbox);
                            drop(mailbox);
                            sync_runtime_snapshot(&thread, &runtime).await;
                            action
                        }
                        ThreadControlEvent::ClaimQueuedJobResult { job_id, reply_tx } => {
                            let claimed = {
                                let mut mailbox = mailbox.lock().await;
                                runtime.claim_queued_job_result(&mut mailbox, &job_id)
                            };
                            sync_runtime_snapshot(&thread, &runtime).await;
                            let _ = reply_tx.send(claimed);
                            ThreadRuntimeAction::Noop
                        }
                        ThreadControlEvent::ShutdownRuntime => {
                            shutdown_requested = true;
                            match runtime.state() {
                                ThreadRuntimeState::Idle => break,
                                _ => {
                                    let mut mailbox = mailbox.lock().await;
                                    let action = runtime.apply_command(
                                        ThreadCommand::CancelActiveTurn,
                                        &mut mailbox,
                                    );
                                    drop(mailbox);
                                    sync_runtime_snapshot(&thread, &runtime).await;
                                    action
                                }
                            }
                        }
                    };

                    process_runtime_action(
                        Arc::clone(&thread),
                        &mut runtime,
                        runtime_action,
                        &turn_done_tx,
                    )
                    .await;
                }
                Some(result) = turn_done_rx.recv() => {
                    let settled_turn_number = match runtime.state() {
                        ThreadRuntimeState::Running { turn_number }
                        | ThreadRuntimeState::WaitingForApproval { turn_number }
                        | ThreadRuntimeState::Stopping { turn_number } => Some(turn_number),
                        ThreadRuntimeState::Idle => None,
                    };
                    let thread_id = {
                        let guard = thread.read().await;
                        guard.id().inner().to_string()
                    };

                    {
                        let guard = thread.read().await;
                        match &result {
                            Ok(output) => {
                                guard.broadcast_to_self(argus_protocol::ThreadEvent::TurnCompleted {
                                    thread_id: thread_id.clone(),
                                    turn_number: settled_turn_number.unwrap_or_default(),
                                    token_usage: output.token_usage.clone(),
                                });
                            }
                            Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)) => {}
                            Err(error) => {
                                guard.broadcast_to_self(argus_protocol::ThreadEvent::TurnFailed {
                                    thread_id: thread_id.clone(),
                                    turn_number: settled_turn_number.unwrap_or_default(),
                                    error: error.to_string(),
                                });
                            }
                        }
                        guard.broadcast_to_self(argus_protocol::ThreadEvent::Idle {
                            thread_id: thread_id.clone(),
                        });
                    }

                    let (finish_result, turn_log_snapshot) = {
                        let mut guard = thread.write().await;
                        guard.set_active_turn_cancellation(None);
                        let finish_result = guard.finish_turn(result);
                        let turn_log_snapshot = guard.turn_log_persistence_snapshot();
                        (finish_result, turn_log_snapshot)
                    };

                    if let Err(error) = finish_result {
                        tracing::error!("turn failed: {}", error);
                    }

                    if let Some(snapshot) = turn_log_snapshot
                        && let Err(error) = persist_turn_log_snapshot(&snapshot).await
                    {
                        tracing::warn!(
                            turn_number = snapshot.turn.turn_number,
                            error = %error,
                            "failed to persist committed turn log snapshot"
                        );
                    }

                    {
                        let mut guard = mailbox.lock().await;
                        guard.clear_interrupts_for_idle_handoff();
                    }
                    if let Some(turn_number) = settled_turn_number {
                        let guard = thread.read().await;
                        guard.broadcast_to_self(argus_protocol::ThreadEvent::TurnSettled {
                            thread_id: thread_id.clone(),
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
                    sync_runtime_snapshot(&thread, &runtime).await;
                    process_runtime_action(
                        Arc::clone(&thread),
                        &mut runtime,
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
    use argus_protocol::llm::{
        ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmProvider,
    };
    use argus_protocol::{
        AgentId, MailboxMessage, MailboxMessageType, SessionId, ThreadControlEvent, ThreadMailbox,
    };
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use std::sync::Arc;
    use tokio::sync::{RwLock, oneshot};
    use tokio::time::{Duration, sleep, timeout};

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

    struct SlowProvider {
        delay: Duration,
    }

    #[async_trait]
    impl LlmProvider for SlowProvider {
        fn model_name(&self) -> &str {
            "slow"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            sleep(self.delay).await;
            Ok(CompletionResponse {
                content: Some("slow reply".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 10,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }
    }

    fn build_test_thread_with_provider(provider: Arc<dyn LlmProvider>) -> Arc<RwLock<Thread>> {
        Arc::new(RwLock::new(
            ThreadBuilder::new()
                .provider(provider)
                .compactor(Arc::new(NoopCompactor))
                .agent_record(test_agent_record())
                .session_id(SessionId::new())
                .build()
                .expect("thread should build"),
        ))
    }

    async fn wait_for_runtime_queue_depth(thread: &Arc<RwLock<Thread>>, expected_depth: usize) {
        timeout(Duration::from_secs(5), async {
            loop {
                let snapshot = {
                    let guard = thread.read().await;
                    guard.runtime_snapshot()
                };
                if snapshot.queue_depth == expected_depth {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runtime snapshot should reach expected queue depth");
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

        let stop = runtime.apply_command(ThreadCommand::CancelActiveTurn, &mut mailbox);
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

    #[tokio::test]
    async fn runtime_actor_syncs_queue_depth_after_queueing_and_claiming_job_results() {
        let thread = build_test_thread_with_provider(Arc::new(SlowProvider {
            delay: Duration::from_millis(250),
        }));

        Thread::spawn_runtime_actor(Arc::clone(&thread));

        {
            let guard = thread.read().await;
            guard
                .send_user_message("first".to_string(), None)
                .expect("message should queue");
        }

        timeout(Duration::from_secs(5), async {
            loop {
                let state = {
                    let guard = thread.read().await;
                    guard.runtime_snapshot().state
                };
                if matches!(state, ThreadRuntimeState::Running { turn_number: 1 }) {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("thread should start first turn");

        {
            let guard = thread.read().await;
            guard
                .send_control_event(ThreadControlEvent::DeliverMailboxMessage(
                    queued_job_result("job-1"),
                ))
                .expect("job result should queue");
        }

        wait_for_runtime_queue_depth(&thread, 1).await;

        let (reply_tx, reply_rx) = oneshot::channel();
        {
            let guard = thread.read().await;
            guard
                .send_control_event(ThreadControlEvent::ClaimQueuedJobResult {
                    job_id: "job-1".to_string(),
                    reply_tx,
                })
                .expect("claim should queue");
        }

        let claimed = timeout(Duration::from_secs(5), reply_rx)
            .await
            .expect("claim reply should arrive")
            .expect("claim reply channel should stay open");
        assert_eq!(
            claimed.as_ref().and_then(MailboxMessage::job_id),
            Some("job-1")
        );

        wait_for_runtime_queue_depth(&thread, 0).await;
    }

    #[tokio::test]
    async fn failed_start_turn_cleanup_syncs_runtime_snapshot_back_to_thread() {
        let thread = build_test_thread_with_provider(Arc::new(DummyProvider));
        let mut runtime = ThreadRuntime::default();
        let mut mailbox = ThreadMailbox::default();

        let action = runtime.apply_command(
            ThreadCommand::EnqueueUserMessage {
                content: "first".to_string(),
                msg_override: None,
            },
            &mut mailbox,
        );
        assert!(matches!(
            action,
            ThreadRuntimeAction::StartTurn { turn_number: 1, .. }
        ));

        sync_runtime_snapshot(&thread, &runtime).await;

        finish_failed_start_turn(&thread, &mut runtime, 1, "thread-test").await;

        let snapshot = {
            let guard = thread.read().await;
            guard.runtime_snapshot()
        };
        assert_eq!(snapshot.state, ThreadRuntimeState::Idle);
        assert_eq!(snapshot.queue_depth, 0);
    }
}

#[allow(clippy::items_after_test_module)]
async fn process_runtime_action(
    thread: Arc<RwLock<Thread>>,
    runtime: &mut ThreadRuntime,
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
                    Ok(()) => {}
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
                        next_action =
                            finish_failed_start_turn(&thread, runtime, turn_number, &thread_id)
                                .await;
                        continue;
                    }
                }
                break;
            }
            ThreadRuntimeAction::StopTurn { turn_number } => {
                let cancellation = {
                    let guard = thread.read().await;
                    guard.active_turn_cancellation()
                };
                if let Some(cancellation) = cancellation {
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
) -> std::result::Result<(), ThreadError> {
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

    Ok(())
}

async fn finish_failed_start_turn(
    thread: &Arc<RwLock<Thread>>,
    runtime: &mut ThreadRuntime,
    turn_number: u32,
    thread_id: &str,
) -> ThreadRuntimeAction {
    let mailbox = {
        let guard = thread.read().await;
        guard.mailbox()
    };
    let mut mailbox = mailbox.lock().await;
    let next_action = runtime.finish_active_turn(&mut mailbox);
    drop(mailbox);
    sync_runtime_snapshot(thread, runtime).await;

    {
        let guard = thread.read().await;
        guard.broadcast_to_self(argus_protocol::ThreadEvent::TurnSettled {
            thread_id: thread_id.to_string(),
            turn_number,
        });
    }
    if matches!(next_action, ThreadRuntimeAction::Noop) {
        let guard = thread.read().await;
        guard.broadcast_to_self(argus_protocol::ThreadEvent::Idle {
            thread_id: thread_id.to_string(),
        });
    }

    next_action
}

#[allow(clippy::needless_pass_by_value)]
async fn sync_runtime_snapshot(thread: &Arc<RwLock<Thread>>, runtime: &ThreadRuntime) {
    let snapshot = runtime.snapshot();
    let mut guard = thread.write().await;
    guard.sync_runtime_snapshot(snapshot);
}
