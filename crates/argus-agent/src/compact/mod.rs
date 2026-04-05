use argus_protocol::{TokenUsage, llm::ChatMessage};
use async_trait::async_trait;

use crate::error::CompactError;

pub mod thread;
pub mod turn;

/// Result of a successful compaction.
#[derive(Debug, Clone)]
pub struct CompactResult {
    /// Compacted messages that become the new context snapshot.
    pub messages: Vec<ChatMessage>,
    /// Authoritative token count for the compaction request + summary response.
    pub token_usage: TokenUsage,
}

/// Shared compactor trait for thread and turn compaction implementations.
#[async_trait]
pub trait Compactor: Send + Sync {
    /// Attempt compaction. Returns `Some(CompactResult)` if compaction occurred,
    /// `None` if compaction was not needed.
    async fn compact(
        &self,
        messages: &[ChatMessage],
        token_count: u32,
    ) -> Result<Option<CompactResult>, CompactError>;

    /// Name of the compactor strategy.
    fn name(&self) -> &'static str;
}
