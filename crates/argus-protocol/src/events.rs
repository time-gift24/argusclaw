//! Thread event types broadcast to subscribers.
//!
//! These events are emitted during thread processing and consumed by subscribers (CLI, Tauri).

use crate::TokenUsage;
use crate::ids::{AgentId, SessionId, ThreadId};
use crate::llm::LlmStreamEvent;
use crate::mcp::ThreadNoticeLevel;
use crate::message_override::MessageOverride;
use serde::{Deserialize, Serialize};

/// Internal control-plane message for thread orchestration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThreadControlMessage {
    /// Request the runtime actor to stop and release its owned thread state.
    ///
    /// This is an internal control-plane message used by the thread pool when a
    /// chat runtime is unloaded from memory.
    ShutdownRuntime,
}

/// Unified ingress message for thread routing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum ThreadMessage {
    /// Queue user input for the next runtime turn.
    UserInput {
        /// Message content.
        content: String,
        /// Optional per-message overrides (temperature, max_tokens, etc.).
        msg_override: Option<MessageOverride>,
    },
    /// Deliver a peer mailbox message to the thread runtime.
    PeerMessage {
        /// Peer mailbox payload.
        message: MailboxMessage,
    },
    /// Deliver a job-result mailbox message to the thread runtime.
    JobResult {
        /// Job-result payload.
        message: MailboxMessage,
    },
    /// Request cancellation of the currently active turn.
    Interrupt,
    /// Internal control-plane message.
    Control(ThreadControlMessage),
}

impl ThreadMessage {
    /// Returns true when this message participates in normal FIFO payload routing.
    #[must_use]
    pub fn is_fifo_payload(&self) -> bool {
        matches!(
            self,
            Self::UserInput { .. } | Self::PeerMessage { .. } | Self::JobResult { .. }
        )
    }
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
}

