//! Thread persistence types and repository trait.

use std::fmt;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::agents::thread::ThreadId;
use crate::db::DbError;

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
    pub provider_id: String,
    /// Thread title (optional, can be auto-generated).
    pub title: Option<String>,
    /// Total token count.
    pub token_count: u32,
    /// Turn count.
    pub turn_count: u32,
    /// Creation timestamp.
    pub created_at: String,
    /// Last update timestamp.
    pub updated_at: String,
}

/// Repository trait for thread persistence.
#[async_trait]
pub trait ThreadRepository: Send + Sync {
    /// Create or update a thread record.
    async fn upsert_thread(&self, thread: &ThreadRecord) -> Result<(), DbError>;

    /// Get a thread by ID.
    async fn get_thread(&self, id: &ThreadId) -> Result<Option<ThreadRecord>, DbError>;

    /// List all threads (most recent first).
    async fn list_threads(&self, limit: u32) -> Result<Vec<ThreadRecord>, DbError>;

    /// Delete a thread and all its messages.
    async fn delete_thread(&self, id: &ThreadId) -> Result<bool, DbError>;

    /// Add a message to a thread.
    async fn add_message(&self, message: &MessageRecord) -> Result<MessageId, DbError>;

    /// Get all messages for a thread.
    async fn get_messages(&self, thread_id: &ThreadId) -> Result<Vec<MessageRecord>, DbError>;

    /// Get the last N messages for a thread.
    async fn get_recent_messages(
        &self,
        thread_id: &ThreadId,
        limit: u32,
    ) -> Result<Vec<MessageRecord>, DbError>;

    /// Delete messages older than a sequence number.
    async fn delete_messages_before(&self, thread_id: &ThreadId, seq: u32) -> Result<u64, DbError>;

    /// Update thread statistics (token_count, turn_count).
    async fn update_thread_stats(
        &self,
        id: &ThreadId,
        token_count: u32,
        turn_count: u32,
    ) -> Result<(), DbError>;
}
