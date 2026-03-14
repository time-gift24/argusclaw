//! TauriContext - bridges Tauri IPC with Claw AppContext.
//!
//! This module provides the integration layer between the Tauri frontend
//! and the backend Claw AppContext, handling Thread operations and event streaming.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};
use tokio::sync::OnceCell;

use claw::agents::thread::{ThreadEvent, ThreadId};
use claw::agents::AgentRuntimeId;
use claw::llm::ChatMessage;

/// Wrapper that combines AppContext with Tauri's AppHandle.
///
/// This provides methods for Thread operations that can emit events to the frontend.
pub struct TauriContext {
    /// Reference to the Claw AppContext.
    app_context: Arc<claw::AppContext>,
    /// Tauri app handle for emitting events.
    app_handle: AppHandle,
    /// ArgusAgent's runtime ID (initialized once).
    argus_agent_id: OnceCell<AgentRuntimeId>,
}

impl TauriContext {
    /// Create a new TauriContext.
    pub fn new(app_context: Arc<claw::AppContext>, app_handle: AppHandle) -> Self {
        Self {
            app_context,
            app_handle,
            argus_agent_id: OnceCell::new(),
        }
    }

    /// Get or initialize the ArgusAgent.
    async fn get_argus_agent_id(&self) -> Result<AgentRuntimeId, String> {
        if let Some(id) = self.argus_agent_id.get() {
            return Ok(*id);
        }

        // Initialize ArgusAgent
        let id = self
            .app_context
            .init_argus_agent()
            .await
            .map_err(|e| e.to_string())?;

        let _ = self.argus_agent_id.set(id);
        tracing::info!("ArgusAgent initialized with runtime_id: {}", id);
        Ok(id)
    }

    /// Subscribe to a thread's events.
    ///
    /// This spawns a background task that forwards ThreadEvent to the frontend
    /// via Tauri events.
    pub async fn subscribe_thread(&self, thread_id: &str) -> Result<(), String> {
        let agent_runtime_id = self.get_argus_agent_id().await?;
        let thread_id = ThreadId::parse(thread_id).map_err(|e| e.to_string())?;

        // Get or create thread and subscribe to events
        let mut event_rx = self
            .app_context
            .get_or_create_thread(agent_runtime_id, thread_id, None)
            .map_err(|e| e.to_string())?;

        let app_handle = self.app_handle.clone();
        let thread_id_str = thread_id.to_string();

        // Spawn background task to forward events
        tokio::spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                // Serialize event to JSON for Tauri
                let event_json = serde_json::to_string(&ThreadEventData::from(event.clone()));
                if let Ok(json) = event_json {
                    let _ = app_handle.emit("thread:event", json);
                }

                // Log for debugging
                tracing::debug!("Thread {} event: {:?}", thread_id_str, event);
            }
        });

        Ok(())
    }

    /// Send a message to a thread.
    ///
    /// This is non-blocking - it returns immediately and the response
    /// comes through the event stream.
    pub async fn send_message(&self, thread_id: &str, message: String) -> Result<(), String> {
        let agent_runtime_id = self.get_argus_agent_id().await?;
        let thread_id = ThreadId::parse(thread_id).map_err(|e| e.to_string())?;

        // Ensure thread exists
        let _ = self
            .app_context
            .get_or_create_thread(agent_runtime_id, thread_id, None)
            .map_err(|e| e.to_string())?;

        // Send message (non-blocking)
        self.app_context
            .send_message(agent_runtime_id, thread_id, message)
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Get messages from a thread.
    pub async fn get_messages(&self, thread_id: &str) -> Result<Vec<ChatMessageData>, String> {
        let agent_runtime_id = self.get_argus_agent_id().await?;
        let thread_id = ThreadId::parse(thread_id).map_err(|e| e.to_string())?;

        // Ensure thread exists
        let _ = self
            .app_context
            .get_or_create_thread(agent_runtime_id, thread_id, None)
            .map_err(|e| e.to_string())?;

        let messages = self
            .app_context
            .get_thread_messages(agent_runtime_id, thread_id)
            .map_err(|e| e.to_string())?;

        Ok(messages.into_iter().map(ChatMessageData::from).collect())
    }

    /// Create a new thread with a specific ID.
    pub async fn create_thread(&self, thread_id: &str) -> Result<(), String> {
        let agent_runtime_id = self.get_argus_agent_id().await?;
        let thread_id = ThreadId::parse(thread_id).map_err(|e| e.to_string())?;

        self.app_context
            .get_or_create_thread(agent_runtime_id, thread_id, None)
            .map_err(|e| e.to_string())?;

        Ok(())
    }
}

/// Serializable thread event data for frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ThreadEventData {
    Processing {
        thread_id: String,
        turn_number: u32,
        event: LlmStreamEventData,
    },
    ToolStarted {
        thread_id: String,
        turn_number: u32,
        tool_call_id: String,
        tool_name: String,
        arguments: serde_json::Value,
    },
    ToolCompleted {
        thread_id: String,
        turn_number: u32,
        tool_call_id: String,
        tool_name: String,
        result: Result<serde_json::Value, String>,
    },
    TurnCompleted {
        thread_id: String,
        turn_number: u32,
        token_usage: TokenUsageData,
    },
    TurnFailed {
        thread_id: String,
        turn_number: u32,
        error: String,
    },
    Idle {
        thread_id: String,
    },
    Compacted {
        thread_id: String,
        new_token_count: u32,
    },
}

