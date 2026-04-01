//! Thread event types broadcast to subscribers.
//!
//! These events are emitted during thread processing and consumed by subscribers (CLI, Tauri).

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::TokenUsage;
use crate::approval::{ApprovalRequest, ApprovalResponse};
use crate::ids::{AgentId, SessionId, ThreadId};
use crate::llm::LlmStreamEvent;
use crate::mcp::ThreadNoticeLevel;
use crate::message_override::MessageOverride;

/// Internal control-plane event for thread orchestration.
#[derive(Debug, Clone)]
pub enum ThreadCommand {
    /// Queue a user message for the runtime inbox.
    EnqueueUserMessage {
        /// Message content.
        content: String,
        /// Optional per-message overrides (temperature, max_tokens, etc.).
        msg_override: Option<MessageOverride>,
    },
    /// Deliver a completed job result back to the thread runtime.
    DeliverJobResult(ThreadJobResult),
    /// Request cancellation of the currently active turn.
    CancelActiveTurn,
}

/// High-level state for the thread runtime actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadRuntimeState {
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

/// Legacy compatibility control event surface for pre-runtime callers.
#[derive(Debug)]
pub enum ThreadControlEvent {
    /// A new user message entered the thread control plane.
    ///
    /// Production semantics: if a turn is currently running, the thread runtime actor
    /// enqueues the message (FIFO) for a subsequent turn. Frontends typically render
    /// this as "queued" work.
    UserMessage {
        /// Message content.
        content: String,
        /// Optional per-message overrides (temperature, max_tokens, etc.).
        msg_override: Option<MessageOverride>,
    },
    /// A user interrupt control signal.
    ///
    /// Production semantics: this is treated as an immediate "stop active turn" signal
    /// (CancelActiveTurn). The `content` is not currently used as redirect text.
    ///
    /// Legacy semantics: callers may still inject interrupts into a running turn via
    /// [`ThreadMailbox`] / [`TurnControlInput`], but this is considered a compatibility
    /// path and not the primary production model.
    UserInterrupt {
        /// Interrupt content (e.g. "stop", "cancel", or a new instruction).
        content: String,
    },
    /// A background job produced a result.
    ///
    /// Production semantics: if a turn is currently running, the thread runtime actor
    /// enqueues the job result (FIFO) for a subsequent turn. Frontends typically render
    /// this as "queued" work.
    JobResult(ThreadJobResult),
    /// Claim a queued job result by job ID so it is not replayed as a future turn.
    ///
    /// This is an internal runtime-actor query path used by `get_job_result(consume=true)`.
    ClaimQueuedJobResult {
        /// Job ID to remove from the runtime inbox.
        job_id: String,
        /// One-shot reply channel containing the removed queued result, if any.
        reply_tx: oneshot::Sender<Option<ThreadJobResult>>,
    },
    /// Request the runtime actor to stop and release its owned thread state.
    ///
    /// This is an internal control-plane event used by the thread pool when a
    /// chat runtime is unloaded from memory.
    ShutdownRuntime,
}

/// Routed job result metadata shared by the control plane and public event stream.
#[derive(Debug, Clone)]
pub struct ThreadJobResult {
    /// Job ID.
    pub job_id: String,
    /// Whether the job succeeded.
    pub success: bool,
    /// Output or error message summary.
    pub message: String,
    /// Token usage if available.
    pub token_usage: Option<TokenUsage>,
    /// Agent ID that handled the subagent work.
    pub agent_id: AgentId,
    /// Human-readable subagent name.
    pub agent_display_name: String,
    /// Subagent description.
    pub agent_description: String,
}

impl ThreadJobResult {
    /// Render this job result as a synthetic user message for the next round.
    #[must_use]
    pub fn into_message_text(&self) -> String {
        format!(
            "Job: {}\nSubagent: {}\nDescription: {}\nResult: {}",
            self.job_id,
            self.agent_display_name,
            if self.agent_description.trim().is_empty() {
                "No description provided."
            } else {
                self.agent_description.as_str()
            },
            self.message
        )
    }
}

/// A queued user message retained by the mailbox.
#[derive(Debug, Clone)]
pub struct QueuedUserMessage {
    /// Message content.
    pub content: String,
    /// Optional per-message overrides (temperature, max_tokens, etc.).
    pub msg_override: Option<MessageOverride>,
}

