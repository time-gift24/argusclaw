//! Agent domain types.
//!
//! These types are shared between:
//! - `argus-template` (template management)
//! - `argus-thread` (thread execution)
//! - `argus-session` (session management)
//! - `argus-repository` (persistence)

use serde::{Deserialize, Serialize};

use crate::ids::ProviderId;
use crate::AgentId;

/// Full agent record/template configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRecord {
    /// Unique agent identifier.
    pub id: AgentId,
    /// Display name for the agent.
    pub display_name: String,
    /// Description of what this agent does.
    pub description: String,
    /// Version string (e.g., "1.0.0").
    pub version: String,
    /// Optional associated provider ID.
    pub provider_id: Option<ProviderId>,
    /// System prompt for the agent.
    pub system_prompt: String,
    /// List of tool names enabled for this agent.
    pub tool_names: Vec<String>,
    /// Maximum tokens for LLM requests.
    pub max_tokens: Option<u32>,
    /// Sampling temperature (0.0-2.0).
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
            provider_id: Some(ProviderId::new(provider_id)),
            system_prompt: "You are a test agent.".to_string(),
            tool_names: vec![],
            max_tokens: None,
            temperature: None,
        }
    }
}
