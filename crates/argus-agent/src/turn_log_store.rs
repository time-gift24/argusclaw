use std::path::{Path, PathBuf};

use argus_protocol::{TokenUsage, llm::ChatMessage};
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::TurnLogError;
use crate::history::{
    CompactionCheckpoint, TurnRecord, TurnRecordKind, TurnState, flatten_turn_messages,
};

const THREAD_META_FILE: &str = "thread.meta.json";
const CHECKPOINTS_DIR: &str = "checkpoints";
const LATEST_CHECKPOINT_FILE: &str = "latest.json";
const TURNS_DIR: &str = "turns";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnLogMeta {
    pub turn_number: u32,
    pub state: TurnState,
    pub token_usage: Option<TokenUsage>,
    pub context_token_count: Option<u32>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub model: Option<String>,
    pub error: Option<String>,
}

impl From<&TurnRecord> for TurnLogMeta {
    fn from(turn: &TurnRecord) -> Self {
        Self {
            turn_number: turn.turn_number.unwrap_or(0),
            state: turn.state.clone(),
            token_usage: turn.token_usage.clone(),
            context_token_count: turn.context_token_count,
            started_at: turn.started_at,
            finished_at: turn.finished_at,
            model: turn.model.clone(),
            error: turn.error.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadLogMeta {
    pub system_messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone)]
pub struct TurnLogPersistenceSnapshot {
    pub base_dir: PathBuf,
    pub turn: TurnRecord,
    pub system_messages: Vec<ChatMessage>,
    pub checkpoint: Option<CompactionCheckpoint>,
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

pub fn turn_stem(turn_number: u32) -> String {
    format!("{turn_number:06}")
}

pub fn turn_messages_path(turns_dir: &Path, turn_number: u32) -> PathBuf {
    turns_dir.join(format!("{}.messages.jsonl", turn_stem(turn_number)))
}

pub fn turn_meta_path(turns_dir: &Path, turn_number: u32) -> PathBuf {
    turns_dir.join(format!("{}.meta.json", turn_stem(turn_number)))
}

pub fn thread_meta_path(base_dir: &Path) -> PathBuf {
    base_dir.join(THREAD_META_FILE)
}

pub fn checkpoint_path(base_dir: &Path) -> PathBuf {
    base_dir.join(CHECKPOINTS_DIR).join(LATEST_CHECKPOINT_FILE)
}

pub fn turns_dir(base_dir: &Path) -> PathBuf {
    base_dir.join(TURNS_DIR)
}

pub fn meta_jsonl_path(base_dir: &Path) -> PathBuf {
    turns_dir(base_dir).join("meta.jsonl")
}

fn same_message_sequence(lhs: &[ChatMessage], rhs: &[ChatMessage]) -> bool {
    lhs.len() == rhs.len()
        && lhs.iter().zip(rhs.iter()).all(|(left, right)| {
            match (serde_json::to_string(left), serde_json::to_string(right)) {
                (Ok(left_json), Ok(right_json)) => left_json == right_json,
                _ => false,
            }
        })
}

pub async fn write_turn_messages(path: &Path, messages: &[ChatMessage]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let mut file = fs::File::create(path).await?;
    for message in messages {
        let line = serde_json::to_string(message)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
    }
    file.flush().await?;
    Ok(())
}

pub async fn write_turn_meta(path: &Path, meta: &TurnLogMeta) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let content = serde_json::to_vec_pretty(meta)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    fs::write(path, content).await
}

pub async fn write_thread_meta(path: &Path, meta: &ThreadLogMeta) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let content = serde_json::to_vec_pretty(meta)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    fs::write(path, content).await
}

pub async fn read_thread_meta(path: &Path) -> Result<ThreadLogMeta, TurnLogError> {
    let content = fs::read_to_string(path)
        .await
        .map_err(|_| TurnLogError::TurnNotFound(path.to_path_buf()))?;
    serde_json::from_str(&content).map_err(|error| TurnLogError::MalformedEvent {
        line: 1,
        reason: error.to_string(),
    })
}

pub async fn write_checkpoint(
    path: &Path,
    checkpoint: &CompactionCheckpoint,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let content = serde_json::to_vec_pretty(checkpoint)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    fs::write(path, content).await
}

pub async fn read_checkpoint(path: &Path) -> Result<CompactionCheckpoint, TurnLogError> {
    let content = fs::read_to_string(path)
        .await
        .map_err(|_| TurnLogError::TurnNotFound(path.to_path_buf()))?;
    serde_json::from_str(&content).map_err(|error| TurnLogError::MalformedEvent {
        line: 1,
        reason: error.to_string(),
    })
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

pub async fn persist_turn_log_snapshot(
    snapshot: &TurnLogPersistenceSnapshot,
) -> std::io::Result<()> {
    let turns_dir = turns_dir(&snapshot.base_dir);
    let turn_number = snapshot.turn.turn_number.unwrap_or(0);
    write_turn_messages(
        &turn_messages_path(&turns_dir, turn_number),
        &snapshot.turn.messages,
    )
    .await?;
    write_turn_meta(
        &turn_meta_path(&turns_dir, turn_number),
        &TurnLogMeta::from(&snapshot.turn),
    )
    .await?;
    write_thread_meta(
        &thread_meta_path(&snapshot.base_dir),
        &ThreadLogMeta {
            system_messages: snapshot.system_messages.clone(),
        },
    )
    .await?;
    if let Some(checkpoint) = snapshot.checkpoint.as_ref() {
        write_checkpoint(&checkpoint_path(&snapshot.base_dir), checkpoint).await?;
    }

    // New canonical persistence stream: append-only turns/meta.jsonl.
    let mut recovered = recover_thread_log_state(&snapshot.base_dir)
        .await
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;

    if recovered.turns.is_empty() {
        let bootstrap = TurnRecord::system_bootstrap(0, snapshot.system_messages.clone());
        append_turn_record(&snapshot.base_dir, &bootstrap)
            .await
            .map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
            })?;
        recovered.turns.push(bootstrap);
    }