/// A control item that can be injected into a running turn as a user message.
#[derive(Debug, Clone)]
pub enum TurnControlInput {
    /// User interrupt content.
    UserInterrupt { content: String },
    /// User follow-up content.
    UserMessage {
        /// Message content.
        content: String,
        /// Optional per-message overrides (temperature, max_tokens, etc.).
        msg_override: Option<MessageOverride>,
    },
    /// Background job result.
    JobResult(ThreadJobResult),
}

impl TurnControlInput {
    /// Render the control item into the synthetic user message text that should
    /// be appended before the next LLM call.
    #[must_use]
    pub fn into_message_text(self) -> String {
        match self {
            Self::UserInterrupt { content } => content,
            Self::UserMessage { content, .. } => content,
            Self::JobResult(result) => result.into_message_text(),
        }
    }
}

/// Thread-level inbox shared between the orchestrator and active turns.
#[derive(Debug, Default)]
pub struct ThreadInbox {
    items: VecDeque<ThreadInboxItem>,
}

#[derive(Debug)]
enum ThreadInboxItem {
    UserMessage(QueuedUserMessage),
    JobResult(ThreadJobResult),
}

impl ThreadInbox {
    /// Queue a user message.
    pub fn enqueue_user_message(&mut self, content: String, msg_override: Option<MessageOverride>) {
        self.items
            .push_back(ThreadInboxItem::UserMessage(QueuedUserMessage {
                content,
                msg_override,
            }));
    }

    /// Queue a job result.
    pub fn deliver_job_result(&mut self, result: ThreadJobResult) {
        self.items.push_back(ThreadInboxItem::JobResult(result));
    }

    /// Remove a queued job result by job ID while preserving FIFO order for remaining items.
    pub fn claim_job_result(&mut self, job_id: &str) -> Option<ThreadJobResult> {
        let index = self.items.iter().position(|item| match item {
            ThreadInboxItem::JobResult(result) => result.job_id == job_id,
            ThreadInboxItem::UserMessage(_) => false,
        })?;

        match self.items.remove(index) {
            Some(ThreadInboxItem::JobResult(result)) => Some(result),
            Some(ThreadInboxItem::UserMessage(_)) | None => None,
        }
    }

    /// Drain items for a running turn.
    ///
    /// Ordering is global FIFO across queued user messages and job results.
    #[must_use]
    pub fn drain_for_turn(&mut self) -> Vec<TurnControlInput> {
        let mut drained = Vec::new();

        while let Some(item) = self.items.pop_front() {
            match item {
                ThreadInboxItem::UserMessage(message) => {
                    drained.push(TurnControlInput::UserMessage {
                        content: message.content,
                        msg_override: message.msg_override,
                    });
                }
                ThreadInboxItem::JobResult(result) => {
                    drained.push(TurnControlInput::JobResult(result));
                }
            }
        }

        drained
    }

    /// Determine which queued work should start the next idle turn.
    ///
    /// Ordering is global FIFO across queued user messages and job results.
    pub fn take_next_turn_message(&mut self) -> Option<QueuedUserMessage> {
        match self.items.pop_front() {
            Some(ThreadInboxItem::UserMessage(message)) => Some(message),
            Some(ThreadInboxItem::JobResult(result)) => Some(QueuedUserMessage {
                content: result.into_message_text(),
                msg_override: None,
            }),
            None => None,
        }
    }

    /// Returns true when no pending control items remain.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Legacy mailbox compatibility layer.
///
/// This preserves interrupt behavior for callers still sending
/// [`ThreadControlEvent`] while using [`ThreadInbox`] for queue storage.
///
/// In production, [`ThreadInbox`] is the primary queue model for user messages and
/// job results, coordinated by the thread runtime actor. [`ThreadMailbox`] should be
/// treated as a compatibility surface for legacy interrupt injection, not as the
/// primary queue.
#[derive(Debug, Default)]
pub struct ThreadMailbox {
    user_interrupts: VecDeque<String>,
    inbox: ThreadInbox,
}

impl ThreadMailbox {
    /// Push a legacy control event into the mailbox.
    pub fn push(&mut self, event: ThreadControlEvent) {
        match event {
            ThreadControlEvent::UserMessage {
                content,
                msg_override,
            } => self.inbox.enqueue_user_message(content, msg_override),
            ThreadControlEvent::UserInterrupt { content } => {
                self.user_interrupts.push_back(content)
            }
            ThreadControlEvent::JobResult(result) => self.inbox.deliver_job_result(result),
            ThreadControlEvent::ClaimQueuedJobResult { reply_tx, .. } => {
                let _ = reply_tx.send(None);
            }
            ThreadControlEvent::ShutdownRuntime => {}
        }
    }

