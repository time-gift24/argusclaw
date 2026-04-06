//! Session repository trait.

use async_trait::async_trait;

use crate::error::DbError;
use crate::types::SessionRecord;
use argus_protocol::SessionId;

/// A session with its thread count (from LEFT JOIN).
#[derive(Debug)]
pub struct SessionWithCount {
    pub session: SessionRecord,
    pub thread_count: i64,
}

/// Repository trait for session persistence.
#[async_trait]
pub trait SessionRepository: Send + Sync {
    /// List all sessions with thread counts, ordered by most recently updated.
    async fn list_with_counts(&self) -> Result<Vec<SessionWithCount>, DbError>;

    /// Get a session by ID.
    async fn get(&self, id: &SessionId) -> Result<Option<SessionRecord>, DbError>;

    /// Create a new session.
    async fn create(&self, id: &SessionId, name: &str) -> Result<(), DbError>;

    /// Rename a session.
    async fn rename(&self, id: &SessionId, name: &str) -> Result<bool, DbError>;

    /// Delete a session (caller is responsible for deleting threads first).
    async fn delete(&self, id: &SessionId) -> Result<bool, DbError>;
}

/// Repository trait for user-owned session persistence in the server product.
#[async_trait]
pub trait UserSessionRepository: Send + Sync {
    /// Create a session owned by the given user.
    async fn create_for_user(
        &self,
        id: &SessionId,
        name: &str,
        owner_user_id: i64,
    ) -> Result<(), DbError>;

    /// List sessions owned by the given user with thread counts.
    async fn list_with_counts_for_user(
        &self,
        owner_user_id: i64,
    ) -> Result<Vec<SessionWithCount>, DbError>;

    /// Check whether the given user owns the given session.
    async fn user_owns_session(
        &self,
        owner_user_id: i64,
        session_id: &SessionId,
    ) -> Result<bool, DbError>;
}
