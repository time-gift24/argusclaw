use argus_protocol::llm::ChatMessage;
use async_trait::async_trait;

use crate::error::CompactError;

#[derive(Debug, Clone)]
pub struct TurnCompactResult {
    pub checkpoint_messages: Vec<ChatMessage>,
}

#[async_trait]
pub trait TurnCompactor: Send + Sync {
    async fn compact(
        &self,
        system_prompt: &str,
        history: &[ChatMessage],
        turn_messages: &[ChatMessage],
    ) -> Result<Option<TurnCompactResult>, CompactError>;

    fn name(&self) -> &'static str;
}
