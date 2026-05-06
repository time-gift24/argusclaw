use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use argus_protocol::TokenUsage;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

const TURN_EVENTS_JSONL_FILE: &str = "turn_events.jsonl";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TurnEventCursor(u64);

impl TurnEventCursor {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnTraceEvent {
    pub turn_number: u32,
    pub cursor: TurnEventCursor,
    pub created_at: DateTime<Utc>,
    pub payload: TurnTraceEventPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TurnTraceEventPayload {
    ContentDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ToolCallDelta {
        index: usize,
        call_id: Option<String>,
        name: Option<String>,
        arguments_delta: Option<String>,
    },
    ToolStarted {
        call_id: String,
        name: String,
        arguments: Value,
    },
    ToolCompleted {
        call_id: String,
        name: String,
        result: Value,
        is_error: bool,
    },
    TurnCompleted {
        token_usage: TokenUsage,
    },
    TurnSettled,
    TurnFailed,
}

impl TurnTraceEventPayload {
    #[must_use]
    pub fn content_delta(text: impl Into<String>) -> Self {
        Self::ContentDelta { text: text.into() }
    }

    #[must_use]
    pub fn reasoning_delta(text: impl Into<String>) -> Self {
        Self::ReasoningDelta { text: text.into() }
    }

    #[must_use]
    pub fn tool_call_delta(
        index: usize,
        call_id: Option<&str>,
        name: Option<&str>,
        arguments_delta: Option<&str>,
    ) -> Self {
        Self::ToolCallDelta {
            index,
            call_id: call_id.map(ToString::to_string),
            name: name.map(ToString::to_string),
            arguments_delta: arguments_delta.map(ToString::to_string),
        }
    }

    #[must_use]
    pub fn tool_started(
        call_id: impl Into<String>,
        name: impl Into<String>,
        arguments: Value,
    ) -> Self {
        Self::ToolStarted {
            call_id: call_id.into(),
            name: name.into(),
            arguments,
        }
    }

    #[must_use]
    pub fn tool_completed(
        call_id: impl Into<String>,
        name: impl Into<String>,
        result: Value,
        is_error: bool,
    ) -> Self {
        Self::ToolCompleted {
            call_id: call_id.into(),
            name: name.into(),
            result,
            is_error,
        }
    }

    #[must_use]
    pub fn turn_completed(token_usage: TokenUsage) -> Self {
        Self::TurnCompleted { token_usage }
    }

    #[must_use]
    pub const fn turn_settled() -> Self {
        Self::TurnSettled
    }

    #[must_use]
    pub const fn turn_failed() -> Self {
        Self::TurnFailed
    }
}

#[derive(Debug, Clone)]
pub struct TurnEventTraceWriter {
    inner: Arc<Mutex<TurnEventTraceWriterInner>>,
}

#[derive(Debug)]
struct TurnEventTraceWriterInner {
    path: PathBuf,
    next_cursor: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingAssistantTrace {
    pub turn_number: u32,
    pub content: String,
    pub reasoning: String,
    pub tool_calls: Vec<PendingToolCallTrace>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingToolCallTrace {
    pub index: usize,
    pub call_id: Option<String>,
    pub name: Option<String>,
    pub arguments_text: String,
    pub status: PendingToolStatus,
    pub arguments: Option<Value>,
    pub result: Option<Value>,
    pub is_error: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PendingToolStatus {
    Pending,
    Started,
    Completed,
}

#[derive(Debug, Error)]
pub enum TurnEventTraceError {
    #[error("turn event trace I/O error at {path:?}: {reason}")]
    Io { path: PathBuf, reason: String },
    #[error("failed to serialize turn event: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[must_use]
pub fn turn_events_jsonl_path(base_dir: &Path) -> PathBuf {
    base_dir.join(TURN_EVENTS_JSONL_FILE)
}

impl TurnEventTraceWriter {
    pub async fn open(base_dir: &Path) -> Result<Self, TurnEventTraceError> {
        fs::create_dir_all(base_dir).await.map_err(|error| {
            TurnEventTraceError::Io {
                path: base_dir.to_path_buf(),
                reason: format!("failed to create turn event trace dir: {error}"),
            }
        })?;

        let path = turn_events_jsonl_path(base_dir);
        let next_cursor = count_non_empty_lines(&path).await?.saturating_add(1);

        Ok(Self {
            inner: Arc::new(Mutex::new(TurnEventTraceWriterInner { path, next_cursor })),
        })
    }

    pub async fn append(
        &self,
        turn_number: u32,
        payload: TurnTraceEventPayload,
    ) -> Result<TurnTraceEvent, TurnEventTraceError> {
        let mut inner = self.inner.lock().await;
        let event = TurnTraceEvent {
            turn_number,
            cursor: TurnEventCursor::new(inner.next_cursor),
            created_at: Utc::now(),
            payload,
        };
        let line = serde_json::to_string(&event)?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&inner.path)
            .await
            .map_err(|error| TurnEventTraceError::Io {
                path: inner.path.clone(),
                reason: format!("failed to open turn event trace: {error}"),
            })?;
        file.write_all(line.as_bytes()).await.map_err(|error| {
            TurnEventTraceError::Io {
                path: inner.path.clone(),
                reason: format!("failed to append turn event: {error}"),
            }
        })?;
        file.write_all(b"\n").await.map_err(|error| {
            TurnEventTraceError::Io {
                path: inner.path.clone(),
                reason: format!("failed to terminate turn event line: {error}"),
            }
        })?;
        inner.next_cursor = inner.next_cursor.saturating_add(1);
        Ok(event)
    }
}

pub async fn recover_pending_assistant(
    base_dir: &Path,
    committed_turn_count: u32,
) -> Result<Option<PendingAssistantTrace>, TurnEventTraceError> {
    let path = turn_events_jsonl_path(base_dir);
    let exists = fs::try_exists(&path)
        .await
        .map_err(|error| TurnEventTraceError::Io {
            path: path.clone(),
            reason: format!("failed to inspect turn event trace: {error}"),
        })?;
    if !exists {
        return Ok(None);
    }

    let content = fs::read_to_string(&path)
        .await
        .map_err(|error| TurnEventTraceError::Io {
            path: path.clone(),
            reason: format!("failed to read turn event trace: {error}"),
        })?;
    let mut events = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<TurnTraceEvent>(line) {
            Ok(event) if event.turn_number > committed_turn_count => events.push(event),
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    path = %path.display(),
                    line = index + 1,
                    error = %error,
                    "skipping malformed turn event trace line"
                );
            }
        }
    }
    events.sort_by_key(|event| event.cursor);

