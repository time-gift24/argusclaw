use std::sync::Arc;

use crate::server_core::ServerCore;

#[derive(Clone)]
pub struct AppState {
    core: Arc<ServerCore>,
}

impl AppState {
    #[must_use]
    pub fn new(core: Arc<ServerCore>) -> Self {
        Self { core }
    }

    #[must_use]
    pub fn core(&self) -> &Arc<ServerCore> {
        &self.core
    }
}