    /// Drain control inputs for a running turn.
    ///
    /// Legacy interrupt behavior is preserved by draining interrupts first.
    #[must_use]
    pub fn drain_for_turn(&mut self) -> Vec<TurnControlInput> {
        let mut drained = Vec::new();

        while let Some(content) = self.user_interrupts.pop_front() {
            drained.push(TurnControlInput::UserInterrupt { content });
        }

        drained.extend(self.inbox.drain_for_turn());
        drained
    }

    /// Determine which queued work should start the next idle turn.
    ///
    /// Interrupts are cleared on idle handoff, matching legacy behavior.
    pub fn take_next_turn_message(&mut self) -> Option<QueuedUserMessage> {
        self.user_interrupts.clear();
        self.inbox.take_next_turn_message()
    }

    /// Clear legacy interrupts without disturbing queued inbox work.
    ///
    /// This matches the legacy "idle handoff" behavior where any interrupts
    /// that arrived for a now-completed turn are discarded before the next turn
    /// begins.
    pub fn clear_interrupts_for_idle_handoff(&mut self) {
        self.user_interrupts.clear();
    }

    /// Returns true when no pending control items remain.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.user_interrupts.is_empty() && self.inbox.is_empty()
    }
}

/// Snapshot of a single thread tracked by the thread pool.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThreadRuntimeStatus {
    /// Runtime has not been loaded into memory.
    Inactive,
    /// Runtime is being loaded.
    Loading,
    /// Runtime is queued for execution.
    Queued,
    /// Runtime is actively executing.
    Running,
    /// Runtime completed recently and is in cooling period.
    Cooling,
    /// Runtime was evicted from memory.
    Evicted,
}

/// Runtime source classification inside the unified thread pool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThreadPoolRuntimeKind {
    /// User-facing conversational runtime.
    Chat,
    /// Background job runtime.
    Job,
}

/// Stable identifier for a runtime tracked by the unified thread pool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadPoolRuntimeRef {
    /// Thread ID.
    pub thread_id: ThreadId,
    /// Runtime category.
    pub kind: ThreadPoolRuntimeKind,
    /// Bound session ID when the runtime belongs to a user chat thread.
    pub session_id: Option<SessionId>,
    /// Bound job ID if this runtime is associated with a dispatched job.
    pub job_id: Option<String>,
}

/// Snapshot of a single thread tracked by the thread pool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadPoolRuntimeSummary {
    /// Stable runtime identity.
    pub runtime: ThreadPoolRuntimeRef,
    /// Runtime status.
    pub status: ThreadRuntimeStatus,
    /// Estimated memory usage for this runtime.
    pub estimated_memory_bytes: u64,
    /// Last activity timestamp (RFC3339).
    pub last_active_at: Option<String>,
    /// Whether the runtime can be reloaded after in-memory eviction.
    pub recoverable: bool,
    /// Last eviction/cooling reason when available.
    pub last_reason: Option<ThreadPoolEventReason>,
}

/// Backward-compatible alias for older thread-pool consumers.
pub type ThreadRuntimeSnapshot = ThreadPoolRuntimeSummary;

/// Aggregated thread-pool telemetry snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadPoolSnapshot {
    /// Configured max number of concurrently loaded runtimes.
    pub max_threads: u32,
    /// Total active (loaded) runtimes.
    pub active_threads: u32,
    /// Runtimes waiting in the queue.
    pub queued_threads: u32,
    /// Runtimes currently executing.
    pub running_threads: u32,
    /// Runtimes in cooling state.
    pub cooling_threads: u32,
    /// Number of evictions observed since process start.
    pub evicted_threads: u64,
    /// Estimated pool memory usage.
    pub estimated_memory_bytes: u64,
    /// Peak estimated pool memory usage.
    pub peak_estimated_memory_bytes: u64,
    /// Process-level memory usage when available.
    pub process_memory_bytes: Option<u64>,
    /// Peak process-level memory usage when available.
    pub peak_process_memory_bytes: Option<u64>,
    /// Number of currently resident runtime records.
    pub resident_thread_count: u32,
    /// Average estimated memory usage per resident runtime.
    pub avg_thread_memory_bytes: u64,
    /// Snapshot timestamp (RFC3339).
    pub captured_at: String,
}

