//! Agent domain types.

use std::fmt;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::db::DbError;
use crate::db::LlmProviderId;

/// Unique identifier for an agent (auto-increment INTEGER).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentId(i64);

impl AgentId {
    /// Creates a new agent ID from a database-generated i64.
    #[must_use]
    pub const fn new(id: i64) -> Self {
        Self(id)
    }

    /// Returns the underlying i64 value.
    #[must_use]
    pub const fn into_inner(self) -> i64 {
        self.0
    }
}

impl From<i64> for AgentId {
    fn from(id: i64) -> Self {
        Self::new(id)
    }
}

impl From<AgentId> for i64 {
    fn from(id: AgentId) -> Self {
        id.into_inner()
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Full agent record stored in database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRecord {
    pub id: AgentId,
    pub display_name: String,
    pub description: String,
    pub version: String,
    pub provider_id: Option<LlmProviderId>,
    pub system_prompt: String,
    pub tool_names: Vec<String>,
    pub max_tokens: Option<u32>,
    /// Sampling temperature (0.0-2.0). Stored as INTEGER * 100 in SQLite for precision.
    pub temperature: Option<f32>,
}

impl AgentRecord {
    /// Create a minimal agent record for testing.
    #[cfg(test)]
    pub fn for_test(id: i64, provider_id: i64) -> Self {
        Self {
            id: AgentId::new(id),
            display_name: format!("Test Agent {id}"),
            description: "A test agent".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(LlmProviderId::new(provider_id)),
            system_prompt: "You are a test agent.".to_string(),
            tool_names: vec![],
            max_tokens: None,
            temperature: None,
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

    /// Find an agent by display name.
    async fn find_by_display_name(
        &self,
        display_name: &str,
    ) -> Result<Option<AgentRecord>, DbError>;

    /// List all agents.
    async fn list(&self) -> Result<Vec<AgentRecord>, DbError>;

    /// Delete an agent. Returns true if a row was deleted.
    async fn delete(&self, id: &AgentId) -> Result<bool, DbError>;
}
