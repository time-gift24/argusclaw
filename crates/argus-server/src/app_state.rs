use std::sync::Arc;

use argus_wing::ArgusWing;

#[derive(Clone)]
pub struct AppState {
    wing: Arc<ArgusWing>,
}

impl AppState {
    #[must_use]
    pub fn new(wing: Arc<ArgusWing>) -> Self {
        Self { wing }
    }

    #[must_use]
    pub fn wing(&self) -> &Arc<ArgusWing> {
        &self.wing
    }
}
