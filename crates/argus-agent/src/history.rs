use argus_protocol::{llm::ChatMessage, TokenUsage};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnState {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct InFlightTurn {
    pub turn_number: u32,
    pub state: InFlightTurnPhase,
    pub pending_messages: Vec<ChatMessage>,
    pub token_usage: TokenUsage,
    pub started_at: DateTime<Utc>,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
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

#[cfg(test)]
mod tests {
    use argus_protocol::llm::ChatMessage;

    use super::{flatten_turn_messages, TurnRecord};

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
}
