use std::sync::Arc;

use argus_wing::ArgusWing;

/// Shared application state injected into all axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub wing: Arc<ArgusWing>,
}

impl AppState {
    pub fn new(wing: Arc<ArgusWing>) -> Self {
        Self { wing }
    }
}
