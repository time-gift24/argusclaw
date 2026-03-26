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
#[derive(Debug, Clone)]
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
            ThreadControlEvent::UserInterrupt { content } => self.user_interrupts.push_back(content),
            ThreadControlEvent::JobResult(result) => self.inbox.deliver_job_result(result),
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
