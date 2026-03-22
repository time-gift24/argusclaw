use argus_protocol::SessionId;
use async_trait::async_trait;

use crate::error::DbError;
use crate::types::SessionRecord;

#[derive(Debug, Clone)]
pub struct SessionSummaryRecord {
    pub id: SessionId,
    pub name: String,
    pub thread_count: i64,
    pub template_id: Option<i64>,
    pub provider_id: Option<i64>,
    pub updated_at: String,
}

#[async_trait]
pub trait SessionRepository: Send + Sync {
    /// Create a new session.
    async fn create_session(&self, name: &str) -> Result<SessionId, DbError>;

    /// Get a session by ID.
    async fn get_session(&self, id: SessionId) -> Result<Option<SessionRecord>, DbError>;

    /// List all sessions with thread counts.
    async fn list_sessions(&self) -> Result<Vec<SessionSummaryRecord>, DbError>;

    /// Update session name and updated_at.
    async fn update_session(&self, id: SessionId, name: &str) -> Result<(), DbError>;

    /// Delete a session and its threads (cascade).
    async fn delete_session(&self, id: SessionId) -> Result<bool, DbError>;

    /// Delete sessions older than the specified days.
    async fn cleanup_old_sessions(&self, days: u32) -> Result<u64, DbError>;
}
