use std::path::{Path, PathBuf};

use argus_protocol::llm::ChatMessage;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::TurnLogError;
use crate::history::{TurnRecord, TurnRecordKind, flatten_turn_messages};

const TURNS_DIR: &str = "turns";

#[derive(Debug, Clone)]
pub struct TurnLogPersistenceSnapshot {
    pub base_dir: PathBuf,
    /// Turns to append to meta.jsonl (may include SystemBootstrap if not yet persisted).
    pub turns: Vec<TurnRecord>,
}

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
            .iter()
            .rev()
            .find_map(|turn| {
                turn.context_token_count
                    .or_else(|| turn.token_usage.as_ref().map(|usage| usage.total_tokens))
            })
            .unwrap_or(0)
    }

    #[must_use]
    pub fn turn_count(&self) -> u32 {
        self.turns
            .iter()
            .filter(|turn| matches!(turn.kind, TurnRecordKind::UserTurn))
            .filter_map(|turn| turn.turn_number)
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
/// - First record is SystemBootstrap
/// - Seq numbers are strictly increasing
/// - User turn numbers are monotonically increasing
/// - Checkpoint through_turn doesn't exceed history
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
    let mut last_seq: u64 = 0;
    let mut last_user_turn_number: u32 = 0;
    let mut max_turn_number: u32 = 0;
    let mut saw_first_record = false;

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

        // Validate first record is SystemBootstrap
        if !saw_first_record {
            if !matches!(record.kind, TurnRecordKind::SystemBootstrap) {
                return Err(TurnLogError::MalformedEvent {
                    line: index + 1,
                    reason: "first record must be SystemBootstrap".to_string(),
                });
            }
            saw_first_record = true;
            last_seq = record.seq;
            turns.push(record);
            continue;
        }

        // Validate seq is strictly increasing (for non-bootstrap records)
        if record.seq <= last_seq {
            return Err(TurnLogError::OutOfOrderSeq {
                line: index + 1,
                expected: last_seq + 1,
                found: record.seq,
            });
        }
        last_seq = record.seq;

        // Validate user turn numbers are monotonically increasing
        if let Some(turn_number) = record.turn_number {
            if turn_number <= last_user_turn_number {
                return Err(TurnLogError::NonMonotonicTurnNumber {
                    line: index + 1,
                    expected: last_user_turn_number + 1,
                    found: turn_number,
                });
            }
            last_user_turn_number = turn_number;
            max_turn_number = max_turn_number.max(turn_number);
        }

        // Validate checkpoint through_turn doesn't exceed history
        if let TurnRecordKind::Checkpoint { through_turn } = record.kind
            && through_turn > max_turn_number
        {
            return Err(TurnLogError::CheckpointBeyondHistory {
                line: index + 1,
                through_turn,
                turn_count: max_turn_number,
            });
        }

        turns.push(record);
    }
    Ok(RecoveredThreadLogState { turns })
}

