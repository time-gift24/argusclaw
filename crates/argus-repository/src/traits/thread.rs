//! Thread repository trait.

use async_trait::async_trait;

use crate::error::DbError;
use crate::types::{MessageId, MessageRecord, ThreadRecord};
use argus_protocol::{LlmProviderId, SessionId, ThreadId, UserId};

/// Repository trait for thread persistence.
#[async_trait]
pub trait ThreadRepository: Send + Sync {
    /// Create or update a thread record.
    async fn upsert_thread(&self, thread: &ThreadRecord) -> Result<(), DbError>;

    /// Get a thread by ID.
    async fn get_thread(&self, id: &ThreadId) -> Result<Option<ThreadRecord>, DbError>;

    /// List all threads (most recent first).
    async fn list_threads(&self, limit: u32) -> Result<Vec<ThreadRecord>, DbError>;

    /// List threads belonging to a session.
    async fn list_threads_in_session(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<ThreadRecord>, DbError>;

    /// Delete a thread and all its messages.
    async fn delete_thread(&self, id: &ThreadId) -> Result<bool, DbError>;

    /// Delete all threads in a session.
    async fn delete_threads_in_session(&self, session_id: &SessionId) -> Result<u64, DbError>;

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

    /// Rename a thread (update title).
    async fn rename_thread(
        &self,
        id: &ThreadId,
        session_id: &SessionId,
        title: Option<&str>,
    ) -> Result<bool, DbError>;

    /// Update a thread's provider and model override.
    async fn update_thread_model(
        &self,
        id: &ThreadId,
        session_id: &SessionId,
        provider_id: LlmProviderId,
        model_override: Option<&str>,
    ) -> Result<bool, DbError>;

    /// Get a thread by session + thread ID (returns template_id and model_override too).
    async fn get_thread_in_session(
        &self,
        thread_id: &ThreadId,
        session_id: &SessionId,
    ) -> Result<Option<ThreadRecord>, DbError>;
    /// Create or update a user-owned thread record.
    async fn upsert_thread_for_user(
        &self,
        _user_id: &UserId,
        thread: &ThreadRecord,
    ) -> Result<(), DbError> {
        self.upsert_thread(thread).await
    }

    /// Get a user-owned thread by ID.
    async fn get_thread_for_user(
        &self,
        _user_id: &UserId,
        id: &ThreadId,
    ) -> Result<Option<ThreadRecord>, DbError> {
        self.get_thread(id).await
    }

    /// List user-owned threads in a session.
    async fn list_threads_in_session_for_user(
        &self,
        _user_id: &UserId,
        session_id: &SessionId,
    ) -> Result<Vec<ThreadRecord>, DbError> {
        self.list_threads_in_session(session_id).await
    }

    /// Delete a user-owned thread.
    async fn delete_thread_for_user(
        &self,
        _user_id: &UserId,
        id: &ThreadId,
    ) -> Result<bool, DbError> {
        self.delete_thread(id).await
    }

    /// Delete all user-owned threads in a session.
    async fn delete_threads_in_session_for_user(
        &self,
        _user_id: &UserId,
        session_id: &SessionId,
    ) -> Result<u64, DbError> {
        self.delete_threads_in_session(session_id).await
    }

    /// Rename a user-owned thread.
    async fn rename_thread_for_user(
        &self,
        _user_id: &UserId,
        id: &ThreadId,
        session_id: &SessionId,
        title: Option<&str>,
    ) -> Result<bool, DbError> {
        self.rename_thread(id, session_id, title).await
    }

    /// Update a user-owned thread's provider and model override.
    async fn update_thread_model_for_user(
        &self,
        _user_id: &UserId,
        id: &ThreadId,
        session_id: &SessionId,
        provider_id: LlmProviderId,
        model_override: Option<&str>,
    ) -> Result<bool, DbError> {
        self.update_thread_model(id, session_id, provider_id, model_override)
            .await
    }

    /// Get a user-owned thread by session + thread ID.
    async fn get_thread_in_session_for_user(
        &self,
        _user_id: &UserId,
        thread_id: &ThreadId,
        session_id: &SessionId,
    ) -> Result<Option<ThreadRecord>, DbError> {
        self.get_thread_in_session(thread_id, session_id).await
    }
}
