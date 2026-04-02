use std::sync::Arc;

use argus_protocol::HookHandler;
use argus_protocol::tool::NamedTool;
use argus_protocol::{TokenUsage, llm::ChatMessage};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnState {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnRecord {
    pub turn_number: u32,
    pub state: TurnState,
    pub messages: Vec<ChatMessage>,
    pub token_usage: Option<TokenUsage>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub model: Option<String>,
    pub error: Option<String>,
}

impl TurnRecord {
    pub fn completed(turn_number: u32, messages: Vec<ChatMessage>) -> Self {
        let started_at = Utc::now();
        Self {
            turn_number,
            state: TurnState::Completed,
            messages,
            token_usage: None,
            started_at,
            finished_at: Some(Utc::now()),
            model: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum InFlightTurnPhase {
    CallingLlm,
    ExecutingTools,
    WaitingApproval,
}

#[derive(Clone, Default)]
pub struct InFlightTurnShared {
    pub history: Arc<Vec<ChatMessage>>,
    pub tools: Arc<Vec<Arc<dyn NamedTool>>>,
    pub hooks: Arc<Vec<Arc<dyn HookHandler>>>,
}

impl std::fmt::Debug for InFlightTurnShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InFlightTurnShared")
            .field("history_len", &self.history.len())
            .field("tool_count", &self.tools.len())
            .field("hook_count", &self.hooks.len())
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct InFlightTurn {
    pub turn_number: u32,
    pub state: InFlightTurnPhase,
    pub pending_messages: Vec<ChatMessage>,
    pub token_usage: TokenUsage,
    pub started_at: DateTime<Utc>,
    pub model: Option<String>,
    pub shared: Arc<InFlightTurnShared>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionCheckpoint {
    pub summarized_through_turn: u32,
    pub summary_messages: Vec<ChatMessage>,
    pub created_at: DateTime<Utc>,
}

pub fn flatten_turn_messages(turns: &[TurnRecord]) -> Vec<ChatMessage> {
    turns
        .iter()
        .flat_map(|turn| turn.messages.iter().cloned())
        .collect()
}

pub fn shared_history<'a>(
    flat_messages: &'a Arc<Vec<ChatMessage>>,
    cached_committed_messages: Option<&'a Arc<Vec<ChatMessage>>>,
) -> &'a Arc<Vec<ChatMessage>> {
    cached_committed_messages.unwrap_or(flat_messages)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use argus_protocol::llm::ChatMessage;

    use super::{TurnRecord, flatten_turn_messages, shared_history};

    #[test]
    fn flatten_committed_messages_skips_inflight_turn() {
        let turns = vec![
            TurnRecord::completed(
                1,
                vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
            ),
            TurnRecord::completed(
                2,
                vec![
                    ChatMessage::user("search"),
                    ChatMessage::assistant("working"),
                ],
            ),
        ];

        let flattened = flatten_turn_messages(&turns);

        assert_eq!(flattened.len(), 4);
        assert_eq!(flattened[0].content, "hi");
        assert_eq!(flattened[3].content, "working");
    }

    #[test]
    fn shared_history_prefers_cached_committed_messages() {
        let flat_messages = Arc::new(vec![ChatMessage::user("stale")]);
        let cached_messages = Arc::new(vec![ChatMessage::user("fresh")]);

        let history = shared_history(&flat_messages, Some(&cached_messages));

        assert_eq!(history[0].content, "fresh");
    }
}
