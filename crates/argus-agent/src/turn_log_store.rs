use std::path::{Path, PathBuf};

use argus_protocol::llm::ChatMessage;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::TurnLogError;
use crate::history::{TurnRecord, TurnRecordKind, flatten_turn_messages};

const TURNS_DIR: &str = "turns";

#[derive(Debug, Clone)]
pub struct RecoveredThreadLogState {
    pub turns: Vec<TurnRecord>,
}

impl RecoveredThreadLogState {
    #[must_use]
    pub fn committed_messages(&self) -> Vec<ChatMessage> {
        flatten_turn_messages(&self.turns)
    }

    #[must_use]
    pub fn token_count(&self) -> u32 {
        self.turns
            .last()
            .map(|turn| turn.token_usage.total_tokens)
            .unwrap_or(0)
    }

    #[must_use]
    pub fn turn_count(&self) -> u32 {
        self.turns
            .iter()
            .filter(|turn| matches!(turn.kind, TurnRecordKind::UserTurn))
            .map(|turn| turn.turn_number)
            .max()
            .unwrap_or(0)
    }
}

pub fn turns_dir(base_dir: &Path) -> PathBuf {
    base_dir.join(TURNS_DIR)
}

pub fn meta_jsonl_path(base_dir: &Path) -> PathBuf {
    turns_dir(base_dir).join("meta.jsonl")
}

/// Append a single TurnRecord to the append-only meta.jsonl log.
pub async fn append_turn_record(base_dir: &Path, record: &TurnRecord) -> Result<(), TurnLogError> {
    let turns_dir = turns_dir(base_dir);
    fs::create_dir_all(&turns_dir)
        .await
        .map_err(|error| TurnLogError::MalformedEvent {
            line: 0,
            reason: format!("failed to create turns dir: {error}"),
        })?;
    let meta_path = meta_jsonl_path(base_dir);
    let line = serde_json::to_string(record).map_err(|error| TurnLogError::MalformedEvent {
        line: 0,
        reason: format!("failed to serialize turn record: {error}"),
    })?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&meta_path)
        .await
        .map_err(|error| TurnLogError::MalformedEvent {
            line: 0,
            reason: format!("failed to open meta.jsonl: {error}"),
        })?;
    file.write_all(line.as_bytes())
        .await
        .map_err(|error| TurnLogError::MalformedEvent {
            line: 0,
            reason: format!("failed to append to meta.jsonl: {error}"),
        })?;
    file.write_all(b"\n")
        .await
        .map_err(|error| TurnLogError::MalformedEvent {
            line: 0,
            reason: format!("failed to write newline: {error}"),
        })?;
    Ok(())
}

