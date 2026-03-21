//! Turn execution trace - iteration-level audit logging.

use serde::Serialize;
use std::path::PathBuf;

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
    pub total_tokens: u32,
}

/// The complete trace file structure.
#[derive(Debug, Clone, Serialize)]
pub struct TraceFile {
    pub version: String,
    pub thread_id: String,
    pub turn_number: u32,
    pub start_time: String,
    pub end_time: Option<String>,
    pub iterations: Vec<IterationRecord>,
    pub final_output: Option<FinalOutput>,
}
