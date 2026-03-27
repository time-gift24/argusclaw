//! Thread persistence types.

use std::fmt;

use serde::{Deserialize, Serialize};

use argus_protocol::{AgentId, SessionId, ThreadId};
use argus_protocol::llm::LlmProviderId;

/// Unique identifier for a stored message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub i64);

impl MessageId {
    /// Create a new message ID.
    pub fn new(id: i64) -> Self {
        Self(id)
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Stored message record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRecord {
    /// Unique ID (auto-generated).
    pub id: Option<MessageId>,
    /// Thread this message belongs to.
    pub thread_id: ThreadId,
    /// Message sequence number within the thread.
    pub seq: u32,
    /// Role: system, user, assistant, tool.
    pub role: String,
    /// Message content.
    pub content: String,
    /// Tool call ID (if role is tool).
    pub tool_call_id: Option<String>,
    /// Tool name (if role is tool).
    pub tool_name: Option<String>,
    /// Tool calls JSON (if role is assistant with tool calls).
    pub tool_calls: Option<String>,
    /// Creation timestamp.
    pub created_at: String,
}

/// Stored thread record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadRecord {
    /// Thread ID.
    pub id: ThreadId,
    /// LLM provider ID.
    pub provider_id: LlmProviderId,
    /// Thread title (optional, can be auto-generated).
    pub title: Option<String>,
    /// Total token count.
    pub token_count: u32,
    /// Turn count.
    pub turn_count: u32,
    /// Session this thread belongs to (for session-scoped queries).
    pub session_id: Option<SessionId>,
    /// Template (agent) this thread uses.
    pub template_id: Option<AgentId>,
    /// Per-thread model override.
    pub model_override: Option<String>,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
}

/// Session record (minimal, for repository operations).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Session ID.
    pub id: SessionId,
    /// Session name.
    pub name: String,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
}