    let mut next_seq = recovered
        .turns
        .last()
        .map_or(0, |record| record.seq.saturating_add(1));

    let user_turn_number = snapshot.turn.turn_number;
    let user_turn_already_persisted = recovered.turns.iter().any(|record| {
        matches!(record.kind, TurnRecordKind::UserTurn) && record.turn_number == user_turn_number
    });

    if !user_turn_already_persisted {
        let mut persisted_turn = snapshot.turn.clone();
        persisted_turn.kind = TurnRecordKind::UserTurn;
        persisted_turn.seq = next_seq;
        append_turn_record(&snapshot.base_dir, &persisted_turn)
            .await
            .map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
            })?;
        recovered.turns.push(persisted_turn);
        next_seq = next_seq.saturating_add(1);
    }

    if let Some(checkpoint) = snapshot.checkpoint.as_ref() {
        let latest_checkpoint = recovered.turns.iter().rev().find_map(|record| {
            if let TurnRecordKind::Checkpoint { through_turn } = record.kind {
                Some((through_turn, record.messages.as_slice()))
            } else {
                None
            }
        });

        let checkpoint_already_persisted =
            latest_checkpoint.is_some_and(|(through_turn, messages)| {
                through_turn == checkpoint.summarized_through_turn
                    && same_message_sequence(messages, checkpoint.summary_messages.as_slice())
            });

        if !checkpoint_already_persisted {
            let checkpoint_record = TurnRecord::checkpoint(
                next_seq,
                checkpoint.summarized_through_turn,
                checkpoint.summary_messages.clone(),
            );
            append_turn_record(&snapshot.base_dir, &checkpoint_record)
                .await
                .map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
                })?;
        }
    }

    Ok(())
}

pub async fn read_turn_messages(path: &Path) -> Result<Vec<ChatMessage>, TurnLogError> {
    let content = fs::read_to_string(path)
        .await
        .map_err(|_| TurnLogError::TurnNotFound(path.to_path_buf()))?;
    let mut messages = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let message = serde_json::from_str::<ChatMessage>(line).map_err(|error| {
            TurnLogError::MalformedEvent {
                line: index + 1,
                reason: error.to_string(),
            }
        })?;
        messages.push(message);
    }
    Ok(messages)
}

pub async fn read_turn_meta(path: &Path) -> Result<TurnLogMeta, TurnLogError> {
    let content = fs::read_to_string(path)
        .await
        .map_err(|_| TurnLogError::TurnNotFound(path.to_path_buf()))?;
    serde_json::from_str(&content).map_err(|error| TurnLogError::MalformedEvent {
        line: 1,
        reason: error.to_string(),
    })
}