/// Persist a turn log snapshot by appending new turn records to meta.jsonl.
///
/// Deduplicates against already-persisted records (SystemBootstrap and UserTurn
/// by turn number).
pub async fn persist_turn_log_snapshot(
    snapshot: &TurnLogPersistenceSnapshot,
) -> std::io::Result<()> {
    let turns_dir = turns_dir(&snapshot.base_dir);
    fs::create_dir_all(&turns_dir).await?;

    let recovered = recover_thread_log_state(&snapshot.base_dir)
        .await
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;

    let mut next_seq = recovered
        .turns
        .last()
        .map_or(0, |record| record.seq.saturating_add(1));

    for turn in &snapshot.turns {
        let already_persisted = recovered.turns.iter().any(|existing| {
            matches!(existing.kind, TurnRecordKind::SystemBootstrap)
                && matches!(turn.kind, TurnRecordKind::SystemBootstrap)
        }) || recovered.turns.iter().any(|existing| {
            matches!(existing.kind, TurnRecordKind::UserTurn)
                && existing.turn_number == turn.turn_number
        });

        if already_persisted {
            continue;
        }

        let mut record = turn.clone();
        record.seq = next_seq;
        next_seq = next_seq.saturating_add(1);
        append_turn_record(&snapshot.base_dir, &record)
            .await
            .map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
            })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use argus_protocol::TokenUsage;

    use super::*;

    #[test]
    fn recovered_token_count_falls_back_to_legacy_total_tokens() {
        let recovered = RecoveredThreadLogState {
            turns: vec![TurnRecord {
                seq: 1,
                kind: TurnRecordKind::UserTurn,
                turn_number: Some(1),
                state: crate::history::TurnState::Completed,
                messages: vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
                token_usage: Some(TokenUsage {
                    input_tokens: 1,
                    output_tokens: 2,
                    total_tokens: 3,
                }),
                context_token_count: None,
                started_at: chrono::Utc::now(),
                finished_at: Some(chrono::Utc::now()),
                model: Some("test-model".to_string()),
                error: None,
            }],
        };

        assert_eq!(recovered.token_count(), 3);
    }

    #[test]
    fn recovered_turn_count_uses_latest_user_turn_when_tail_is_checkpoint() {
        let recovered = RecoveredThreadLogState {
            turns: vec![
                TurnRecord::system_bootstrap(0, vec![ChatMessage::system("sys")]),
                TurnRecord::user_completed(
                    1,
                    1,
                    vec![ChatMessage::user("u1"), ChatMessage::assistant("a1")],
                ),
                TurnRecord::checkpoint(2, 1, vec![ChatMessage::assistant("summary")]),
            ],
        };

        assert_eq!(recovered.turn_count(), 1);
    }

    #[tokio::test]
    async fn persist_turn_log_snapshot_writes_meta_jsonl_replay_stream() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let base_dir = temp_dir.path().join("thread");
        let snapshot = TurnLogPersistenceSnapshot {
            base_dir: base_dir.clone(),
            turns: vec![
                TurnRecord::system_bootstrap(0, vec![ChatMessage::system("sys")]),
                TurnRecord::user_completed(
                    1,
                    1,
                    vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
                ),
            ],
        };

        persist_turn_log_snapshot(&snapshot)
            .await
            .expect("snapshot should persist");

        let recovered = recover_thread_log_state(&base_dir)
            .await
            .expect("meta.jsonl replay should recover");
        assert_eq!(recovered.turns.len(), 2);
        assert!(matches!(
            recovered.turns[0].kind,
            TurnRecordKind::SystemBootstrap
        ));
        assert!(matches!(recovered.turns[1].kind, TurnRecordKind::UserTurn));
        assert_eq!(recovered.turns[1].turn_number, Some(1));
    }

    #[tokio::test]
    async fn append_and_recover_meta_jsonl_roundtrip() {
        use crate::history::TurnRecordKind;
        let temp_dir = tempdir().expect("temp dir should exist");
        let base_dir = temp_dir.path();

        let bootstrap = TurnRecord::system_bootstrap(0, vec![ChatMessage::system("sys")]);
        append_turn_record(base_dir, &bootstrap)
            .await
            .expect("bootstrap should append");

        let user = TurnRecord::user_completed(1, 1, vec![ChatMessage::user("hi")]);
        append_turn_record(base_dir, &user)
            .await
            .expect("user turn should append");

        let checkpoint = TurnRecord::checkpoint(2, 1, vec![ChatMessage::assistant("summary")]);
        append_turn_record(base_dir, &checkpoint)
            .await
            .expect("checkpoint should append");

        let recovered = recover_thread_log_state(base_dir)
            .await
            .expect("recovery should succeed");

        assert_eq!(recovered.turns.len(), 3);
        assert!(matches!(
            recovered.turns[0].kind,
            TurnRecordKind::SystemBootstrap
        ));
        assert!(matches!(recovered.turns[1].kind, TurnRecordKind::UserTurn));
        assert!(matches!(
            &recovered.turns[2].kind,
            TurnRecordKind::Checkpoint { through_turn: 1 }
        ));
        assert_eq!(recovered.turns[1].messages[0].content, "hi");
        assert_eq!(recovered.turns[2].messages[0].content, "summary");
    }

    #[tokio::test]
    async fn recover_fails_when_first_record_is_not_system_bootstrap() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let base_dir = temp_dir.path();
        let turns_dir = turns_dir(base_dir);
        fs::create_dir_all(&turns_dir)
            .await
            .expect("turns dir should exist");
        let meta_path = meta_jsonl_path(base_dir);
        let invalid_record = serde_json::to_string(&TurnRecord::user_completed(
            0,
            0,
            vec![ChatMessage::user("bad")],
        ))
        .unwrap();
        fs::write(&meta_path, format!("{invalid_record}\n"))
            .await
            .expect("invalid log should write");

        let result = recover_thread_log_state(base_dir).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn recover_fails_on_out_of_order_seq() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let base_dir = temp_dir.path();

        let bootstrap = TurnRecord::system_bootstrap(0, vec![ChatMessage::system("sys")]);
        append_turn_record(base_dir, &bootstrap)
            .await
            .expect("bootstrap should append");

        let turn1 = TurnRecord::user_completed(1, 1, vec![ChatMessage::user("turn1")]);
        append_turn_record(base_dir, &turn1)
            .await
            .expect("turn1 should append");

        let turn2 = TurnRecord::user_completed(0, 2, vec![ChatMessage::user("turn2")]);
        append_turn_record(base_dir, &turn2)
            .await
            .expect("turn2 should append");

        let result = recover_thread_log_state(base_dir).await;
        let err = result.expect_err("out-of-order seq should fail");
        assert!(matches!(err, TurnLogError::OutOfOrderSeq { .. }));
        let err_str = err.to_string();
        assert!(
            err_str.contains("expected 2"),
            "expected 'expected 2' in: {}",
            err_str
        );
        assert!(
            err_str.contains("found 0"),
            "expected 'found 0' in: {}",
            err_str
        );
    }

    #[tokio::test]
    async fn recover_fails_on_non_monotonic_user_turn_numbers() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let base_dir = temp_dir.path();

        let bootstrap = TurnRecord::system_bootstrap(0, vec![ChatMessage::system("sys")]);
        append_turn_record(base_dir, &bootstrap)
            .await
            .expect("bootstrap should append");

        let turn2 = TurnRecord::user_completed(1, 2, vec![ChatMessage::user("turn2")]);
        append_turn_record(base_dir, &turn2)
            .await
            .expect("turn2 should append");

        let turn1 = TurnRecord::user_completed(2, 1, vec![ChatMessage::user("turn1")]);
        append_turn_record(base_dir, &turn1)
            .await
            .expect("turn1 should append");

        let result = recover_thread_log_state(base_dir).await;
        let err = result.expect_err("non-monotonic turn numbers should fail");
        assert!(matches!(err, TurnLogError::NonMonotonicTurnNumber { .. }));
        let err_str = err.to_string();
        assert!(err_str.contains("expected turn 3"));
        assert!(err_str.contains("found 1"));
    }

    #[tokio::test]
    async fn recover_fails_on_checkpoint_through_turn_ahead_of_history() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let base_dir = temp_dir.path();

        let bootstrap = TurnRecord::system_bootstrap(0, vec![ChatMessage::system("sys")]);
        append_turn_record(base_dir, &bootstrap)
            .await
            .expect("bootstrap should append");

        let checkpoint = TurnRecord::checkpoint(1, 5, vec![ChatMessage::assistant("summary")]);
        append_turn_record(base_dir, &checkpoint)
            .await
            .expect("checkpoint should append");

        let result = recover_thread_log_state(base_dir).await;
        let err = result.expect_err("checkpoint beyond history should fail");
        assert!(matches!(
            err,
            TurnLogError::CheckpointBeyondHistory {
                through_turn: 5,
                turn_count: 0,
                ..
            }
        ));
    }
}
