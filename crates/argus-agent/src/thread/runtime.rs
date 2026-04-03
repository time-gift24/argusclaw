use std::sync::Arc;

use tokio::sync::RwLock;

use crate::TurnOutput;
use crate::error::ThreadError;
use crate::turn::{TurnCancellation, TurnExecution, TurnProgress};
use crate::turn_log_store::persist_turn_log_snapshot;
use argus_protocol::{
    MessageOverride, ThreadCommand, ThreadControlEvent, ThreadEvent, ThreadRuntimeState,
};

use super::{Thread, ThreadReactor, ThreadReactorAction};

impl Thread {
    /// Spawn the thread-owned reactor loop that coordinates queued control events.
    pub fn spawn_reactor(thread: Arc<RwLock<Self>>) {
        tokio::spawn(async move {
            Self::run_reactor_loop(thread).await;
        });
    }

    async fn run_reactor_loop(thread: Arc<RwLock<Self>>) {
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

        let mut runtime = ThreadReactor::seeded_from_next_turn_number(next_turn_number);
        let mut active_turn: Option<TurnExecution> = None;
        Self::sync_runtime_snapshot_async(&thread, &runtime).await;
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
                            let mut mailbox = mailbox.lock().await;
                            let action = runtime.apply_command(
                                ThreadCommand::EnqueueUserMessage {
                                    content,
                                    msg_override,
                                },
                                &mut mailbox,
                            );
                            drop(mailbox);
                            Self::sync_runtime_snapshot_async(&thread, &runtime).await;
                            action
                        }
                        ThreadControlEvent::DeliverMailboxMessage(message) => {
                            let mut mailbox = mailbox.lock().await;
                            let action = runtime.apply_command(
                                ThreadCommand::EnqueueMailboxMessage(message),
                                &mut mailbox,
                            );
                            drop(mailbox);
                            Self::sync_runtime_snapshot_async(&thread, &runtime).await;
                            action
                        }
                        ThreadControlEvent::UserInterrupt { content } => {
                            let _ = content;
                            let mut mailbox = mailbox.lock().await;
                            let action =
                                runtime.apply_command(ThreadCommand::CancelActiveTurn, &mut mailbox);
                            drop(mailbox);
                            Self::sync_runtime_snapshot_async(&thread, &runtime).await;
                            action
                        }
                        ThreadControlEvent::ClaimQueuedJobResult { job_id, reply_tx } => {
                            let claimed = {
                                let mut mailbox = mailbox.lock().await;
                                runtime.claim_queued_job_result(&mut mailbox, &job_id)
                            };
                            Self::sync_runtime_snapshot_async(&thread, &runtime).await;
                            let _ = reply_tx.send(claimed);
                            ThreadReactorAction::Noop
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
                                    Self::sync_runtime_snapshot_async(&thread, &runtime).await;
                                    action
                                }
                            }
                        }
                    };

                    Self::process_reactor_action(
                        Arc::clone(&thread),
                        &mut runtime,
                        runtime_action,
                        &mut active_turn,
                    )
                    .await;
                }
                progress = async {
                    match active_turn.as_mut() {
                        Some(execution) => execution.recv().await,
                        None => None,
                    }
                }, if active_turn.is_some() => {
                    match progress {
                        Some(progress) => {
                            Self::handle_turn_progress(&thread, &mut runtime, &progress).await;
                        }
                        None => {
                            let result = active_turn
                                .take()
                                .expect("active turn should exist while receiving progress")
                                .finish()
                                .await
                                .map_err(ThreadError::TurnFailed);

                            Self::settle_active_turn(
                                &thread,
                                &mut runtime,
                                result,
                                &mut active_turn,
                                shutdown_requested,
                            )
                            .await;

                            if shutdown_requested && active_turn.is_none() {
                                break;
                            }
                        }
                    }
                }
                else => break,
            }
        }
    }

    pub(super) async fn process_reactor_action(
        thread: Arc<RwLock<Self>>,
        runtime: &mut ThreadReactor,
        action: ThreadReactorAction,
        active_turn: &mut Option<TurnExecution>,
    ) {
        let mut next_action = action;
        loop {
            match next_action {
                ThreadReactorAction::StartTurn {
                    turn_number,
                    content,
                    msg_override,
                } => match Self::start_turn_execution(
                    Arc::clone(&thread),
                    turn_number,
                    content,
                    msg_override,
                )
                .await
                {
                    Ok(execution) => {
                        *active_turn = Some(execution);
                    }
                    Err(error) => {
                        let thread_id = {
                            let guard = thread.read().await;
                            guard.id().inner().to_string()
                        };
                        {
                            let guard = thread.read().await;
                            guard.broadcast_to_self(ThreadEvent::TurnFailed {
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
                        next_action = Self::finish_failed_start_turn(
                            &thread,
                            runtime,
                            turn_number,
                            &thread_id,
                        )
                        .await;
                        continue;
                    }
                },
                ThreadReactorAction::StopTurn { turn_number } => {
                    let cancellation = {
                        let guard = thread.read().await;
                        guard.active_turn_cancellation()
                    };
                    if let Some(cancellation) = cancellation {
                        tracing::info!(turn_number, "cancelling active turn");
                        cancellation.cancel();
                    } else {
                        tracing::warn!(
                            turn_number,
                            "stop-turn requested but no active turn handle"
                        );
                    }
                }
                ThreadReactorAction::Noop => {}
            }
            break;
        }
    }

    async fn start_turn_execution(
        thread: Arc<RwLock<Self>>,
        turn_number: u32,
        content: String,
        msg_override: Option<MessageOverride>,
    ) -> Result<TurnExecution, ThreadError> {
        let cancellation = TurnCancellation::new();
        let turn = {
            let mut guard = thread.write().await;
            guard
                .begin_turn_with_number(turn_number, content, msg_override, cancellation.clone())
                .await?
        };

        Ok(turn.execute_progress())
    }

    async fn finish_failed_start_turn(
        thread: &Arc<RwLock<Self>>,
        runtime: &mut ThreadReactor,
        turn_number: u32,
        thread_id: &str,
    ) -> ThreadReactorAction {
        let mailbox = {
            let guard = thread.read().await;
            guard.mailbox()
        };
        let mut mailbox = mailbox.lock().await;
        let next_action = runtime.finish_active_turn(&mut mailbox);
        drop(mailbox);
        Self::sync_runtime_snapshot_async(thread, runtime).await;

        {
            let guard = thread.read().await;
            guard.broadcast_to_self(ThreadEvent::TurnSettled {
                thread_id: thread_id.to_string(),
                turn_number,
            });
        }
        if matches!(next_action, ThreadReactorAction::Noop) {
            let guard = thread.read().await;
            guard.broadcast_to_self(ThreadEvent::Idle {
                thread_id: thread_id.to_string(),
            });
        }

        next_action
    }

    async fn handle_turn_progress(
        thread: &Arc<RwLock<Self>>,
        runtime: &mut ThreadReactor,
        progress: &TurnProgress,
    ) {
        match progress {
            TurnProgress::WaitingForApproval { turn_number, .. } => {
                runtime.mark_waiting_for_approval(*turn_number);
                Self::sync_runtime_snapshot_async(thread, runtime).await;
            }
            TurnProgress::ApprovalResolved { turn_number, .. } => {
                runtime.mark_running_after_approval(*turn_number);
                Self::sync_runtime_snapshot_async(thread, runtime).await;
            }
            TurnProgress::LlmEvent(_)
            | TurnProgress::ToolStarted { .. }
            | TurnProgress::ToolCompleted { .. }
            | TurnProgress::Completed(_)
            | TurnProgress::Failed { .. } => {}
        }
    }

    async fn settle_active_turn(
        thread: &Arc<RwLock<Self>>,
        runtime: &mut ThreadReactor,
        result: Result<TurnOutput, ThreadError>,
        active_turn: &mut Option<TurnExecution>,
        shutdown_requested: bool,
    ) {
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
                    guard.broadcast_to_self(ThreadEvent::TurnCompleted {
                        thread_id: thread_id.clone(),
                        turn_number: settled_turn_number.unwrap_or_default(),
                        token_usage: output.token_usage.clone(),
                    });
                }
                Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)) => {}
                Err(error) => {
                    guard.broadcast_to_self(ThreadEvent::TurnFailed {
                        thread_id: thread_id.clone(),
                        turn_number: settled_turn_number.unwrap_or_default(),
                        error: error.to_string(),
                    });
                }
            }
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
            let mailbox = {
                let guard = thread.read().await;
                guard.mailbox()
            };
            let mut guard = mailbox.lock().await;
            guard.clear_interrupts_for_idle_handoff();
        }

        if let Some(turn_number) = settled_turn_number {
            let guard = thread.read().await;
            guard.broadcast_to_self(ThreadEvent::TurnSettled {
                thread_id: thread_id.clone(),
                turn_number,
            });
        }

        if shutdown_requested {
            return;
        }

        let runtime_action = {
            let mailbox = {
                let guard = thread.read().await;
                guard.mailbox()
            };
            let mut guard = mailbox.lock().await;
            runtime.finish_active_turn(&mut guard)
        };
        Self::sync_runtime_snapshot_async(thread, runtime).await;
        Self::process_reactor_action(
            Arc::clone(thread),
            runtime,
            runtime_action.clone(),
            active_turn,
        )
        .await;

        if matches!(runtime_action, ThreadReactorAction::Noop) && active_turn.is_none() {
            let guard = thread.read().await;
            guard.broadcast_to_self(ThreadEvent::Idle { thread_id });
        }
    }

    async fn sync_runtime_snapshot_async(thread: &Arc<RwLock<Self>>, runtime: &ThreadReactor) {
        let snapshot = runtime.snapshot();
        let mut guard = thread.write().await;
        guard.sync_runtime_snapshot(snapshot);
    }
}
