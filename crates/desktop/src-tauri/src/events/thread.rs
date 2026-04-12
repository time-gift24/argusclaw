//! Thread event types for frontend communication.
//!
//! This module provides envelope and payload types for forwarding thread events
//! from the backend to the frontend via Tauri events.

use serde::{Deserialize, Serialize};

use argus_protocol::{
    JobRuntimePoolSnapshot, LlmStreamEvent, MailboxMessage, RuntimeEventReason, RuntimeRef,
    ThreadEvent, ThreadNoticeLevel,
};

/// Envelope for thread events sent to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadEventEnvelope {
    /// The session ID that owns this thread.
    pub session_id: String,
    /// The thread ID.
    pub thread_id: String,
    /// The turn number (if applicable).
    pub turn_number: Option<u32>,
    /// The event payload.
    pub payload: ThreadEventPayload,
}

impl ThreadEventEnvelope {
    /// Create an envelope from a ThreadEvent.
    pub fn from_thread_event(session_id: String, event: ThreadEvent) -> Option<Self> {
        match event {
            ThreadEvent::Processing {
                thread_id,
                turn_number,
                event,
            } => ThreadEventPayload::from_llm_event(event).map(|payload| Self {
                session_id,
                thread_id,
                turn_number: Some(turn_number),
                payload,
            }),
            ThreadEvent::ToolStarted {
                thread_id,
                turn_number,
                tool_call_id,
                tool_name,
                arguments,
            } => Some(Self {
                session_id,
                thread_id,
                turn_number: Some(turn_number),
                payload: ThreadEventPayload::ToolStarted {
                    tool_call_id,
                    tool_name,
                    arguments,
                },
            }),
            ThreadEvent::ToolCompleted {
                thread_id,
                turn_number,
                tool_call_id,
                tool_name,
                result,
            } => {
                let (result, is_error) = match result {
                    Ok(result) => (result, false),
                    Err(error) => (serde_json::Value::String(error), true),
                };

                Some(Self {
                    session_id,
                    thread_id,
                    turn_number: Some(turn_number),
                    payload: ThreadEventPayload::ToolCompleted {
                        tool_call_id,
                        tool_name,
                        result,
                        is_error,
                    },
                })
            }
            ThreadEvent::TurnCompleted {
                thread_id,
                turn_number,
                token_usage,
            } => Some(Self {
                session_id,
                thread_id,
                turn_number: Some(turn_number),
                payload: ThreadEventPayload::TurnCompleted {
                    input_tokens: token_usage.input_tokens,
                    output_tokens: token_usage.output_tokens,
                    total_tokens: token_usage.total_tokens,
                },
            }),
            ThreadEvent::TurnFailed {
                thread_id,
                turn_number,
                error,
            } => Some(Self {
                session_id,
                thread_id,
                turn_number: Some(turn_number),
                payload: ThreadEventPayload::TurnFailed { error },
            }),
            ThreadEvent::TurnSettled {
                thread_id,
                turn_number,
            } => Some(Self {
                session_id,
                thread_id,
                turn_number: Some(turn_number),
                payload: ThreadEventPayload::TurnSettled,
            }),
            ThreadEvent::Idle { thread_id } => Some(Self {
                session_id,
                thread_id,
                turn_number: None,
                payload: ThreadEventPayload::Idle,
            }),
            ThreadEvent::Notice {
                thread_id,
                level,
                message,
            } => Some(Self {
                session_id,
                thread_id,
                turn_number: None,
                payload: ThreadEventPayload::Notice { level, message },
            }),
            ThreadEvent::Compacted {
                thread_id,
                new_token_count,
            } => Some(Self {
                session_id,
                thread_id,
                turn_number: None,
                payload: ThreadEventPayload::Compacted { new_token_count },
            }),
            ThreadEvent::CompactionStarted { thread_id } => Some(Self {
                session_id,
                thread_id,
                turn_number: None,
                payload: ThreadEventPayload::CompactionStarted,
            }),
            ThreadEvent::CompactionFinished { thread_id } => Some(Self {
                session_id,
                thread_id,
                turn_number: None,
                payload: ThreadEventPayload::CompactionFinished,
            }),
            ThreadEvent::CompactionFailed { thread_id, error } => Some(Self {
                session_id,
                thread_id,
                turn_number: None,
                payload: ThreadEventPayload::CompactionFailed { error },
            }),
            ThreadEvent::JobDispatched {
                thread_id,
                job_id,
                agent_id,
                prompt,
                context,
            } => Some(Self {
                session_id,
                thread_id: thread_id.inner().to_string(),
                turn_number: None,
                payload: ThreadEventPayload::JobDispatched {
                    job_id,
                    agent_id: agent_id.inner(),
                    prompt,
                    context,
                },
            }),
            ThreadEvent::JobResult {
                thread_id,
                job_id,
                success,
                message,
                token_usage,
                agent_id,
                agent_display_name,
                agent_description,
            } => Some(Self {
                session_id,
                thread_id: thread_id.inner().to_string(),
                turn_number: None,
                payload: ThreadEventPayload::JobResult {
                    job_id,
                    success,
                    message,
                    input_tokens: token_usage.as_ref().map(|u| u.input_tokens),
                    output_tokens: token_usage.as_ref().map(|u| u.output_tokens),
                    agent_id: agent_id.inner(),
                    agent_display_name,
                    agent_description,
                },
            }),
            ThreadEvent::MailboxMessageQueued { thread_id, message } => Some(Self {
                session_id,
                thread_id: thread_id.inner().to_string(),
                turn_number: None,
                payload: ThreadEventPayload::MailboxMessageQueued { message },
            }),
            ThreadEvent::ThreadBoundToJob { job_id, thread_id } => Some(Self {
                session_id,
                thread_id: thread_id.inner().to_string(),
                turn_number: None,
                payload: ThreadEventPayload::ThreadBoundToJob { job_id },
            }),
            ThreadEvent::JobRuntimeQueued { runtime } => Some(Self {
                session_id,
                thread_id: runtime.thread_id.inner().to_string(),
                turn_number: None,
                payload: ThreadEventPayload::JobRuntimeQueued { runtime },
            }),
            ThreadEvent::JobRuntimeStarted { runtime } => Some(Self {
                session_id,
                thread_id: runtime.thread_id.inner().to_string(),
                turn_number: None,
                payload: ThreadEventPayload::JobRuntimeStarted { runtime },
            }),
            ThreadEvent::JobRuntimeCooling { runtime } => Some(Self {
                session_id,
                thread_id: runtime.thread_id.inner().to_string(),
                turn_number: None,
                payload: ThreadEventPayload::JobRuntimeCooling { runtime },
            }),
            ThreadEvent::JobRuntimeEvicted { runtime, reason } => Some(Self {
                session_id,
                thread_id: runtime.thread_id.inner().to_string(),
                turn_number: None,
                payload: ThreadEventPayload::JobRuntimeEvicted { runtime, reason },
            }),
            ThreadEvent::JobRuntimeMetricsUpdated { snapshot } => Some(Self {
                session_id,
                thread_id: String::new(),
                turn_number: None,
                payload: ThreadEventPayload::JobRuntimeMetricsUpdated { snapshot },
            }),
            ThreadEvent::UserInterrupt { .. } => None,
            ThreadEvent::UserMessage { .. } => None,
        }
    }
}

