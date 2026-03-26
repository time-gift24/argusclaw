//! Thread event types broadcast to subscribers.
//!
//! These events are emitted during thread processing and consumed by subscribers (CLI, Tauri).

use std::collections::VecDeque;

use crate::TokenUsage;
use crate::approval::{ApprovalRequest, ApprovalResponse};
use crate::ids::{AgentId, ThreadId};
use crate::llm::LlmStreamEvent;
use crate::message_override::MessageOverride;

/// Internal control-plane event for thread orchestration.
#[derive(Debug, Clone)]
pub enum ThreadControlEvent {
    /// A new user message entered the thread control plane.
    UserMessage {
        /// Message content.
        content: String,
        /// Optional per-message overrides (temperature, max_tokens, etc.).
        msg_override: Option<MessageOverride>,
    },
    /// A user interrupt that should be surfaced before the next LLM round.
    UserInterrupt {
        /// Interrupt content (e.g. "stop", "cancel", or a new instruction).
        content: String,
    },
    /// A background job produced a result.
    JobResult(ThreadJobResult),
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
    UserMessage { content: String },
    /// Background job result.
    JobResult(ThreadJobResult),
}

impl TurnControlInput {
    /// Render the control item into the synthetic user message text that should
    /// be appended before the next LLM call.
    #[must_use]
    pub fn into_message_text(self) -> String {
        match self {
            Self::UserInterrupt { content } | Self::UserMessage { content } => content,
            Self::JobResult(result) => result.into_message_text(),
        }
    }
}

/// Thread-level mailbox shared between the orchestrator and active turns.
#[derive(Debug, Default)]
pub struct ThreadMailbox {
    user_interrupts: VecDeque<String>,
    queued_user_message: Option<QueuedUserMessage>,
    job_results: VecDeque<ThreadJobResult>,
}

impl ThreadMailbox {
    /// Push a control event into the mailbox.
    pub fn push(&mut self, event: ThreadControlEvent) {
        match event {
            ThreadControlEvent::UserInterrupt { content } => {
                self.user_interrupts.push_back(content);
            }
            ThreadControlEvent::UserMessage {
                content,
                msg_override,
            } => {
                self.queued_user_message = Some(QueuedUserMessage {
                    content,
                    msg_override,
                });
            }
            ThreadControlEvent::JobResult(result) => {
                self.job_results.push_back(result);
            }
        }
    }

    /// Drain items for a running turn.
    ///
    /// Ordering is fixed:
    /// 1. user interrupts (FIFO)
    /// 2. queued user message (single-slot)
    /// 3. job results (FIFO)
    #[must_use]
    pub fn drain_for_turn(&mut self) -> Vec<TurnControlInput> {
        let mut drained = Vec::new();

        while let Some(content) = self.user_interrupts.pop_front() {
            drained.push(TurnControlInput::UserInterrupt { content });
        }

        if let Some(message) = self.queued_user_message.take() {
            drained.push(TurnControlInput::UserMessage {
                content: message.content,
            });
        }

        while let Some(result) = self.job_results.pop_front() {
            drained.push(TurnControlInput::JobResult(result));
        }

        drained
    }

    /// Determine which queued work should start the next idle turn.
    ///
    /// Interrupts are only meaningful while a turn is running, so they are
    /// discarded here before looking at queued user messages and job results.
    pub fn take_next_turn_message(&mut self) -> Option<QueuedUserMessage> {
        self.user_interrupts.clear();

        if let Some(message) = self.queued_user_message.take() {
            return Some(message);
        }

        self.job_results.pop_front().map(|result| QueuedUserMessage {
            content: result.into_message_text(),
            msg_override: None,
        })
    }

    /// Returns true when no pending control items remain.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.user_interrupts.is_empty()
            && self.queued_user_message.is_none()
            && self.job_results.is_empty()
    }
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
    /// User wants to interrupt or redirect the current turn.
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
