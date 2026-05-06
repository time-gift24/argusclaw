//! Session repository trait.

use async_trait::async_trait;

use crate::error::DbError;
use crate::types::SessionRecord;
use argus_protocol::{SessionId, UserId};

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

    /// List sessions owned by one user. Backends without user ownership may fall back to raw listing.
    async fn list_with_counts_for_user(
        &self,
        _user_id: &UserId,
    ) -> Result<Vec<SessionWithCount>, DbError> {
        self.list_with_counts().await
    }

    /// Get a session owned by one user.
    async fn get_for_user(
        &self,
        _user_id: &UserId,
        id: &SessionId,
    ) -> Result<Option<SessionRecord>, DbError> {
        self.get(id).await
    }

    /// Create a new session owned by one user.
    async fn create_for_user(
        &self,
        _user_id: &UserId,
        id: &SessionId,
        name: &str,
    ) -> Result<(), DbError> {
        self.create(id, name).await
    }

    /// Rename a session owned by one user.
    async fn rename_for_user(
        &self,
        _user_id: &UserId,
        id: &SessionId,
        name: &str,
    ) -> Result<bool, DbError> {
        self.rename(id, name).await
    }

    /// Delete a session owned by one user.
    async fn delete_for_user(&self, _user_id: &UserId, id: &SessionId) -> Result<bool, DbError> {
        self.delete(id).await
    }
}