/// Payload types for thread events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThreadEventPayload {
    ReasoningDelta {
        delta: String,
    },
    ContentDelta {
        delta: String,
    },
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: Option<String>,
    },
    LlmUsage {
        input_tokens: u32,
        output_tokens: u32,
        total_tokens: u32,
    },
    RetryAttempt {
        attempt: u32,
        max_retries: u32,
        error: String,
    },
    ToolStarted {
        tool_call_id: String,
        tool_name: String,
        arguments: serde_json::Value,
    },
    ToolCompleted {
        tool_call_id: String,
        tool_name: String,
        result: serde_json::Value,
        is_error: bool,
    },
    TurnCompleted {
        input_tokens: u32,
        output_tokens: u32,
        total_tokens: u32,
    },
    TurnFailed {
        error: String,
    },
    TurnSettled,
    Idle,
    Notice {
        level: ThreadNoticeLevel,
        message: String,
    },
    Compacted {
        new_token_count: u32,
    },
    CompactionStarted,
    CompactionFinished,
    CompactionFailed {
        error: String,
    },
    ThreadBoundToJob {
        job_id: String,
    },
    JobRuntimeQueued {
        runtime: RuntimeRef,
    },
    JobRuntimeStarted {
        runtime: RuntimeRef,
    },
    JobRuntimeCooling {
        runtime: RuntimeRef,
    },
    JobRuntimeEvicted {
        runtime: RuntimeRef,
        reason: RuntimeEventReason,
    },
    JobRuntimeMetricsUpdated {
        snapshot: JobRuntimePoolSnapshot,
    },
    JobDispatched {
        job_id: String,
        agent_id: i64,
        prompt: String,
        context: Option<serde_json::Value>,
    },
    JobResult {
        job_id: String,
        success: bool,
        message: String,
        input_tokens: Option<u32>,
        output_tokens: Option<u32>,
        agent_id: i64,
        agent_display_name: String,
        agent_description: String,
    },
    MailboxMessageQueued {
        message: MailboxMessage,
    },
}

