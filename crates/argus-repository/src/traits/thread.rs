//! Thread repository trait.

use async_trait::async_trait;

use crate::error::DbError;
use crate::types::{MessageId, MessageRecord, ThreadRecord};
use argus_protocol::ThreadId;

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
