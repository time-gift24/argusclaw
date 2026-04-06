//! Application state shared across all handlers.

use std::sync::Arc;

use argus_repository::UserRepository;

use crate::auth::provider::OAuth2AuthProvider;
use crate::auth::session::AuthSession;
use crate::config::ServerConfig;

/// Shared application state injected into all route handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub auth_provider: Arc<dyn OAuth2AuthProvider>,
    pub user_repo: Arc<dyn UserRepository>,
    pub auth_session: Arc<AuthSession>,
    /// User chat services for the server product.
    /// `None` when chat is not yet configured (e.g., during initial setup).
    pub chat_services: Option<Arc<argus_session::UserChatServices>>,
}
