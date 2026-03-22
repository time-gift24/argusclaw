//! Turn execution trace - iteration-level audit logging.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Configuration for trace recording.
#[derive(Debug, Clone)]
pub struct TraceConfig {
    /// Whether tracing is enabled.
    pub enabled: bool,
    /// Directory where trace files are written.
    pub trace_dir: PathBuf,
}

impl TraceConfig {
    /// Create a new TraceConfig.
    pub fn new(enabled: bool, trace_dir: PathBuf) -> Self {
        Self { enabled, trace_dir }
    }

    /// Create a disabled TraceConfig (no tracing).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            trace_dir: PathBuf::new(),
        }
    }
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self::disabled()
    }
}

/// LLM request captured for a single iteration.
#[derive(Debug, Clone, Serialize)]
pub struct LlmRequest {
    pub messages: Vec<serde_json::Value>,
    pub tools: Vec<serde_json::Value>,
}

/// LLM response captured for a single iteration.
#[derive(Debug, Clone, Serialize)]
pub struct LlmResponse {
    pub content: Option<String>,
    pub reasoning_content: Option<String>,
    pub tool_calls: Vec<serde_json::Value>,
    pub finish_reason: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Single tool execution result.
#[derive(Debug, Clone, Serialize)]
pub struct ToolExecution {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub result: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}

/// A single iteration's record (one LLM call + tool executions).
#[derive(Debug, Clone, Serialize)]
pub struct IterationRecord {
    pub iteration: u32,
    pub llm_request: LlmRequest,
    pub llm_response: LlmResponse,
    pub tools: Vec<ToolExecution>,
}

/// The final output of a turn.
#[derive(Debug, Clone, Serialize)]
pub struct FinalOutput {
    pub token_usage: TokenUsageRecord,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenUsageRecord {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub reasoning_tokens: u32,
    pub total_tokens: u32,
}

/// The complete trace file structure.
#[derive(Debug, Clone, Serialize)]
pub struct TraceFile {
    pub version: String,
    pub thread_id: String,
    pub turn_number: u32,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub iterations: Vec<IterationRecord>,
    pub final_output: Option<FinalOutput>,
}

/// Writer for trace files.
pub struct TraceWriter {
    file: BufWriter<File>,
    thread_id: String,
    turn_number: u32,
    start_time: DateTime<Utc>,
    iterations: Vec<IterationRecord>,
}

impl TraceWriter {
    /// Create a new TraceWriter for the given thread and turn.
    pub fn new(thread_id: &str, turn_number: u32, config: &TraceConfig) -> std::io::Result<Self> {
        let dir = config.trace_dir.join(thread_id);
        fs::create_dir_all(&dir)?;

        let file_path = dir.join(format!("{}.json", turn_number));
        let file = File::create(file_path)?;
        let writer = BufWriter::new(file);

        Ok(Self {
            file: writer,
            thread_id: thread_id.to_string(),
            turn_number,
            start_time: Utc::now(),
            iterations: Vec::new(),
        })
    }

    /// Write an iteration record.
    pub fn write_iteration(&mut self, iteration: IterationRecord) -> std::io::Result<()> {
        self.iterations.push(iteration);
        Ok(())
    }

    /// Finalize the trace as a success with token usage.
    pub fn finish_success(
        mut self,
        token_usage: &argus_protocol::TokenUsage,
    ) -> std::io::Result<()> {
        let trace = TraceFile {
            version: "1.0".to_string(),
            thread_id: self.thread_id,
            turn_number: self.turn_number,
            start_time: self.start_time,
            end_time: Some(Utc::now()),
            iterations: self.iterations,
            final_output: Some(FinalOutput {
                token_usage: TokenUsageRecord {
                    input_tokens: token_usage.input_tokens,
                    output_tokens: token_usage.output_tokens,
                    reasoning_tokens: token_usage.reasoning_tokens,
                    total_tokens: token_usage.total_tokens,
                },
            }),
        };

        serde_json::to_writer(&mut self.file, &trace)?;
        self.file.flush()?;
        Ok(())
    }

    /// Finalize the trace as a failure (no final output).
    #[allow(dead_code)]
    pub fn finish_failure(mut self, error: &str) -> std::io::Result<()> {
        let thread_id = self.thread_id.clone();
        let turn_number = self.turn_number;

        let trace = TraceFile {
            version: "1.0".to_string(),
            thread_id: self.thread_id,
            turn_number: self.turn_number,
            start_time: self.start_time,
            end_time: Some(Utc::now()),
            iterations: self.iterations,
            final_output: None,
        };

        // Write the trace file with error context in stderr
        eprintln!(
            "[TRACE ERROR] thread_id={} turn={} error={}",
            thread_id, turn_number, error
        );

        serde_json::to_writer(&mut self.file, &trace)?;
        self.file.flush()?;
        Ok(())
    }
}
