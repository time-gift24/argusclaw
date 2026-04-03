use std::path::{Path, PathBuf};

use argus_protocol::{TokenUsage, llm::ChatMessage};
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::TurnLogError;
use crate::history::{CompactionCheckpoint, TurnRecord, TurnRecordKind, TurnState, flatten_turn_messages};

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
    pub system_messages: Vec<ChatMessage>,
    pub turns: Vec<TurnRecord>,
    pub checkpoint: Option<CompactionCheckpoint>,
}

impl RecoveredThreadLogState {
    #[must_use]
    pub fn committed_messages(&self) -> Vec<ChatMessage> {
        let mut messages = self.system_messages.clone();
        messages.extend(flatten_turn_messages(&self.turns));
        messages
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
        self.turns.last().map_or(0, |turn| turn.turn_number.unwrap_or(0))
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

pub async fn recover_thread_log_state(
    base_dir: &Path,
    turn_count_hint: Option<u32>,
) -> Result<RecoveredThreadLogState, TurnLogError> {
    let turns_dir = turns_dir(base_dir);
    let turn_numbers = resolve_turn_numbers(&turns_dir, turn_count_hint).await?;
    let mut turns = Vec::with_capacity(turn_numbers.len());

    for turn_number in turn_numbers {
        let turn = read_turn_record(&turns_dir, turn_number)
            .await?
            .ok_or_else(|| {
                TurnLogError::TurnNotFound(turn_messages_path(&turns_dir, turn_number))
            })?;
        turns.push(turn);
    }

    let system_messages = {
        let path = thread_meta_path(base_dir);
        if fs::try_exists(&path).await.unwrap_or(false) {
            read_thread_meta(&path).await?.system_messages
        } else {
            Vec::new()
        }
    };
    let checkpoint = {
        let path = checkpoint_path(base_dir);
        if fs::try_exists(&path).await.unwrap_or(false) {
            Some(read_checkpoint(&path).await?)
        } else {
            None
        }
    };

    Ok(RecoveredThreadLogState {
        system_messages,
        turns,
        checkpoint,
    })
}

async fn resolve_turn_numbers(
    turns_dir: &Path,
    turn_count_hint: Option<u32>,
) -> Result<Vec<u32>, TurnLogError> {
    if !fs::try_exists(turns_dir).await.unwrap_or(false) {
        if turn_count_hint.unwrap_or(0) > 0 {
            return Err(TurnLogError::TurnNotFound(turn_messages_path(turns_dir, 1)));
        }
        return Ok(Vec::new());
    }

    let mut entries =
        fs::read_dir(turns_dir)
            .await
            .map_err(|error| TurnLogError::MalformedEvent {
                line: 1,
                reason: format!(
                    "failed to inspect trace turns directory {}: {error}",
                    turns_dir.display()
                ),
            })?;
    let mut turn_numbers = Vec::new();

    while let Some(entry) =
        entries
            .next_entry()
            .await
            .map_err(|error| TurnLogError::MalformedEvent {
                line: 1,
                reason: format!(
                    "failed to inspect trace turns directory {}: {error}",
                    turns_dir.display()
                ),
            })?
    {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if let Some(stem) = file_name.strip_suffix(".messages.jsonl") {
            let turn_number =
                stem.parse::<u32>()
                    .map_err(|error| TurnLogError::MalformedEvent {
                        line: 1,
                        reason: format!(
                            "failed to parse turn trace filename {}: {error}",
                            path.display()
                        ),
                    })?;
            turn_numbers.push(turn_number);
        }
    }

    turn_numbers.sort_unstable();
    turn_numbers.dedup();

    let highest_discovered_turn = turn_numbers.last().copied().unwrap_or(0);
    let highest_expected_turn = highest_discovered_turn.max(turn_count_hint.unwrap_or(0));
    for expected in 1..=highest_expected_turn {
        if turn_numbers.get((expected - 1) as usize).copied() != Some(expected) {
            return Err(TurnLogError::TurnNotFound(turn_messages_path(
                turns_dir, expected,
            )));
        }
    }

    Ok((1..=highest_expected_turn).collect())
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

    #[tokio::test]
    async fn recover_thread_log_state_restores_system_messages_turns_and_checkpoint() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let base_dir = temp_dir.path().join("thread");
        let turn = TurnRecord {
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
            context_token_count: Some(3),
            started_at: chrono::Utc::now(),
            finished_at: Some(chrono::Utc::now()),
            model: Some("test-model".to_string()),
            error: None,
        };
        let checkpoint = CompactionCheckpoint {
            summarized_through_turn: 1,
            summary_messages: vec![ChatMessage::assistant("summary")],
            created_at: chrono::Utc::now(),
            token_count_stale: false,
        };

        persist_turn_log_snapshot(&TurnLogPersistenceSnapshot {
            base_dir: base_dir.clone(),
            turn: turn.clone(),
            system_messages: vec![ChatMessage::system("persisted system")],
            checkpoint: Some(checkpoint.clone()),
        })
        .await
        .expect("turn log snapshot should persist");

        let recovered = recover_thread_log_state(&base_dir, Some(1))
            .await
            .expect("turn log state should recover");

        assert_eq!(recovered.system_messages.len(), 1);
        assert_eq!(recovered.system_messages[0].content, "persisted system");
        assert_eq!(recovered.turns.len(), 1);
        assert_eq!(recovered.turns[0].turn_number, Some(1));
        assert_eq!(recovered.turns[0].messages.len(), 2);
        assert_eq!(
            recovered.turns[0].messages[0].content,
            turn.messages[0].content
        );
        assert_eq!(
            recovered.turns[0].messages[1].content,
            turn.messages[1].content
        );
        let recovered_checkpoint = recovered
            .checkpoint
            .as_ref()
            .expect("checkpoint should recover");
        assert_eq!(recovered_checkpoint.summarized_through_turn, 1);
        assert_eq!(recovered_checkpoint.summary_messages.len(), 1);
        assert_eq!(
            recovered_checkpoint.summary_messages[0].content,
            checkpoint.summary_messages[0].content
        );
        assert_eq!(recovered.turn_count(), 1);
        assert_eq!(recovered.token_count(), 3);
        assert_eq!(recovered.committed_messages().len(), 3);
        assert_eq!(
            recovered.committed_messages()[0].content,
            "persisted system"
        );
    }

    #[test]
    fn recovered_token_count_falls_back_to_legacy_total_tokens() {
        let recovered = RecoveredThreadLogState {
            system_messages: Vec::new(),
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
            checkpoint: None,
        };

        assert_eq!(recovered.token_count(), 3);
    }
}