/// A queued user message retained by the mailbox.
#[derive(Debug, Clone)]
pub struct QueuedUserMessage {
    /// Message content.
    pub content: String,
    /// Optional per-message overrides (temperature, max_tokens, etc.).
    pub msg_override: Option<MessageOverride>,
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThreadPoolRuntimeKind {
    /// User-facing conversational runtime.
    Chat,
    /// Background job runtime.
    Job,
}

/// Snapshot of a single thread tracked by the thread pool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadPoolRuntimeSummary {
    /// Thread ID.
    pub thread_id: ThreadId,
    /// Runtime category.
    pub kind: ThreadPoolRuntimeKind,
    /// Bound session ID when the runtime belongs to a user chat thread.
    pub session_id: Option<SessionId>,
    /// Bound job ID if this runtime is associated with a dispatched job.
    pub job_id: Option<String>,
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
        /// Thread ID.
        thread_id: ThreadId,
        /// Runtime category.
        kind: ThreadPoolRuntimeKind,
        /// Bound session ID when the runtime belongs to a user chat thread.
        session_id: Option<SessionId>,
        /// Bound job ID if this runtime is associated with a dispatched job.
        job_id: Option<String>,
    },
    /// Job/thread has started running inside the thread pool.
    ThreadPoolStarted {
        /// Thread ID.
        thread_id: ThreadId,
        /// Runtime category.
        kind: ThreadPoolRuntimeKind,
        /// Bound session ID when the runtime belongs to a user chat thread.
        session_id: Option<SessionId>,
        /// Bound job ID if this runtime is associated with a dispatched job.
        job_id: Option<String>,
    },
    /// Job/thread has entered cooling state inside the thread pool.
    ThreadPoolCooling {
        /// Thread ID.
        thread_id: ThreadId,
        /// Runtime category.
        kind: ThreadPoolRuntimeKind,
        /// Bound session ID when the runtime belongs to a user chat thread.
        session_id: Option<SessionId>,
        /// Bound job ID if this runtime is associated with a dispatched job.
        job_id: Option<String>,
    },
    /// Job/thread runtime has been evicted from memory.
    ThreadPoolEvicted {
        /// Thread ID.
        thread_id: ThreadId,
        /// Runtime category.
        kind: ThreadPoolRuntimeKind,
        /// Bound session ID when the runtime belongs to a user chat thread.
        session_id: Option<SessionId>,
        /// Bound job ID if this runtime is associated with a dispatched job.
        job_id: Option<String>,
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
pub(crate) fn assert_thread_pool_state_round_trip() {
    let state = ThreadPoolState {
        snapshot: ThreadPoolSnapshot {
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
        },
        runtimes: vec![ThreadPoolRuntimeSummary {
            thread_id: ThreadId::new(),
            kind: ThreadPoolRuntimeKind::Job,
            session_id: None,
            job_id: Some("job-1".to_string()),
            status: ThreadRuntimeStatus::Queued,
            estimated_memory_bytes: 1024,
            last_active_at: Some("2026-03-29T00:00:01Z".to_string()),
            recoverable: true,
            last_reason: None,
        }],
    };

    let value = serde_json::to_value(&state).unwrap();
    let restored: ThreadPoolState = serde_json::from_value(value).unwrap();
    assert_eq!(restored.runtimes.len(), 1);
    assert_eq!(restored.runtimes[0].kind, ThreadPoolRuntimeKind::Job);
    assert_eq!(restored.runtimes[0].job_id.as_deref(), Some("job-1"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_mailbox_message(text: &str) -> MailboxMessage {
        MailboxMessage {
            id: format!("msg-{text}"),
            from_thread_id: ThreadId::new(),
            to_thread_id: ThreadId::new(),
            from_label: "Peer".to_string(),
            message_type: MailboxMessageType::Plain,
            text: text.to_string(),
            timestamp: "2026-04-01T00:00:00Z".to_string(),
            read: false,
            summary: Some(format!("summary {text}")),
        }
    }

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
    fn thread_message_routes_fifo_payloads() {
        let messages = vec![
            ThreadMessage::UserInput {
                content: "a".into(),
                msg_override: None,
            },
            ThreadMessage::PeerMessage {
                message: plain_mailbox_message("b"),
            },
            ThreadMessage::JobResult {
                message: job_result_message("job-1"),
            },
        ];

        let payloads: Vec<_> = messages
            .iter()
            .filter(|message| message.is_fifo_payload())
            .collect();

        assert_eq!(payloads.len(), 3);
        assert!(matches!(
            payloads[0],
            ThreadMessage::UserInput { content, .. } if content == "a"
        ));
        assert!(matches!(
            payloads[1],
            ThreadMessage::PeerMessage { message } if message.text == "b"
        ));
        assert!(matches!(
            payloads[2],
            ThreadMessage::JobResult { message }
                if message.job_id() == Some("job-1")
        ));
    }

    #[test]
    fn thread_message_interrupt_is_not_part_of_fifo_payload_flow() {
        let messages = vec![
            ThreadMessage::Interrupt,
            ThreadMessage::UserInput {
                content: "after-interrupt".into(),
                msg_override: None,
            },
        ];

        let payloads: Vec<_> = messages
            .iter()
            .filter(|message| message.is_fifo_payload())
            .collect();

        assert!(matches!(messages[0], ThreadMessage::Interrupt));
        assert_eq!(payloads.len(), 1);
        assert!(matches!(
            payloads[0],
            ThreadMessage::UserInput { content, .. } if content == "after-interrupt"
        ));
    }

    #[test]
    fn thread_message_control_wraps_thread_control_message() {
        let control = ThreadMessage::Control(ThreadControlMessage::ShutdownRuntime);

        assert!(!control.is_fifo_payload());
        assert!(matches!(
            control,
            ThreadMessage::Control(ThreadControlMessage::ShutdownRuntime)
        ));
    }

    #[test]
    fn thread_message_serializes_with_stable_snake_case_shape() {
        let value = serde_json::to_value(ThreadMessage::UserInput {
            content: "hello".to_string(),
            msg_override: None,
        })
        .unwrap();

        assert_eq!(
            value,
            serde_json::json!({
                "type": "user_input",
                "payload": {
                    "content": "hello",
                    "msg_override": null
                }
            })
        );
    }

    #[test]
    fn thread_message_control_round_trips_with_explicit_shape() {
        let value = serde_json::json!({
            "type": "control",
            "payload": {
                "type": "shutdown_runtime"
            }
        });

        let message: ThreadMessage = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(
            message,
            ThreadMessage::Control(ThreadControlMessage::ShutdownRuntime)
        ));

        let restored = serde_json::to_value(message).unwrap();
        assert_eq!(restored, value);
    }

    #[test]
    fn thread_pool_snapshot_round_trips_through_json() {
        assert_thread_pool_snapshot_round_trip();
    }
}
