//! Agent domain types.

use std::fmt;
use std::str::FromStr;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbError;

/// Unique identifier for an agent template.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(String);

/// Unique runtime identifier for an Agent instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentRuntimeId(pub Uuid);

impl AgentRuntimeId {
    /// Create a new runtime ID.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse from a string representation.
    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(s).map(Self)
    }
}

impl Default for AgentRuntimeId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AgentRuntimeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for AgentRuntimeId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl AgentId {
    /// Creates a new agent ID.
    ///
    /// # Panics
    /// Panics in debug mode if `id` is empty.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        debug_assert!(!id.is_empty(), "AgentId cannot be empty");
        Self(id)
    }
}

impl AsRef<str> for AgentId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

/// Parses an agent ID from a string.
///
/// This implementation is intentionally infallible to match the behavior of `AgentId::new()`.
/// If validation is needed in the future, use `AgentId::try_from_str()` instead.
impl FromStr for AgentId {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

/// Full agent record stored in database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRecord {
    pub id: AgentId,
    pub display_name: String,
    pub description: String,
    pub version: String,
    pub provider_id: String,
    pub system_prompt: String,
    pub tool_names: Vec<String>,
    pub max_tokens: Option<u32>,
    /// Sampling temperature (0.0-2.0). Stored as INTEGER * 100 in SQLite for precision.
    pub temperature: Option<f32>,
}

impl AgentRecord {
    /// Create a minimal agent record for testing.
    #[cfg(test)]
    pub fn for_test(id: &str, provider_id: &str) -> Self {
        Self {
            id: AgentId::new(id),
            display_name: format!("Test Agent {id}"),
            description: "A test agent".to_string(),
            version: "1.0.0".to_string(),
            provider_id: provider_id.to_string(),
            system_prompt: "You are a test agent.".to_string(),
            tool_names: vec![],
            max_tokens: None,
            temperature: None,
        }
    }
}

/// Summary for listing (excludes large fields like system_prompt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSummary {
    pub id: AgentId,
    pub display_name: String,
    pub description: String,
    pub version: String,
    pub provider_id: String,
}

impl From<AgentRecord> for AgentSummary {
    fn from(record: AgentRecord) -> Self {
        Self {
            id: record.id,
            display_name: record.display_name,
            description: record.description,
            version: record.version,
            provider_id: record.provider_id,
        }
    }
}

/// Repository trait for agent persistence.
#[async_trait]
pub trait AgentRepository: Send + Sync {
    /// Create or update an agent.
    async fn upsert(&self, record: &AgentRecord) -> Result<(), DbError>;

    /// Get an agent by ID.
    async fn get(&self, id: &AgentId) -> Result<Option<AgentRecord>, DbError>;

    /// List all agents (summaries only).
    async fn list(&self) -> Result<Vec<AgentSummary>, DbError>;

    /// Delete an agent. Returns true if a row was deleted.
    async fn delete(&self, id: &AgentId) -> Result<bool, DbError>;
}