/// Authoritative thread-pool query payload used by external observers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadPoolState {
    /// Aggregated pool metrics.
    pub snapshot: ThreadPoolSnapshot,
    /// Current runtime summaries.
    pub runtimes: Vec<ThreadPoolRuntimeSummary>,
}

/// Reason associated with thread-pool lifecycle transitions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThreadPoolEventReason {
    /// Runtime naturally cooled down and aged out.
    CoolingExpired,
    /// Runtime was evicted due to memory pressure.
    MemoryPressure,
    /// Runtime was cancelled explicitly.
    Cancelled,
    /// Runtime failed while executing.
    ExecutionFailed,
}

/// Thread event broadcast to subscribers (CLI, Tauri).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadEvent {
    /// Turn is processing, streaming LLM/tool events.
    Processing {
        /// Thread ID.
        thread_id: String,
        /// Turn number.
        turn_number: u32,
        /// LLM stream event.
        event: LlmStreamEvent,
    },
    /// Tool execution started.
    ToolStarted {
        /// Thread ID.
        thread_id: String,
        /// Turn number.
        turn_number: u32,
        /// Tool call ID.
        tool_call_id: String,
        /// Tool name.
        tool_name: String,
        /// Tool arguments.
        arguments: serde_json::Value,
    },
    /// Tool execution completed.
    ToolCompleted {
        /// Thread ID.
        thread_id: String,
        /// Turn number.
        turn_number: u32,
        /// Tool call ID.
        tool_call_id: String,
        /// Tool name.
        tool_name: String,
        /// Tool result (Ok for success, Err for failure).
        result: Result<serde_json::Value, String>,
    },
    /// Turn completed successfully.
    TurnCompleted {
        /// Thread ID.
        thread_id: String,
        /// Turn number.
        turn_number: u32,
        /// Token usage for this turn.
        token_usage: TokenUsage,
    },
    /// Turn failed.
    TurnFailed {
        /// Thread ID.
        thread_id: String,
        /// Turn number.
        turn_number: u32,
        /// Error message.
        error: String,
    },
    /// Thread entered idle state.
    Idle {
        /// Thread ID.
        thread_id: String,
    },
    /// Non-fatal notice for the UI and logs.
    Notice {
        /// Thread ID.
        thread_id: String,
        /// Notice level.
        level: ThreadNoticeLevel,
        /// Human-readable message.
        message: String,
    },
    /// Context was compacted.
    Compacted {
        /// Thread ID.
        thread_id: String,
        /// New token count after compaction.
        new_token_count: u32,
    },
    /// Hidden compact-agent flow started.
    CompactionStarted {
        /// Thread ID.
        thread_id: String,
    },
    /// Hidden compact-agent flow finished successfully.
    CompactionFinished {
        /// Thread ID.
        thread_id: String,
    },
    /// Hidden compact-agent flow failed but the visible turn may continue.
    CompactionFailed {
        /// Thread ID.
        thread_id: String,
        /// Error summary.
        error: String,
    },
    /// Waiting for approval - tool execution paused for human confirmation.
    WaitingForApproval {
        /// Thread ID.
        thread_id: String,
        /// Turn number.
        turn_number: u32,
        /// The approval request.
        request: ApprovalRequest,
    },
    /// Approval was resolved.
    ApprovalResolved {
        /// Thread ID.
        thread_id: String,
        /// Turn number.
        turn_number: u32,
        /// The approval response.
        response: ApprovalResponse,
    },
    /// A job was dispatched by the dispatch_job tool.
    JobDispatched {
        /// Thread ID of the originating thread (for routing to the correct session).
        thread_id: ThreadId,
        /// Job ID.
        job_id: String,
        /// Agent ID for this job.
        agent_id: AgentId,
        /// Prompt/task description for the job.
        prompt: String,
        /// Optional context JSON for the job.
        context: Option<serde_json::Value>,
    },
    /// A dispatched job produced a result.
    JobResult {
        /// Thread ID of the originating thread (for routing to the correct session).
        thread_id: ThreadId,
        /// Job ID.
        job_id: String,
        /// Whether the job succeeded.
        success: bool,
        /// Output or error message.
        message: String,
        /// Token usage if available.
        token_usage: Option<TokenUsage>,
        /// Agent ID that handled the subagent work.
        agent_id: AgentId,
        /// Human-readable subagent name.
        agent_display_name: String,
        /// Subagent description.
        agent_description: String,
    },
    /// Job has been bound to a concrete thread runtime.
    ThreadBoundToJob {
        /// Job ID.
        job_id: String,
        /// Thread ID.
        thread_id: ThreadId,
    },
    /// Job/thread has entered queued state inside the thread pool.
    ThreadPoolQueued {
        /// Runtime reference.
        runtime: ThreadPoolRuntimeRef,
    },
    /// Job/thread has started running inside the thread pool.
    ThreadPoolStarted {
        /// Runtime reference.
        runtime: ThreadPoolRuntimeRef,
    },
    /// Job/thread has entered cooling state inside the thread pool.
    ThreadPoolCooling {
        /// Runtime reference.
        runtime: ThreadPoolRuntimeRef,
    },
    /// Job/thread runtime has been evicted from memory.
    ThreadPoolEvicted {
        /// Runtime reference.
        runtime: ThreadPoolRuntimeRef,
        /// Eviction reason.
        reason: ThreadPoolEventReason,
    },
    /// Aggregated thread-pool metrics update.
    ThreadPoolMetricsUpdated {
        /// Current thread-pool telemetry snapshot.
        snapshot: ThreadPoolSnapshot,
    },
    /// User wants to interrupt the current turn.
    ///
    /// Production semantics: this is an immediate stop signal. Redirect-style
    /// reuse of `content` is not currently supported on the runtime path.
    UserInterrupt {
        /// Interrupt content (e.g. "stop", "cancel", or a new instruction).
        content: String,
    },
    /// A new user message to process.
    UserMessage {
        /// Message content.
        content: String,
        /// Optional per-message overrides (temperature, max_tokens, etc.).
        msg_override: Option<MessageOverride>,
    },
}

