//! Turn execution trace - append-only JSONL incremental logging.

use std::path::{Path, PathBuf};
use std::pin::Pin;

use chrono::Utc;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufStream};
use tokio_stream::Stream;

use super::events::TurnLogEvent;
use super::error::TurnLogError;

/// Configuration for trace recording.
#[derive(Debug, Clone)]
pub struct TraceConfig {
    /// Whether tracing is enabled.
    pub enabled: bool,
    /// Root directory where trace files are written.
    pub trace_dir: PathBuf,
    /// Session ID (included in path: {trace_dir}/{session_id}/{thread_id}/).
    pub session_id: Option<argus_protocol::SessionId>,
    /// System prompt for this turn (written as TurnStart event).
    pub system_prompt: Option<String>,
    /// Model name for this turn (written as TurnStart event).
    pub model: Option<String>,
    /// Whether to record streaming deltas (llm_delta, tool_call_delta).
    pub include_streaming_deltas: bool,
}

impl TraceConfig {
    /// Create a new TraceConfig.
    pub fn new(enabled: bool, trace_dir: PathBuf) -> Self {
        Self {
            enabled,
            trace_dir,
            session_id: None,
            system_prompt: None,
            model: None,
            include_streaming_deltas: true,
        }
    }

    /// Set the session ID for trace directory path.
    pub fn with_session_id(mut self, session_id: argus_protocol::SessionId) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Set system prompt and model for TurnStart event.
    pub fn with_turn_start(mut self, system_prompt: Option<String>, model: Option<String>) -> Self {
        self.system_prompt = system_prompt;
        self.model = model;
        self
    }

    /// Create a disabled TraceConfig (no tracing).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            trace_dir: PathBuf::new(),
            session_id: None,
            system_prompt: None,
            model: None,
            include_streaming_deltas: true,
        }
    }
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self::disabled()
    }
}

/// Turn log state reconstructed from JSONL for replay/recovery.
#[derive(Debug)]
pub struct TurnLogState {
    /// Thread ID.
    pub thread_id: String,
    /// Turn number.
    pub turn_number: u32,
    /// System prompt (from turn_start).
    pub system_prompt: Option<String>,
    /// Model name (from turn_start).
    pub model: Option<String>,
    /// Messages accumulated so far.
    pub messages: Vec<argus_protocol::llm::ChatMessage>,
    /// Tools available.
    pub tools: Vec<serde_json::Value>,
    /// Token usage (from turn_end).
    pub token_usage: Option<argus_protocol::TokenUsage>,
    /// Finish reason (from turn_end).
    pub finish_reason: Option<String>,
    /// Error message (from turn_error).
    pub error: Option<String>,
}

/// Async writer for append-only JSONL trace files.
pub struct TraceWriter {
    file: BufStream<File>,
    thread_id: String,
    turn_number: u32,
    include_streaming_deltas: bool,
}

impl TraceWriter {
    /// Create a new TraceWriter for the given thread and turn.
    /// Opens file in append mode, creates if not exists.
    ///
    /// `base_dir` should be `{trace_dir}/{session_id}/{thread_id}/turns/`.
    pub async fn new(
        base_dir: &std::path::Path,
        turn_number: u32,
        config: &TraceConfig,
    ) -> std::io::Result<Self> {
        let dir = base_dir.join("turns");
        tokio::fs::create_dir_all(&dir).await?;

        let file_path = dir.join(format!("{}.jsonl", turn_number));
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .await?;

        Ok(Self {
            file: BufStream::new(file),
            thread_id: base_dir.file_name().and_then(|s| s.to_str()).unwrap_or("unknown").to_string(),
            turn_number,
            include_streaming_deltas: config.include_streaming_deltas,
        })
    }

