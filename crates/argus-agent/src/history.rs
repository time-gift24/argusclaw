use argus_protocol::{TokenUsage, llm::ChatMessage};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TurnRecordKind {
    UserTurn,
    Checkpoint,
    TurnCheckpoint,
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

    pub fn turn_checkpoint(
        turn_number: u32,
        messages: Vec<ChatMessage>,
        token_usage: TokenUsage,
    ) -> Self {
        let now = Utc::now();
        Self::turn_checkpoint_with_times(turn_number, messages, token_usage, now, now)
    }

    pub fn turn_checkpoint_with_times(
        turn_number: u32,
        messages: Vec<ChatMessage>,
        token_usage: TokenUsage,
        started_at: DateTime<Utc>,
        finished_at: DateTime<Utc>,
    ) -> Self {
        Self {
            kind: TurnRecordKind::TurnCheckpoint,
            turn_number,
            messages,
            token_usage,
            started_at,
            finished_at,
        }
    }
}

pub fn derive_next_user_turn_number(turns: &[TurnRecord]) -> u32 {
    turns
        .iter()
        .filter(|t| {
            matches!(
                t.kind,
                TurnRecordKind::UserTurn | TurnRecordKind::TurnCheckpoint
            )
        })
        .map(|t| t.turn_number)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

pub fn flatten_turn_messages(turns: &[TurnRecord]) -> Vec<ChatMessage> {
    turns
        .iter()
        .filter(|turn| {
            matches!(
                turn.kind,
                TurnRecordKind::UserTurn | TurnRecordKind::TurnCheckpoint
            )
        })
        .flat_map(|turn| turn.messages.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use argus_protocol::TokenUsage;
    use argus_protocol::llm::ChatMessage;

    use super::{TurnRecord, TurnRecordKind, derive_next_user_turn_number, flatten_turn_messages};

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

    #[test]
    fn user_turn_number_derivation_counts_turn_checkpoints() {
        let records = vec![
            TurnRecord::turn_checkpoint(
                1,
                vec![ChatMessage::user("snapshot")],
                TokenUsage {
                    input_tokens: 2,
                    output_tokens: 1,
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

    #[test]
    fn flatten_committed_messages_includes_turn_checkpoint_records() {
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
            TurnRecord::turn_checkpoint(
                2,
                vec![
                    ChatMessage::user("snapshot"),
                    ChatMessage::assistant("state"),
                ],
                TokenUsage {
                    input_tokens: 2,
                    output_tokens: 2,
                    total_tokens: 4,
                },
            ),
        ];

        let flattened = flatten_turn_messages(&turns);

        assert_eq!(flattened.len(), 4);
        assert!(
            turns
                .iter()
                .any(|turn| matches!(turn.kind, TurnRecordKind::TurnCheckpoint))
        );
        assert_eq!(flattened[0].content, "hi");
        assert_eq!(flattened[1].content, "hello");
        assert_eq!(flattened[2].content, "snapshot");
        assert_eq!(flattened[3].content, "state");
    }
}
