//! Thread event types broadcast to subscribers.
//!
//! These events are emitted during thread processing and consumed by subscribers (CLI, Tauri).

use std::collections::VecDeque;

use crate::TokenUsage;
use crate::approval::{ApprovalRequest, ApprovalResponse};
use crate::ids::{AgentId, SessionId, ThreadId};
use crate::llm::LlmStreamEvent;
use crate::message_override::MessageOverride;
use serde::{Deserialize, Serialize};

/// Internal control-plane event for thread orchestration.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum ThreadCommand {
    /// Queue a user message for the runtime inbox.
    EnqueueUserMessage {
        /// Message content.
        content: String,
        /// Optional per-message overrides (temperature, max_tokens, etc.).
        msg_override: Option<MessageOverride>,
    },
    /// Deliver a mailbox message back to the thread runtime.
    EnqueueMailboxMessage(MailboxMessage),
    /// Request cancellation of the currently active turn.
    CancelActiveTurn,
}

/// Internal control-plane event used to wake or shut down a thread runtime.
#[derive(Debug)]
pub enum ThreadControlEvent {
    /// Wake the runtime to inspect its mailbox state.
    MailboxUpdated,
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

/// Mailbox message type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MailboxMessageType {
    /// Plain text message.
    Plain,
    /// Job completion result.
    JobResult {
        job_id: String,
        success: bool,
        token_usage: Option<TokenUsage>,
        agent_id: AgentId,
        agent_display_name: String,
        agent_description: String,
    },
    /// Structured task assignment.
    TaskAssignment {
        task_id: String,
        subject: String,
        description: String,
    },
}

impl MailboxMessageType {
    #[must_use]
    pub fn job_id(&self) -> Option<&str> {
        match self {
            Self::JobResult { job_id, .. } => Some(job_id.as_str()),
            Self::Plain | Self::TaskAssignment { .. } => None,
        }
    }

    #[must_use]
    pub fn into_message_text(self, text: String) -> String {
        match self {
            Self::Plain => text,
            Self::JobResult {
                job_id,
                agent_display_name,
                agent_description,
                ..
            } => format!(
                "Job: {}\nSubagent: {}\nDescription: {}\nResult: {}",
                job_id,
                agent_display_name,
                if agent_description.trim().is_empty() {
                    "No description provided."
                } else {
                    agent_description.as_str()
                },
                text
            ),
            Self::TaskAssignment {
                task_id,
                subject,
                description,
            } => format!(
                "Task: {}\nSubject: {}\nDescription: {}\nMessage: {}",
                task_id, subject, description, text
            ),
        }
    }
}

/// Unified mailbox message for cross-thread communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxMessage {
    pub id: String,
    pub from_thread_id: ThreadId,
    pub to_thread_id: ThreadId,
    pub from_label: String,
    pub message_type: MailboxMessageType,
    pub text: String,
    pub timestamp: String,
    pub read: bool,
    pub summary: Option<String>,
}

impl MailboxMessage {
    #[must_use]
    pub fn job_id(&self) -> Option<&str> {
        self.message_type.job_id()
    }

    #[must_use]
    pub fn into_message_text(self) -> String {
        self.message_type.into_message_text(self.text)
    }

    #[must_use]
    pub fn into_queued_user_message(self) -> QueuedUserMessage {
        QueuedUserMessage {
            content: self.into_message_text(),
            msg_override: None,
        }
    }

    pub fn mark_read(&mut self) {
        self.read = true;
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

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum ThreadMailboxItem {
    UserMessage(QueuedUserMessage),
    MailboxMessage(MailboxMessage),
}

/// Thread-level mailbox for queued user messages and mailbox messages.
#[derive(Debug, Default)]
pub struct ThreadMailbox {
    items: VecDeque<ThreadMailboxItem>,
    stop_requested: bool,
}

impl ThreadMailbox {
    /// Queue a user message.
    pub fn enqueue_user_message(&mut self, content: String, msg_override: Option<MessageOverride>) {
        self.items
            .push_back(ThreadMailboxItem::UserMessage(QueuedUserMessage {
                content,
                msg_override,
            }));
    }

    /// Queue a mailbox message.
    pub fn enqueue_mailbox_message(&mut self, message: MailboxMessage) {
        self.items
            .push_back(ThreadMailboxItem::MailboxMessage(message));
    }

    /// Request that the current active turn stop.
    pub fn interrupt_stop(&mut self) {
        self.stop_requested = true;
    }

    /// Take the pending stop request, if any.
    pub fn take_stop_signal(&mut self) -> bool {
        std::mem::take(&mut self.stop_requested)
    }

    /// Remove a queued job result by job ID while preserving FIFO order for remaining items.
    pub fn claim_job_result(&mut self, job_id: &str) -> Option<MailboxMessage> {
        let index = self.items.iter().position(|item| match item {
            ThreadMailboxItem::MailboxMessage(message) => message.job_id() == Some(job_id),
            ThreadMailboxItem::UserMessage(_) => false,
        })?;

        match self.items.remove(index) {
            Some(ThreadMailboxItem::MailboxMessage(message)) => Some(message),
            Some(ThreadMailboxItem::UserMessage(_)) | None => None,
        }
    }

    /// Determine which queued work should start the next idle turn.
    #[must_use]
    pub fn take_next_turn_message(&mut self) -> Option<QueuedUserMessage> {
        match self.items.pop_front() {
            Some(ThreadMailboxItem::UserMessage(message)) => Some(message),
            Some(ThreadMailboxItem::MailboxMessage(message)) => {
                Some(message.into_queued_user_message())
            }
            None => None,
        }
    }

    /// Return the number of pending mailbox items, including a pending stop request.
    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.items.len() + self.stop_requested as usize
    }

    /// Returns true when no pending control items remain.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.stop_requested && self.items.is_empty()
    }