impl From<ThreadEvent> for ThreadEventData {
    fn from(event: ThreadEvent) -> Self {
        match event {
            ThreadEvent::Processing {
                thread_id,
                turn_number,
                event,
            } => ThreadEventData::Processing {
                thread_id: thread_id.to_string(),
                turn_number,
                event: LlmStreamEventData::from(event),
            },
            ThreadEvent::ToolStarted {
                thread_id,
                turn_number,
                tool_call_id,
                tool_name,
                arguments,
            } => ThreadEventData::ToolStarted {
                thread_id: thread_id.to_string(),
                turn_number,
                tool_call_id,
                tool_name,
                arguments,
            },
            ThreadEvent::ToolCompleted {
                thread_id,
                turn_number,
                tool_call_id,
                tool_name,
                result,
            } => ThreadEventData::ToolCompleted {
                thread_id: thread_id.to_string(),
                turn_number,
                tool_call_id,
                tool_name,
                result,
            },
            ThreadEvent::TurnCompleted {
                thread_id,
                turn_number,
                token_usage,
            } => ThreadEventData::TurnCompleted {
                thread_id: thread_id.to_string(),
                turn_number,
                token_usage: TokenUsageData::from(token_usage),
            },
            ThreadEvent::TurnFailed {
                thread_id,
                turn_number,
                error,
            } => ThreadEventData::TurnFailed {
                thread_id: thread_id.to_string(),
                turn_number,
                error,
            },
            ThreadEvent::Idle { thread_id } => ThreadEventData::Idle {
                thread_id: thread_id.to_string(),
            },
            ThreadEvent::Compacted {
                thread_id,
                new_token_count,
            } => ThreadEventData::Compacted {
                thread_id: thread_id.to_string(),
                new_token_count,
            },
            ThreadEvent::WaitingForApproval { .. } => {
                // Skip approval events for now - not needed for basic integration
                ThreadEventData::Idle {
                    thread_id: String::new(),
                }
            }
            ThreadEvent::ApprovalResolved { .. } => {
                // Skip approval events for now
                ThreadEventData::Idle {
                    thread_id: String::new(),
                }
            }
        }
    }
}

/// Serializable LLM stream event data for frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LlmStreamEventData {
    ReasoningDelta {
        delta: String,
    },
    ContentDelta {
        delta: String,
    },
    ToolCallDelta {
        delta: ToolCallDeltaData,
    },
    Usage {
        input_tokens: u32,
        output_tokens: u32,
    },
    Finished {
        finish_reason: String,
    },
}

impl From<claw::llm::LlmStreamEvent> for LlmStreamEventData {
    fn from(event: claw::llm::LlmStreamEvent) -> Self {
        match event {
            claw::llm::LlmStreamEvent::ReasoningDelta { delta } => {
                LlmStreamEventData::ReasoningDelta { delta }
            }
            claw::llm::LlmStreamEvent::ContentDelta { delta } => {
                LlmStreamEventData::ContentDelta { delta }
            }
            claw::llm::LlmStreamEvent::ToolCallDelta(delta) => LlmStreamEventData::ToolCallDelta {
                delta: ToolCallDeltaData::from(delta),
            },
            claw::llm::LlmStreamEvent::Usage {
                input_tokens,
                output_tokens,
            } => LlmStreamEventData::Usage {
                input_tokens,
                output_tokens,
            },
            claw::llm::LlmStreamEvent::Finished { finish_reason } => LlmStreamEventData::Finished {
                finish_reason: format!("{:?}", finish_reason),
            },
        }
    }
}

/// Serializable tool call delta for frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCallDeltaData {
    pub index: usize,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments_delta: Option<String>,
}

impl From<claw::llm::ToolCallDelta> for ToolCallDeltaData {
    fn from(delta: claw::llm::ToolCallDelta) -> Self {
        Self {
            index: delta.index,
            id: delta.id,
            name: delta.name,
            arguments_delta: delta.arguments_delta,
        }
    }
}

/// Serializable token usage for frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TokenUsageData {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

impl From<claw::agents::turn::TokenUsage> for TokenUsageData {
    fn from(usage: claw::agents::turn::TokenUsage) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
        }
    }
}

/// Serializable chat message for frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessageData {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallData>>,
}

impl From<ChatMessage> for ChatMessageData {
    fn from(msg: ChatMessage) -> Self {
        Self {
            role: format!("{:?}", msg.role).to_lowercase(),
            content: msg.content,
            tool_calls: msg.tool_calls.map(|calls| {
                calls
                    .into_iter()
                    .map(|c| ToolCallData {
                        id: c.id,
                        name: c.name,
                        arguments: c.arguments,
                    })
                    .collect()
            }),
        }
    }
}

/// Serializable tool call for frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCallData {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}