#[cfg(test)]
pub(crate) fn assert_thread_pool_snapshot_round_trip() {
    let snapshot = ThreadPoolSnapshot {
        max_threads: 8,
        active_threads: 2,
        queued_threads: 1,
        running_threads: 1,
        cooling_threads: 1,
        evicted_threads: 3,
        estimated_memory_bytes: 4096,
        peak_estimated_memory_bytes: 8192,
        process_memory_bytes: Some(16_384),
        peak_process_memory_bytes: Some(32_768),
        resident_thread_count: 2,
        avg_thread_memory_bytes: 2048,
        captured_at: "2026-03-29T00:00:00Z".to_string(),
    };

    let value = serde_json::to_value(&snapshot).unwrap();
    let restored: ThreadPoolSnapshot = serde_json::from_value(value).unwrap();
    assert_eq!(restored.max_threads, 8);
    assert_eq!(restored.queued_threads, 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job_result(job_id: &str) -> ThreadJobResult {
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
    fn thread_inbox_claim_job_result_removes_only_matching_job() {
        let mut inbox = ThreadInbox::default();
        inbox.enqueue_user_message("first".to_string(), None);
        inbox.deliver_job_result(job_result("job-1"));
        inbox.deliver_job_result(job_result("job-2"));

        let claimed = inbox.claim_job_result("job-1");
        assert_eq!(
            claimed.as_ref().map(|result| result.job_id.as_str()),
            Some("job-1")
        );

        let remaining = inbox.drain_for_turn();
        assert_eq!(remaining.len(), 2);
        assert!(matches!(
            &remaining[0],
            TurnControlInput::UserMessage { content, .. } if content == "first"
        ));
        assert!(matches!(
            &remaining[1],
            TurnControlInput::JobResult(result) if result.job_id == "job-2"
        ));
    }

    #[test]
    fn thread_pool_snapshot_round_trips_through_json() {
        assert_thread_pool_snapshot_round_trip();
    }
}
