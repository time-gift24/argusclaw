//! Agent domain types.

use std::fmt;
use std::str::FromStr;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::db::DbError;

/// Unique identifier for an agent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(String);

impl AgentId {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
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
