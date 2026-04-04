use std::sync::Arc;

use argus_protocol::HookHandler;
use argus_protocol::tool::NamedTool;
use argus_protocol::{TokenUsage, llm::ChatMessage};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TurnRecordKind {
    UserTurn,
    Checkpoint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnRecord {
    pub kind: TurnRecordKind,
    pub turn_number: u32,
    pub messages: Vec<ChatMessage>,
    pub token_usage: TokenUsage,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
}

impl TurnRecord {
    pub fn user_turn(
        turn_number: u32,
        messages: Vec<ChatMessage>,
        token_usage: TokenUsage,
    ) -> Self {
        let now = Utc::now();
        Self::user_turn_with_times(turn_number, messages, token_usage, now, now)
    }

    pub fn user_turn_with_times(
        turn_number: u32,
        messages: Vec<ChatMessage>,
        token_usage: TokenUsage,
        started_at: DateTime<Utc>,
        finished_at: DateTime<Utc>,
    ) -> Self {
        Self {
            kind: TurnRecordKind::UserTurn,
            turn_number,
            messages,
            token_usage,
            started_at,
            finished_at,
        }
    }

    pub fn checkpoint(messages: Vec<ChatMessage>, token_usage: TokenUsage) -> Self {
        let now = Utc::now();
        Self::checkpoint_with_times(messages, token_usage, now, now)
    }

    pub fn checkpoint_with_times(
        messages: Vec<ChatMessage>,
        token_usage: TokenUsage,
        started_at: DateTime<Utc>,
        finished_at: DateTime<Utc>,
    ) -> Self {
        Self {
            kind: TurnRecordKind::Checkpoint,
            turn_number: 0,
            messages,
            token_usage,
            started_at,
            finished_at,
        }
    }
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

impl InFlightTurnShared {
    pub(crate) fn resolved_tools(&self) -> Vec<Arc<dyn NamedTool>> {
        self.tools.iter().cloned().collect()
    }

    pub(crate) fn resolved_hooks(&self) -> Vec<Arc<dyn HookHandler>> {
        self.hooks.iter().cloned().collect()
    }

    pub(crate) fn find_tool(&self, tool_name: &str) -> Option<Arc<dyn NamedTool>> {
        self.resolved_tools()
            .into_iter()
            .find(|tool| tool.name() == tool_name)
    }
}

#[derive(Debug, Clone)]
pub struct InFlightTurn {
    pub turn_number: u32,
    pub pending_messages: Vec<ChatMessage>,
    pub started_at: DateTime<Utc>,
}

pub fn derive_next_user_turn_number(turns: &[TurnRecord]) -> u32 {
    turns
        .iter()
        .filter(|t| matches!(t.kind, TurnRecordKind::UserTurn))
        .map(|t| t.turn_number)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

pub fn flatten_turn_messages(turns: &[TurnRecord]) -> Vec<ChatMessage> {
    turns
        .iter()
        .filter(|turn| matches!(turn.kind, TurnRecordKind::UserTurn))
        .flat_map(|turn| turn.messages.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use argus_protocol::TokenUsage;
    use argus_protocol::llm::ChatMessage;

    use super::{TurnRecord, derive_next_user_turn_number, flatten_turn_messages};

    #[test]
    fn flatten_committed_messages_skips_checkpoint_records() {
        let turns = vec![
            TurnRecord::user_turn(
                1,
                vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
                TokenUsage {
                    input_tokens: 1,
                    output_tokens: 1,
                    total_tokens: 2,
                },
            ),
            TurnRecord::checkpoint(
                vec![ChatMessage::assistant("summary")],
                TokenUsage {
                    input_tokens: 5,
                    output_tokens: 2,
                    total_tokens: 7,
                },
            ),
            TurnRecord::user_turn(
                2,
                vec![
                    ChatMessage::user("search"),
                    ChatMessage::assistant("working"),
                ],
                TokenUsage {
                    input_tokens: 3,
                    output_tokens: 1,
                    total_tokens: 4,
                },
            ),
        ];

        let flattened = flatten_turn_messages(&turns);

        assert_eq!(flattened.len(), 4);
        assert_eq!(flattened[0].content, "hi");
        assert_eq!(flattened[3].content, "working");
    }

    #[test]
    fn user_turn_number_derivation_ignores_checkpoint_records() {
        let records = vec![
            TurnRecord::user_turn(
                1,
                vec![ChatMessage::system("sys"), ChatMessage::user("u1")],
                TokenUsage {
                    input_tokens: 1,
                    output_tokens: 2,
                    total_tokens: 3,
                },
            ),
            TurnRecord::checkpoint(
                vec![ChatMessage::assistant("summary")],
                TokenUsage {
                    input_tokens: 4,
                    output_tokens: 1,
                    total_tokens: 5,
                },
            ),
        ];

        assert_eq!(derive_next_user_turn_number(&records), 2);
    }
}
