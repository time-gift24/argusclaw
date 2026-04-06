//! User-facing chat service boundary for server product.
//!
//! This service wraps `SessionManager` and `TemplateManager` to provide
//! user-aware operations. Desktop continues to use `SessionManager` directly
//! without passing through this layer.
//!
//! Ownership tracking:
//! - In-memory `DashMap` maps sessions to their owner `user_id`.
//! - For the server product, future PostgreSQL repository implementations
//!   will persist `user_id` at the data level.

use std::sync::Arc;

use argus_protocol::{
    AgentId, ArgusError, ProviderId, Result, SessionId, ThreadEvent, ThreadId,
};
use dashmap::DashMap;
use tokio::sync::broadcast;

use crate::session::{SessionSummary, ThreadSummary};
use crate::SessionManager;

/// Authenticated principal for server user operations.
///
/// Created by the server auth middleware after OAuth2 login.
/// Desktop does not use this type.
#[derive(Debug, Clone)]
pub struct UserPrincipal {
    /// Database-assigned user ID.
    pub user_id: i64,
    /// Account identifier (e.g., email).
    pub account: String,
    /// Human-readable display name.
    pub display_name: String,
}

/// Error type for user chat service operations.
#[derive(Debug, thiserror::Error)]
pub enum UserChatError {
    /// The user does not own the requested session.
    #[error("session {session_id} not found or access denied for user {user_id}")]
    SessionNotFound {
        user_id: i64,
        session_id: SessionId,
    },

    /// The user does not own the requested thread.
    #[error("thread {thread_id} not found or access denied for user {user_id}")]
    ThreadNotFound {
        user_id: i64,
        thread_id: ThreadId,
    },

    /// The requested agent is not enabled.
    #[error("agent {agent_id} is not enabled")]
    AgentNotEnabled { agent_id: AgentId },

    /// An underlying service error.
    #[error("{0}")]
    ServiceError(#[from] ArgusError),
}

/// User-facing chat service boundary.
///
/// Provides user-aware session, thread, and messaging operations.
/// Desktop uses `SessionManager` directly; server uses this wrapper.
pub struct UserChatServices {
    session_manager: Arc<SessionManager>,
    session_owners: DashMap<SessionId, i64>,
}

impl UserChatServices {
    /// Create a new `UserChatServices` wrapping the given managers.
    pub fn new(session_manager: Arc<SessionManager>) -> Self {
        Self {
            session_manager,
            session_owners: DashMap::new(),
        }
    }

    /// List all enabled agents visible to server users.
    pub async fn list_enabled_agents(&self) -> Vec<argus_protocol::AgentRecord> {
        let agents = self.session_manager.list_templates_for_user().await;
        agents.into_iter().filter(|agent| agent.is_enabled).collect()
    }

    /// Create a new session owned by the given user.
    pub async fn create_session(
        &self,
        user: &UserPrincipal,
        name: &str,
    ) -> Result<SessionId> {
        let session_id = self.session_manager.create(name.to_string()).await?;
        self.session_owners.insert(session_id, user.user_id);
        Ok(session_id)
    }

    /// List sessions owned by the given user.
    pub async fn list_sessions(&self, user: &UserPrincipal) -> Result<Vec<SessionSummary>> {
        let all = self.session_manager.list_sessions().await?;
        Ok(all
            .into_iter()
            .filter(|session| {
                self.session_owners
                    .get(&session.id)
                    .map(|entry| *entry.value() == user.user_id)
                    .unwrap_or(false)
            })
            .collect())
    }

    /// List threads in a session owned by the given user.
    pub async fn list_threads(
        &self,
        user: &UserPrincipal,
        session_id: SessionId,
    ) -> Result<Vec<ThreadSummary>> {
        self.verify_session_owner(user.user_id, session_id)?;
        self.session_manager.list_threads(session_id).await
    }

    /// Create a thread in a session owned by the given user.
    pub async fn create_thread(
        &self,
        user: &UserPrincipal,
        session_id: SessionId,
        template_id: AgentId,
        provider_id: Option<ProviderId>,
        model_override: Option<&str>,
    ) -> Result<ThreadId> {
        self.verify_session_owner(user.user_id, session_id)?;
        self.session_manager
            .create_thread(session_id, template_id, provider_id, model_override)
            .await
    }

    /// Get a thread snapshot for a session owned by the given user.
    pub async fn get_thread_snapshot(
        &self,
        user: &UserPrincipal,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<(Vec<argus_protocol::llm::ChatMessage>, u32, u32, u32)> {
        self.verify_session_owner(user.user_id, session_id)?;
        self.session_manager
            .get_thread_snapshot(session_id, &thread_id)
            .await
    }

    /// Send a message to a thread in a session owned by the given user.
    pub async fn send_message(
        &self,
        user: &UserPrincipal,
        session_id: SessionId,
        thread_id: ThreadId,
        message: String,
    ) -> Result<()> {
        self.verify_session_owner(user.user_id, session_id)?;
        self.session_manager
            .send_message(session_id, &thread_id, message)
            .await
    }

    /// Cancel work on a thread in a session owned by the given user.
    pub async fn cancel_work(
        &self,
        user: &UserPrincipal,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<()> {
        self.verify_session_owner(user.user_id, session_id)?;
        self.session_manager
            .cancel_thread(session_id, &thread_id)
            .await
    }

    /// Subscribe to thread events for a session owned by the given user.
    pub async fn subscribe(
        &self,
        user: &UserPrincipal,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Option<broadcast::Receiver<ThreadEvent>> {
        if self.verify_session_owner(user.user_id, session_id).is_err() {
            return None;
        }
        self.session_manager.subscribe(session_id, &thread_id).await
    }

    fn verify_session_owner(
        &self,
        user_id: i64,
        session_id: SessionId,
    ) -> Result<()> {
        let is_owner = self
            .session_owners
            .get(&session_id)
            .map(|entry| *entry.value() == user_id)
            .unwrap_or(false);
        if is_owner {
            Ok(())
        } else {
            Err(ArgusError::DatabaseError {
                reason: format!("session {} not found or access denied for user {}", session_id, user_id),
            })
        }
    }
}
