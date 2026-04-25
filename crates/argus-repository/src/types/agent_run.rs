//! Agent run persistence types.

use std::fmt;

use argus_protocol::{AgentId, SessionId, ThreadId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Public ID for an externally triggered agent run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentRunId(pub Uuid);

impl AgentRunId {
    /// Create a new time-sortable run ID.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// Parse an agent run ID from its string representation.
    pub fn parse(value: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(value)?))
    }
}

impl Default for AgentRunId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AgentRunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Persisted lifecycle state for an externally triggered agent run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

impl AgentRunStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// Stored agent run record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunRecord {
    pub id: AgentRunId,
    pub agent_id: AgentId,
    pub session_id: SessionId,
    pub thread_id: ThreadId,
    pub prompt: String,
    pub status: AgentRunStatus,
    pub result: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}
