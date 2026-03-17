use argus_protocol::ThreadId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Turn log entry for recording turn metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnLog {
    /// The thread this turn belongs to.
    pub thread_id: ThreadId,
    /// Sequence number of this turn within the thread.
    pub turn_seq: i64,
    /// Number of input tokens used.
    pub input_tokens: i64,
    /// Number of output tokens generated.
    pub output_tokens: i64,
    /// Model used for this turn.
    pub model: String,
    /// Latency in milliseconds.
    pub latency_ms: i64,
    /// Serialized turn data (messages, tool calls, etc.).
    pub turn_data: String,
    /// When this turn log was created.
    pub created_at: DateTime<Utc>,
}

/// Report from a cleanup operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupReport {
    /// Number of log entries deleted.
    pub deleted_count: i64,
}