    /// Return unread mailbox messages that remain queued.
    #[must_use]
    pub fn unread_mailbox_messages(&self) -> Vec<MailboxMessage> {
        self.items
            .iter()
            .filter_map(|item| match item {
                ThreadMailboxItem::MailboxMessage(message) if !message.read => {
                    Some(message.clone())
                }
                ThreadMailboxItem::UserMessage(_) | ThreadMailboxItem::MailboxMessage(_) => None,
            })
            .collect()
    }

    /// Mark a queued mailbox message as read by message ID.
    pub fn mark_mailbox_message_read(&mut self, message_id: &str) -> bool {
        for item in &mut self.items {
            if let ThreadMailboxItem::MailboxMessage(message) = item
                && message.id == message_id
            {
                message.mark_read();
                return true;
            }
        }

        false
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
#[derive(Debug, Clone)]
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
    /// Turn output has been applied to authoritative thread state.
    TurnSettled {
        /// Thread ID.
        thread_id: String,
        /// Turn number.
        turn_number: u32,
    },
    /// Thread entered idle state.
    Idle {
        /// Thread ID.
        thread_id: String,
    },
    /// Context was compacted.
    Compacted {
        /// Thread ID.
        thread_id: String,
        /// New token count after compaction.
        new_token_count: u32,
    },
    /// Hidden compaction flow started.
    CompactionStarted {
        /// Thread ID.
        thread_id: String,
    },
    /// Hidden compaction flow finished successfully.
    CompactionFinished {
        /// Thread ID.
        thread_id: String,
    },
    /// Hidden compaction flow failed but the visible turn may continue.
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
    /// A mailbox message was queued for a thread.
    MailboxMessageQueued {
        /// Thread ID that received the message.
        thread_id: ThreadId,
        /// Queued mailbox message.
        message: MailboxMessage,
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

    fn job_result_message(job_id: &str) -> MailboxMessage {
        MailboxMessage {
            id: format!("msg-{job_id}"),
            from_thread_id: ThreadId::new(),
            to_thread_id: ThreadId::new(),
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
            summary: Some(format!("summary {job_id}")),
        }
    }

    #[test]
    fn mailbox_job_result_renders_as_synthetic_message_text() {
        let rendered = job_result_message("job-render").into_message_text();

        assert!(rendered.contains("Job: job-render"));
        assert!(rendered.contains("Subagent: Worker"));
        assert!(rendered.contains("Description: Background worker"));
        assert!(rendered.contains("Result: result for job-render"));
    }

    #[test]
    fn thread_mailbox_take_next_turn_message_preserves_global_fifo() {
        let mut mailbox = ThreadMailbox::default();
        mailbox.enqueue_user_message("first".to_string(), None);
        mailbox.enqueue_mailbox_message(job_result_message("job-1"));

        let first = mailbox
            .take_next_turn_message()
            .expect("first queued message should exist");
        let second = mailbox
            .take_next_turn_message()
            .expect("second queued message should exist");

        assert_eq!(first.content, "first");
        assert!(second.content.contains("Job: job-1"));
    }

    #[test]
    fn thread_mailbox_claim_job_result_preserves_remaining_fifo_order() {
        let mut mailbox = ThreadMailbox::default();
        mailbox.enqueue_user_message("first".to_string(), None);
        mailbox.enqueue_mailbox_message(job_result_message("job-1"));
        mailbox.enqueue_mailbox_message(job_result_message("job-2"));

        let claimed = mailbox.claim_job_result("job-1");
        assert_eq!(
            claimed.as_ref().and_then(MailboxMessage::job_id),
            Some("job-1")
        );

        let next = mailbox
            .take_next_turn_message()
            .expect("remaining user message should stay at the head of the queue");
        let final_message = mailbox
            .take_next_turn_message()
            .expect("remaining job result should preserve FIFO order");

        assert_eq!(next.content, "first");
        assert!(final_message.content.contains("Job: job-2"));
    }

    #[test]
    fn thread_mailbox_messages_remain_unread_until_marked_read() {
        let mut mailbox = ThreadMailbox::default();
        let message = job_result_message("job-unread");
        mailbox.enqueue_mailbox_message(message.clone());

        let unread = mailbox.unread_mailbox_messages();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].id, message.id);

        assert!(mailbox.mark_mailbox_message_read(&message.id));
        assert!(
            mailbox.unread_mailbox_messages().is_empty(),
            "queued mailbox messages should remain unread until mark_read is called"
        );
    }

    #[test]
    fn thread_mailbox_interrupt_stop_is_not_enqueued() {
        let mut mailbox = ThreadMailbox::default();
        mailbox.interrupt_stop();

        assert!(mailbox.take_next_turn_message().is_none());
        assert!(mailbox.take_stop_signal());
        assert!(!mailbox.take_stop_signal());
    }

    #[test]
    fn thread_pool_snapshot_round_trips_through_json() {
        assert_thread_pool_snapshot_round_trip();
    }
}
