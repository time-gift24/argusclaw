use serde::{Deserialize, Serialize};

use crate::{AgentId, ThreadId};

/// A snapshot of a tool call within a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallSnapshot {
    /// The unique ID of the tool call.
    pub id: String,
    /// The name of the tool being called.
    pub name: String,
    /// The arguments passed to the tool.
    pub arguments: serde_json::Value,
}

/// A snapshot of a message in a thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadMessageSnapshot {
    /// The role of the message sender (system, user, assistant, tool).
    pub role: String,
    /// The content of the message.
    pub content: String,
    /// The tool call ID if this is a tool response message.
    pub tool_call_id: Option<String>,
    /// The name of the tool if this is a tool message.
    pub name: Option<String>,
    /// Tool calls made in this message (assistant messages only).
    pub tool_calls: Option<Vec<ToolCallSnapshot>>,
}

/// A snapshot of a thread's current state.
///
/// This captures the messages and metadata of a thread at a point in time,
/// suitable for serialization and transmission to frontend clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadSnapshot {
    /// The ID of the runtime agent that owns this thread.
    pub runtime_agent_id: AgentId,
    /// The ID of the thread.
    pub thread_id: ThreadId,
    /// The messages in the thread.
    pub messages: Vec<ThreadMessageSnapshot>,
    /// The number of completed turns.
    pub turn_count: u32,
    /// The current token count.
    pub token_count: u32,
}