/// Recover thread log state by replaying the append-only meta.jsonl.
/// Validates that:
/// - First record is UserTurn with turn_number = 1
/// - User turn numbers are strictly increasing
/// - Checkpoint turn_number is always 0
pub async fn recover_thread_log_state(
    base_dir: &Path,
) -> Result<RecoveredThreadLogState, TurnLogError> {
    let meta_path = meta_jsonl_path(base_dir);
    if !fs::try_exists(&meta_path).await.unwrap_or(false) {
        return Ok(RecoveredThreadLogState { turns: Vec::new() });
    }
    let content =
        fs::read_to_string(&meta_path)
            .await
            .map_err(|error| TurnLogError::MalformedEvent {
                line: 0,
                reason: format!("failed to read meta.jsonl: {error}"),
            })?;

    let mut turns = Vec::new();
    let mut saw_first_record = false;
    let mut last_user_turn_number: u32 = 0;

    for (index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let record: TurnRecord =
            serde_json::from_str(line).map_err(|error| TurnLogError::MalformedEvent {
                line: index + 1,
                reason: error.to_string(),
            })?;

        if matches!(record.kind, TurnRecordKind::Checkpoint) && record.turn_number != 0 {
            return Err(TurnLogError::MalformedEvent {
                line: index + 1,
                reason: "checkpoint turn_number must be 0".to_string(),
            });
        }

        if !saw_first_record {
            match record.kind {
                TurnRecordKind::UserTurn if record.turn_number == 1 => {
                    last_user_turn_number = 1;
                }
                TurnRecordKind::UserTurn => {
                    return Err(TurnLogError::NonMonotonicTurnNumber {
                        line: index + 1,
                        expected: 1,
                        found: record.turn_number,
                    });
                }
                TurnRecordKind::Checkpoint => {
                    return Err(TurnLogError::MalformedEvent {
                        line: index + 1,
                        reason: "first record must be UserTurn(turn_number=1)".to_string(),
                    });
                }
            }
            saw_first_record = true;
            turns.push(record);
            continue;
        }

        if matches!(record.kind, TurnRecordKind::UserTurn) {
            let expected = last_user_turn_number.saturating_add(1);
            if record.turn_number != expected {
                return Err(TurnLogError::NonMonotonicTurnNumber {
                    line: index + 1,
                    expected,
                    found: record.turn_number,
                });
            }
            last_user_turn_number = record.turn_number;
        }

        turns.push(record);
    }

    Ok(RecoveredThreadLogState { turns })
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use argus_protocol::TokenUsage;

    use super::*;

    fn usage(total_tokens: u32) -> TokenUsage {
        TokenUsage {
            input_tokens: total_tokens.saturating_sub(1),
            output_tokens: 1,
            total_tokens,
        }
    }

    #[test]
    fn recovered_token_count_returns_last_record_usage() {
        let recovered = RecoveredThreadLogState {
            turns: vec![
                TurnRecord::user_turn(
                    1,
                    vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
                    usage(3),
                ),
                TurnRecord::checkpoint(vec![ChatMessage::assistant("summary")], usage(9)),
            ],
        };

        assert_eq!(recovered.token_count(), 9);
    }

    #[test]
    fn recovered_turn_count_uses_latest_user_turn_when_tail_is_checkpoint() {
        let recovered = RecoveredThreadLogState {
            turns: vec![
                TurnRecord::user_turn(
                    1,
                    vec![ChatMessage::user("u1"), ChatMessage::assistant("a1")],
                    usage(2),
                ),
                TurnRecord::checkpoint(vec![ChatMessage::assistant("summary")], usage(7)),
            ],
        };

        assert_eq!(recovered.turn_count(), 1);
    }

    #[tokio::test]
    async fn append_and_recover_meta_jsonl_roundtrip() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let base_dir = temp_dir.path();

        let turn1 = TurnRecord::user_turn(
            1,
            vec![
                ChatMessage::system("sys"),
                ChatMessage::user("hi"),
                ChatMessage::assistant("hello"),
            ],
            usage(3),
        );
        append_turn_record(base_dir, &turn1)
            .await
            .expect("turn1 should append");

        let checkpoint = TurnRecord::checkpoint(vec![ChatMessage::assistant("summary")], usage(7));
        append_turn_record(base_dir, &checkpoint)
            .await
            .expect("checkpoint should append");

        let turn2 = TurnRecord::user_turn(
            2,
            vec![ChatMessage::user("next"), ChatMessage::assistant("reply")],
            usage(4),
        );
        append_turn_record(base_dir, &turn2)
            .await
            .expect("turn2 should append");

        let recovered = recover_thread_log_state(base_dir)
            .await
            .expect("recovery should succeed");

        assert_eq!(recovered.turns.len(), 3);
        assert!(matches!(recovered.turns[0].kind, TurnRecordKind::UserTurn));
        assert!(matches!(
            recovered.turns[1].kind,
            TurnRecordKind::Checkpoint
        ));
        assert!(matches!(recovered.turns[2].kind, TurnRecordKind::UserTurn));
        assert_eq!(recovered.turns[0].turn_number, 1);
        assert_eq!(recovered.turns[1].turn_number, 0);
        assert_eq!(recovered.turns[2].turn_number, 2);
        assert_eq!(recovered.turns[0].messages[0].content, "sys");
        assert_eq!(recovered.turns[1].messages[0].content, "summary");
    }

    #[tokio::test]
    async fn recover_fails_when_first_record_is_checkpoint() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let base_dir = temp_dir.path();

        append_turn_record(
            base_dir,
            &TurnRecord::checkpoint(vec![ChatMessage::assistant("summary")], usage(4)),
        )
        .await
        .expect("checkpoint should append");

        let result = recover_thread_log_state(base_dir).await;
        let err = result.expect_err("checkpoint-first log should fail");
        assert!(matches!(err, TurnLogError::MalformedEvent { .. }));
    }

    #[tokio::test]
    async fn recover_fails_when_first_user_turn_does_not_start_at_one() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let base_dir = temp_dir.path();

        append_turn_record(
            base_dir,
            &TurnRecord::user_turn(2, vec![ChatMessage::user("bad")], usage(1)),
        )
        .await
        .expect("invalid turn should append");

        let result = recover_thread_log_state(base_dir).await;
        let err = result.expect_err("non-one first turn should fail");
        assert!(matches!(
            err,
            TurnLogError::NonMonotonicTurnNumber {
                expected: 1,
                found: 2,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn recover_fails_on_non_monotonic_user_turn_numbers() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let base_dir = temp_dir.path();

        append_turn_record(
            base_dir,
            &TurnRecord::user_turn(1, vec![ChatMessage::user("turn1")], usage(2)),
        )
        .await
        .expect("turn1 should append");
        append_turn_record(
            base_dir,
            &TurnRecord::user_turn(3, vec![ChatMessage::user("turn3")], usage(4)),
        )
        .await
        .expect("turn3 should append");

        let result = recover_thread_log_state(base_dir).await;
        let err = result.expect_err("non-monotonic turn numbers should fail");
        assert!(matches!(
            err,
            TurnLogError::NonMonotonicTurnNumber {
                expected: 2,
                found: 3,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn recover_fails_when_checkpoint_has_non_zero_turn_number() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let base_dir = temp_dir.path();

        append_turn_record(
            base_dir,
            &TurnRecord::user_turn(1, vec![ChatMessage::user("turn1")], usage(2)),
        )
        .await
        .expect("turn1 should append");

        let invalid_checkpoint = TurnRecord {
            kind: TurnRecordKind::Checkpoint,
            turn_number: 9,
            messages: vec![ChatMessage::assistant("summary")],
            token_usage: usage(5),
            started_at: chrono::Utc::now(),
            finished_at: chrono::Utc::now(),
        };
        append_turn_record(base_dir, &invalid_checkpoint)
            .await
            .expect("invalid checkpoint should append");

        let result = recover_thread_log_state(base_dir).await;
        let err = result.expect_err("checkpoint turn number should fail");
        assert!(matches!(err, TurnLogError::MalformedEvent { .. }));
    }
}
