//! Application state shared across handlers.

use std::sync::Arc;

use argus_repository::UserRepository;
use argus_session::UserChatApi;

use crate::auth::provider::OAuth2AuthProvider;
use crate::auth::session::AuthSession;
use crate::config::ServerConfig;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub auth_provider: Arc<dyn OAuth2AuthProvider>,
    pub user_repo: Arc<dyn UserRepository>,
    pub auth_session: Arc<AuthSession>,
    pub chat_services: Arc<dyn UserChatApi>,
}