pub async fn read_turn_record(
    turns_dir: &Path,
    turn_number: u32,
) -> Result<Option<TurnRecord>, TurnLogError> {
    let messages_path = turn_messages_path(turns_dir, turn_number);
    let meta_path = turn_meta_path(turns_dir, turn_number);
    let messages_exists = fs::try_exists(&messages_path).await.unwrap_or(false);
    let meta_exists = fs::try_exists(&meta_path).await.unwrap_or(false);

    if !messages_exists && !meta_exists {
        return Ok(None);
    }
    if !messages_exists {
        return Err(TurnLogError::TurnNotFound(messages_path));
    }
    if !meta_exists {
        return Err(TurnLogError::TurnNotFound(meta_path));
    }

    let messages = read_turn_messages(&messages_path).await?;
    let meta = read_turn_meta(&meta_path).await?;

    Ok(Some(TurnRecord {
        seq: 0,
        kind: TurnRecordKind::UserTurn,
        turn_number: Some(meta.turn_number),
        state: meta.state,
        messages,
        token_usage: meta.token_usage,
        context_token_count: meta.context_token_count,
        started_at: meta.started_at,
        finished_at: meta.finished_at,
        model: meta.model,
        error: meta.error,
    }))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn roundtrip_turn_messages_and_meta() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let turns_dir = turns_dir(temp_dir.path());
        let messages = vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")];
        let meta = TurnLogMeta {
            turn_number: 1,
            state: TurnState::Completed,
            token_usage: Some(TokenUsage {
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
            }),
            context_token_count: Some(2),
            started_at: chrono::Utc::now(),
            finished_at: Some(chrono::Utc::now()),
            model: Some("test-model".to_string()),
            error: None,
        };

        write_turn_messages(&turn_messages_path(&turns_dir, 1), &messages)
            .await
            .expect("messages should write");
        write_turn_meta(&turn_meta_path(&turns_dir, 1), &meta)
            .await
            .expect("meta should write");

        let record = read_turn_record(&turns_dir, 1)
            .await
            .expect("record should read")
            .expect("record should exist");
        assert_eq!(record.turn_number, Some(1));
        assert_eq!(record.messages.len(), 2);
        assert_eq!(record.messages[0].content, "hi");
        assert_eq!(record.messages[1].content, "hello");
        assert!(matches!(record.state, TurnState::Completed));
    }

    #[test]
    fn recovered_token_count_falls_back_to_legacy_total_tokens() {
        let recovered = RecoveredThreadLogState {
            turns: vec![TurnRecord {
                seq: 1,
                kind: TurnRecordKind::UserTurn,
                turn_number: Some(1),
                state: TurnState::Completed,
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
            turn: TurnRecord::user_completed(
                0,
                1,
                vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
            ),
            system_messages: vec![ChatMessage::system("sys")],
            checkpoint: None,
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
        // append system, user, checkpoint; recover and assert same sequence
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
        // write invalid log and assert strict error
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

        // Write bootstrap (seq 0)
        let bootstrap = TurnRecord::system_bootstrap(0, vec![ChatMessage::system("sys")]);
        append_turn_record(base_dir, &bootstrap)
            .await
            .expect("bootstrap should append");

        // Write turn with seq 1 (correct order after bootstrap)
        let turn1 = TurnRecord::user_completed(1, 1, vec![ChatMessage::user("turn1")]);
        append_turn_record(base_dir, &turn1)
            .await
            .expect("turn1 should append");

        // Write turn with seq 0 (OUT OF ORDER - seq 0 was already used by bootstrap)
        let turn2 = TurnRecord::user_completed(0, 2, vec![ChatMessage::user("turn2")]);
        append_turn_record(base_dir, &turn2)
            .await
            .expect("turn2 should append");

        let result = recover_thread_log_state(base_dir).await;
        let err = result.expect_err("out-of-order seq should fail");
        assert!(matches!(err, TurnLogError::OutOfOrderSeq { .. }));
        let err_str = err.to_string();
        // Error format is "out-of-order seq at line X: expected Y but found Z"
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

        // Write bootstrap
        let bootstrap = TurnRecord::system_bootstrap(0, vec![ChatMessage::system("sys")]);
        append_turn_record(base_dir, &bootstrap)
            .await
            .expect("bootstrap should append");

        // Write turn 2 first, then turn 1 - should fail
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

        // Write bootstrap
        let bootstrap = TurnRecord::system_bootstrap(0, vec![ChatMessage::system("sys")]);
        append_turn_record(base_dir, &bootstrap)
            .await
            .expect("bootstrap should append");

        // Write checkpoint through turn 5, but only turn 1 exists - should fail
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