impl ThreadEventPayload {
    /// Convert an LLM stream event to a payload.
    pub fn from_llm_event(event: LlmStreamEvent) -> Option<Self> {
        match event {
            LlmStreamEvent::ReasoningDelta { delta } => Some(Self::ReasoningDelta { delta }),
            LlmStreamEvent::ContentDelta { delta } => Some(Self::ContentDelta { delta }),
            LlmStreamEvent::ToolCallDelta(delta) => Some(Self::ToolCallDelta {
                index: delta.index,
                id: delta.id,
                name: delta.name,
                arguments_delta: delta.arguments_delta,
            }),
            LlmStreamEvent::Usage {
                input_tokens,
                output_tokens,
            } => Some(Self::LlmUsage {
                input_tokens,
                output_tokens,
                total_tokens: input_tokens + output_tokens,
            }),
            LlmStreamEvent::RetryAttempt {
                attempt,
                max_retries,
                error,
            } => Some(Self::RetryAttempt {
                attempt,
                max_retries,
                error,
            }),
            LlmStreamEvent::Finished { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use argus_protocol::{JobRuntimePoolSnapshot, LlmStreamEvent, ThreadEvent, ThreadId};

    use super::{ThreadEventEnvelope, ThreadEventPayload};

    #[test]
    fn processing_event_conversion_keeps_route_fields() {
        let session_id = "session-1".to_string();
        let thread_id = ThreadId::new();
        let event = argus_protocol::ThreadEvent::Processing {
            thread_id: thread_id.inner().to_string(),
            turn_number: 3,
            event: LlmStreamEvent::ContentDelta {
                delta: "hello".to_string(),
            },
        };

        let envelope = ThreadEventEnvelope::from_thread_event(session_id.clone(), event)
            .expect("content delta should forward");

        assert_eq!(envelope.session_id, session_id);
        assert_eq!(envelope.thread_id, thread_id.inner().to_string());
        assert_eq!(envelope.turn_number, Some(3));
        assert!(matches!(
            envelope.payload,
            ThreadEventPayload::ContentDelta { ref delta } if delta == "hello"
        ));
    }

    #[test]
    fn tool_completed_error_conversion_preserves_error_text() {
        let thread_id = ThreadId::new();
        let envelope = ThreadEventEnvelope::from_thread_event(
            "session-1".to_string(),
            argus_protocol::ThreadEvent::ToolCompleted {
                thread_id: thread_id.inner().to_string(),
                turn_number: 1,
                tool_call_id: "call-1".to_string(),
                tool_name: "shell".to_string(),
                result: Err("command failed".to_string()),
            },
        )
        .expect("tool completed errors should still forward");

        assert!(matches!(
            envelope.payload,
            ThreadEventPayload::ToolCompleted {
                ref tool_call_id,
                ref tool_name,
                ref result,
                is_error: true,
            } if tool_call_id == "call-1"
                && tool_name == "shell"
                && result == &serde_json::Value::String("command failed".to_string())
        ));
    }

    #[test]
    fn job_runtime_metrics_updated_event_conversion_preserves_snapshot() {
        let snapshot = JobRuntimePoolSnapshot {
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

        let envelope = ThreadEventEnvelope::from_thread_event(
            "session-1".to_string(),
            ThreadEvent::JobRuntimeMetricsUpdated {
                snapshot: snapshot.clone(),
            },
        )
        .expect("snapshot updates should forward");

        assert_eq!(envelope.session_id, "session-1");
        assert_eq!(envelope.thread_id, String::new());
        assert_eq!(envelope.turn_number, None);
        assert!(matches!(
            envelope.payload,
            ThreadEventPayload::JobRuntimeMetricsUpdated { snapshot: ref forwarded }
                if forwarded == &snapshot
        ));
    }
}