    let latest_turn_number = events.iter().map(|event| event.turn_number).max();
    let Some(latest_turn_number) = latest_turn_number else {
        return Ok(None);
    };

    let mut pending = PendingAssistantTrace {
        turn_number: latest_turn_number,
        content: String::new(),
        reasoning: String::new(),
        tool_calls: Vec::new(),
    };
    let mut tool_call_indexes = BTreeMap::new();

    for event in events
        .into_iter()
        .filter(|event| event.turn_number == latest_turn_number)
    {
        match event.payload {
            TurnTraceEventPayload::ContentDelta { text } => pending.content.push_str(&text),
            TurnTraceEventPayload::ReasoningDelta { text } => pending.reasoning.push_str(&text),
            TurnTraceEventPayload::ToolCallDelta {
                index,
                call_id,
                name,
                arguments_delta,
            } => {
                let tool_index = tool_call_trace_index(&mut pending, &mut tool_call_indexes, index);
                let tool_call = &mut pending.tool_calls[tool_index];
                if call_id.is_some() {
                    tool_call.call_id = call_id;
                }
                if name.is_some() {
                    tool_call.name = name;
                }
                if let Some(arguments_delta) = arguments_delta {
                    tool_call.arguments_text.push_str(&arguments_delta);
                }
            }
            TurnTraceEventPayload::ToolStarted {
                call_id,
                name,
                arguments,
            } => {
                let tool_index = tool_call_trace_index_by_identity(
                    &mut pending,
                    &mut tool_call_indexes,
                    &call_id,
                );
                let tool_call = &mut pending.tool_calls[tool_index];
                tool_call.call_id = Some(call_id);
                tool_call.name = Some(name);
                tool_call.arguments = Some(arguments);
                tool_call.status = PendingToolStatus::Started;
            }
            TurnTraceEventPayload::ToolCompleted {
                call_id,
                name,
                result,
                is_error,
            } => {
                let tool_index = tool_call_trace_index_by_identity(
                    &mut pending,
                    &mut tool_call_indexes,
                    &call_id,
                );
                let tool_call = &mut pending.tool_calls[tool_index];
                tool_call.call_id = Some(call_id);
                tool_call.name = Some(name);
                tool_call.result = Some(result);
                tool_call.is_error = is_error;
                tool_call.status = PendingToolStatus::Completed;
            }
            TurnTraceEventPayload::TurnCompleted { .. }
            | TurnTraceEventPayload::TurnSettled
            | TurnTraceEventPayload::TurnFailed => return Ok(None),
        }
    }

