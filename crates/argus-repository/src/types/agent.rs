//! Agent persistence types.

use std::fmt;

use serde::{Deserialize, Serialize};

use argus_protocol::llm::LlmProviderId;

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
