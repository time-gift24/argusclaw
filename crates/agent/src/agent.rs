use std::sync::Arc;

use crate::llm::LLMManager;

#[derive(Clone)]
pub struct Agent {
    llm_manager: Arc<LLMManager>,
}

impl Agent {
    #[must_use]
    pub fn new(llm_manager: Arc<LLMManager>) -> Self {
        Self { llm_manager }
    }

    #[must_use]
    pub fn llm_manager(&self) -> Arc<LLMManager> {
        Arc::clone(&self.llm_manager)
    }
}