    Ok(Some(pending))
}

async fn count_non_empty_lines(path: &Path) -> Result<u64, TurnEventTraceError> {
    match fs::read_to_string(path).await {
        Ok(content) => Ok(content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count() as u64),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(TurnEventTraceError::Io {
            path: path.to_path_buf(),
            reason: format!("failed to read turn event trace: {error}"),
        }),
    }
}

fn tool_call_trace_index(
    pending: &mut PendingAssistantTrace,
    tool_call_indexes: &mut BTreeMap<usize, usize>,
    index: usize,
) -> usize {
    if let Some(tool_index) = tool_call_indexes.get(&index) {
        return *tool_index;
    }
    let tool_index = pending.tool_calls.len();
    pending.tool_calls.push(PendingToolCallTrace {
        index,
        call_id: None,
        name: None,
        arguments_text: String::new(),
        status: PendingToolStatus::Pending,
        arguments: None,
        result: None,
        is_error: false,
    });
    tool_call_indexes.insert(index, tool_index);
    tool_index
}

fn tool_call_trace_index_by_identity(
    pending: &mut PendingAssistantTrace,
    tool_call_indexes: &mut BTreeMap<usize, usize>,
    call_id: &str,
) -> usize {
    if let Some((index, _)) = pending
        .tool_calls
        .iter()
        .enumerate()
        .find(|(_, tool_call)| tool_call.call_id.as_deref() == Some(call_id))
    {
        return index;
    }
    let index = pending.tool_calls.len();
    tool_call_trace_index(pending, tool_call_indexes, index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(total_tokens: u32) -> TokenUsage {
        TokenUsage {
            input_tokens: total_tokens.saturating_sub(1),
            output_tokens: 1,
            total_tokens,
        }
    }

    #[tokio::test]
    async fn recovers_pending_assistant_from_turn_events() {
        let dir = tempfile::tempdir().expect("temp dir");
        let writer = TurnEventTraceWriter::open(dir.path()).await.unwrap();
        let first = writer
            .append(1, TurnTraceEventPayload::content_delta("hello"))
            .await
            .unwrap();
        let second = writer
            .append(1, TurnTraceEventPayload::reasoning_delta("thinking"))
            .await
            .unwrap();
        writer
            .append(
                1,
                TurnTraceEventPayload::tool_call_delta(
                    0,
                    Some("call-1"),
                    Some("search"),
                    Some("{\"q\""),
                ),
            )
            .await
            .unwrap();
        writer
            .append(
                1,
                TurnTraceEventPayload::tool_call_delta(0, None, None, Some(":\"rust\"}")),
            )
            .await
            .unwrap();
        writer
            .append(
                1,
                TurnTraceEventPayload::tool_started(
                    "call-1",
                    "search",
                    serde_json::json!({"q":"rust"}),
                ),
            )
            .await
            .unwrap();
        writer
            .append(
                1,
                TurnTraceEventPayload::tool_completed(
                    "call-1",
                    "search",
                    serde_json::json!({"ok":true}),
                    false,
                ),
            )
            .await
            .unwrap();

        assert_eq!(first.cursor, TurnEventCursor::new(1));
        assert_eq!(second.cursor, TurnEventCursor::new(2));

        let pending = recover_pending_assistant(dir.path(), 0).await.unwrap().unwrap();
        assert_eq!(pending.turn_number, 1);
        assert_eq!(pending.content, "hello");
        assert_eq!(pending.reasoning, "thinking");
        assert_eq!(pending.tool_calls[0].arguments_text, "{\"q\":\"rust\"}");
        assert_eq!(pending.tool_calls[0].status, PendingToolStatus::Completed);
    }

    #[tokio::test]
    async fn settled_turn_does_not_recover_pending_assistant() {
        let dir = tempfile::tempdir().expect("temp dir");
        let writer = TurnEventTraceWriter::open(dir.path()).await.unwrap();
        writer
            .append(1, TurnTraceEventPayload::content_delta("done"))
            .await
            .unwrap();
        writer
            .append(1, TurnTraceEventPayload::turn_settled())
            .await
            .unwrap();

        let pending = recover_pending_assistant(dir.path(), 0).await.unwrap();
        assert!(pending.is_none());
    }

    #[tokio::test]
    async fn completed_turn_does_not_recover_pending_assistant() {
        let dir = tempfile::tempdir().expect("temp dir");
        let writer = TurnEventTraceWriter::open(dir.path()).await.unwrap();
        writer
            .append(1, TurnTraceEventPayload::content_delta("done"))
            .await
            .unwrap();
        writer
            .append(1, TurnTraceEventPayload::turn_completed(usage(3)))
            .await
            .unwrap();

        let pending = recover_pending_assistant(dir.path(), 0).await.unwrap();
        assert!(pending.is_none());
    }

    #[tokio::test]
    async fn writer_assigns_unique_cursors_for_concurrent_appends() {
        let dir = tempfile::tempdir().expect("temp dir");
        let writer = TurnEventTraceWriter::open(dir.path()).await.unwrap();

        let left = writer.clone();
        let right = writer.clone();
        let (left, right) = tokio::join!(
            left.append(1, TurnTraceEventPayload::content_delta("a")),
            right.append(1, TurnTraceEventPayload::content_delta("b")),
        );

        let mut cursors = vec![left.unwrap().cursor, right.unwrap().cursor];
        cursors.sort();
        assert_eq!(
            cursors,
            vec![TurnEventCursor::new(1), TurnEventCursor::new(2)]
        );
    }
}
