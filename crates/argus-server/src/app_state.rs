use std::sync::Arc;

use crate::auth::AuthState;
use crate::server_core::ServerCore;

#[derive(Clone)]
pub struct AppState {
    core: Arc<ServerCore>,
    auth: AuthState,
}

impl AppState {
    #[must_use]
    pub fn new(core: Arc<ServerCore>) -> Self {
        Self::with_auth(core, AuthState::disabled())
    }

    #[must_use]
    pub fn with_auth(core: Arc<ServerCore>, auth: AuthState) -> Self {
        Self { core, auth }
    }

    #[must_use]
    pub fn core(&self) -> &Arc<ServerCore> {
        &self.core
    }

    #[must_use]
    pub fn auth(&self) -> &AuthState {
        &self.auth
    }
}