    /// Write a single event to the JSONL file.
    /// Skips delta events if `include_streaming_deltas` is false.
    pub async fn write_event(&mut self, event: &TurnLogEvent) -> std::io::Result<()> {
        // Skip delta events if streaming deltas are disabled
        if !self.include_streaming_deltas {
            match event {
                TurnLogEvent::LlmDelta { .. } | TurnLogEvent::ToolCallDelta { .. } => {
                    return Ok(());
                }
                _ => {}
            }
        }

        let ts = Utc::now().to_rfc3339();
        // Serialize the event to extract its fields, then flatten into the wrapper
        let event_value = serde_json::to_value(event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        // event_value has form: {"type": "...", "data": {...fields...}}
        // We flatten: wrapper has "type", "v", "thread_id", "turn", "ts", plus all event fields
        let mut wrapper = serde_json::json!({
            "v": "1",
            "thread_id": self.thread_id,
            "turn": self.turn_number,
            "ts": ts,
        });
        if let Some(obj) = wrapper.as_object_mut() {
            obj.insert("type".to_string(), event_value.get("type").cloned().unwrap_or(serde_json::Value::String(event.type_name().to_string())));
            if let Some(data) = event_value.get("data")
                && let Some(fields) = data.as_object() {
                    for (k, v) in fields {
                        obj.insert(k.clone(), v.clone());
                    }
                }
        }
        let line = serde_json::to_string(&wrapper)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.file.write_all(line.as_bytes()).await?;
        self.file.write_all(b"\n").await?;
        self.file.flush().await?;
        Ok(())
    }

    /// Finalize the trace as a success with token usage.
    pub async fn finish_success(
        mut self,
        token_usage: &argus_protocol::TokenUsage,
    ) -> std::io::Result<()> {
        let event = TurnLogEvent::TurnEnd {
            token_usage: token_usage.clone(),
            finish_reason: "stop".into(),
        };
        self.write_event(&event).await?;
        self.file.flush().await?;
        Ok(())
    }

    /// Finalize the trace as a failure.
    pub async fn finish_failure(mut self, error: &str) -> std::io::Result<()> {
        let event = TurnLogEvent::TurnError {
            error: error.to_string(),
            at_iteration: None,
        };
        self.write_event(&event).await?;
        self.file.flush().await?;
        Ok(())
    }
}

/// Read JSONL events from a turn file path.
pub async fn read_jsonl_events(
    path: &PathBuf,
) -> Result<Vec<TurnLogEvent>, TurnLogError> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|_e| TurnLogError::TurnNotFound(path.clone()))?;

    let mut events = Vec::new();
    for (line_idx, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let line_num = line_idx + 1;
        // Try parsing as the full JSONL wrapper
        if let Ok(wrapper) = serde_json::from_str::<serde_json::Value>(line) {
            // New flattened format: type and fields at wrapper level
            if let Some(type_str) = wrapper.get("type").and_then(|t| t.as_str()) {
                // Build synthetic inner data for TurnLogEvent deserialization
                // TurnLogEvent uses #[serde(tag = "type", content = "data")]
                // so we need {"type": "...", "data": {...variant_fields...}}
                let mut event_data = serde_json::Map::new();
                // Collect all event fields from wrapper (except metadata)
                for (k, v) in wrapper.as_object().unwrap_or(&serde_json::Map::new()) {
                    if !["v", "thread_id", "turn", "ts", "type"].contains(&k.as_str()) {
                        event_data.insert(k.clone(), v.clone());
                    }
                }
                let inner = serde_json::json!({
                    "type": type_str,
                    "data": serde_json::Value::Object(event_data),
                });
                if let Ok(event) = serde_json::from_value(inner) {
                    events.push(event);
                    continue;
                }
            }
        }
        // Try direct parse (for bare TurnLogEvent lines)
        match serde_json::from_str::<TurnLogEvent>(line) {
            Ok(event) => events.push(event),
            Err(_) => {
                tracing::warn!("Malformed JSONL line {}: skipped", line_num);
            }
        }
    }
    Ok(events)
}

/// Recover events from a turn JSONL file as a stream.
pub async fn recover_turn_events(
    trace_dir: &Path,
    session_id: &argus_protocol::SessionId,
    thread_id: &argus_protocol::ThreadId,
    from_turn: u32,
) -> impl Stream<Item = Result<TurnLogEvent, TurnLogError>> {
    let path = trace_dir
        .join(session_id.inner().to_string())
        .join(thread_id.inner().to_string())
        .join("turns")
        .join(format!("{}.jsonl", from_turn));

    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => {
            // Return a stream that immediately yields an error then completes
            let err_path = path.clone();
            return Box::pin(futures_util::stream::iter(
                vec![Err(TurnLogError::TurnNotFound(err_path))]
            )) as Pin<Box<dyn Stream<Item = Result<TurnLogEvent, TurnLogError>>>>;
        }
    };

    use std::cell::Cell;
    let line_num = Cell::new(0usize);

    let lines_stream = tokio_stream::wrappers::LinesStream::new(
        tokio::io::BufReader::new(file).lines(),
    );

    Box::pin(tokio_stream::StreamExt::map(lines_stream, move |line_result: std::io::Result<String>| {
        line_num.set(line_num.get() + 1);
        let line = match line_result {
            Ok(l) => l.trim().to_string(),
            Err(e) => {
                return Err(TurnLogError::MalformedEvent {
                    line: line_num.get(),
                    reason: format!("IO error reading line: {}", e),
                });
            }
        };
        if line.is_empty() {
            return Err(TurnLogError::MalformedEvent {
                line: line_num.get(),
                reason: "empty line".into(),
            });
        }
        if let Ok(wrapper) = serde_json::from_str::<serde_json::Value>(&line)
            && let Some(type_str) = wrapper.get("type").and_then(|t| t.as_str()) {
                let mut event_data = serde_json::Map::new();
                for (k, v) in wrapper.as_object().unwrap_or(&serde_json::Map::new()) {
                    if !["v", "thread_id", "turn", "ts", "type"].contains(&k.as_str()) {
                        event_data.insert(k.clone(), v.clone());
                    }
                }
                let inner = serde_json::json!({
                    "type": type_str,
                    "data": serde_json::Value::Object(event_data),
                });
                if let Ok(event) = serde_json::from_value(inner) {
                    return Ok(event);
                }
            }
        match serde_json::from_str::<TurnLogEvent>(&line) {
            Ok(event) => Ok(event),
            Err(e) => {
                if line.len() < 10 {
                    Err(TurnLogError::TruncatedEvent {
                        line: line_num.get(),
                        reason: format!("incomplete JSON: {}", e),
                    })
                } else {
                    Err(TurnLogError::MalformedEvent {
                        line: line_num.get(),
                        reason: e.to_string(),
                    })
                }
            }
        }
    })) as Pin<Box<dyn Stream<Item = Result<TurnLogEvent, TurnLogError>>>>
}
