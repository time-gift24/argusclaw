//! User-facing chat service boundary for the server product.

use std::sync::Arc;

use argus_protocol::{SessionId, ThreadEvent, ThreadId};
use argus_repository::traits::{SessionWithCount, UserSessionRepository};
use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::session::{SessionSummary, ThreadSummary};
use crate::SessionManager;

/// Authenticated principal for server user operations.
#[derive(Debug, Clone)]
pub struct UserPrincipal {
    pub user_id: i64,
    pub account: String,
    pub display_name: String,
}

/// Error type for user chat service operations.
#[derive(Debug, thiserror::Error)]
pub enum UserChatError {
    #[error("resource not found")]
    NotFound,

    #[error("agent is not enabled")]
    AgentNotEnabled,

    #[error("internal service error: {reason}")]
    Internal { reason: String },
}

impl From<argus_protocol::ArgusError> for UserChatError {
    fn from(error: argus_protocol::ArgusError) -> Self {
        Self::Internal {
            reason: error.to_string(),
        }
    }
}

impl From<argus_repository::DbError> for UserChatError {
    fn from(error: argus_repository::DbError) -> Self {
        Self::Internal {
            reason: error.to_string(),
        }
    }
}

/// Server-facing chat API abstraction.
#[async_trait]
pub trait UserChatApi: Send + Sync {
    async fn list_enabled_agents(&self) -> Vec<argus_protocol::AgentRecord>;
    async fn create_session(
        &self,
        user: &UserPrincipal,
        name: &str,
    ) -> Result<SessionId, UserChatError>;
    async fn list_sessions(
        &self,
        user: &UserPrincipal,
    ) -> Result<Vec<SessionSummary>, UserChatError>;
    async fn list_threads(
        &self,
        user: &UserPrincipal,
        session_id: SessionId,
    ) -> Result<Vec<ThreadSummary>, UserChatError>;
    async fn send_message(
        &self,
        user: &UserPrincipal,
        session_id: SessionId,
        thread_id: ThreadId,
        message: String,
    ) -> Result<(), UserChatError>;
    async fn subscribe(
        &self,
        user: &UserPrincipal,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<broadcast::Receiver<ThreadEvent>, UserChatError>;
}

/// User-facing chat service boundary.
pub struct UserChatServices {
    session_manager: Arc<SessionManager>,
    user_session_repo: Arc<dyn UserSessionRepository>,
}

impl UserChatServices {
    /// Create a new `UserChatServices` for the server product.
    pub fn new(
        session_manager: Arc<SessionManager>,
        user_session_repo: Arc<dyn UserSessionRepository>,
    ) -> Self {
        Self {
            session_manager,
            user_session_repo,
        }
    }

    async fn verify_session_owner(
        &self,
        user_id: i64,
        session_id: &SessionId,
    ) -> Result<(), UserChatError> {
        let owns = self
            .user_session_repo
            .user_owns_session(user_id, session_id)
            .await?;
        if owns {
            Ok(())
        } else {
            Err(UserChatError::NotFound)
        }
    }
}

#[async_trait]
impl UserChatApi for UserChatServices {
    async fn list_enabled_agents(&self) -> Vec<argus_protocol::AgentRecord> {
        let agents = self.session_manager.list_templates_for_user().await;
        agents
            .into_iter()
            .filter(|agent| agent.is_enabled)
            .collect()
    }

    async fn create_session(
        &self,
        user: &UserPrincipal,
        name: &str,
    ) -> Result<SessionId, UserChatError> {
        let session_id = SessionId::new();
        self.user_session_repo
            .create_for_user(&session_id, name, user.user_id)
            .await?;
        Ok(session_id)
    }

    async fn list_sessions(
        &self,
        user: &UserPrincipal,
    ) -> Result<Vec<SessionSummary>, UserChatError> {
        let sessions = self
            .user_session_repo
            .list_with_counts_for_user(user.user_id)
            .await?;
        Ok(sessions.into_iter().map(map_session_summary).collect())
    }

    async fn list_threads(
        &self,
        user: &UserPrincipal,
        session_id: SessionId,
    ) -> Result<Vec<ThreadSummary>, UserChatError> {
        self.verify_session_owner(user.user_id, &session_id).await?;
        self.session_manager
            .list_threads(session_id)
            .await
            .map_err(Into::into)
    }

    async fn send_message(
        &self,
        user: &UserPrincipal,
        session_id: SessionId,
        thread_id: ThreadId,
        message: String,
    ) -> Result<(), UserChatError> {
        self.verify_session_owner(user.user_id, &session_id).await?;
        self.session_manager
            .send_message(session_id, &thread_id, message)
            .await
            .map_err(Into::into)
    }

    async fn subscribe(
        &self,
        user: &UserPrincipal,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<broadcast::Receiver<ThreadEvent>, UserChatError> {
        self.verify_session_owner(user.user_id, &session_id).await?;
        self.session_manager
            .subscribe(session_id, &thread_id)
            .await
            .ok_or(UserChatError::NotFound)
    }
}

fn map_session_summary(summary: SessionWithCount) -> SessionSummary {
    let updated_at = chrono::DateTime::parse_from_rfc3339(&summary.session.updated_at)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());

    SessionSummary {
        id: summary.session.id,
        name: summary.session.name,
        thread_count: summary.thread_count,
        updated_at,
    }
}
