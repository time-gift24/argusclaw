//! Agent domain types.
//!
//! These types are shared between:
//! - `argus-template` (template management)
//! - `argus-thread` (thread execution)
//! - `argus-session` (session management)
//! - `argus-repository` (persistence)

use serde::{Deserialize, Serialize};

use crate::AgentId;
use crate::ids::ProviderId;
use crate::llm::ThinkingConfig;

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
    /// Optional default model override for this agent.
    /// If set, this model is used instead of the provider's default_model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// System prompt for the agent.
    pub system_prompt: String,
    /// List of tool names enabled for this agent.
    pub tool_names: Vec<String>,
    /// Display names of agents this agent can dispatch to.
    #[serde(default)]
    pub subagent_names: Vec<String>,
    /// Maximum tokens for LLM requests.
    pub max_tokens: Option<u32>,
    /// Sampling temperature (0.0-2.0).
    pub temperature: Option<f32>,
    /// Thinking configuration for reasoning mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<ThinkingConfig>,
}

impl Default for AgentRecord {
    fn default() -> Self {
        Self {
            id: AgentId::new(0),
            display_name: String::new(),
            description: String::new(),
            version: String::new(),
            provider_id: None,
            model_id: None,
            system_prompt: String::new(),
            tool_names: Vec::new(),
            subagent_names: Vec::new(),
            max_tokens: None,
            temperature: None,
            thinking_config: None,
        }
    }
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
            model_id: None,
            system_prompt: "You are a test agent.".to_string(),
            tool_names: vec![],
            subagent_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: Some(ThinkingConfig::enabled()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AgentRecord;

    #[test]
    fn default_subagent_names_are_empty_and_legacy_fields_are_ignored() {
        let record: AgentRecord = serde_json::from_value(serde_json::json!({
            "id": 7,
            "display_name": "Legacy Agent",
            "description": "from an older snapshot",
            "version": "1.0.0",
            "provider_id": 3,
            "model_id": null,
            "system_prompt": "",
            "tool_names": [],
            "max_tokens": null,
            "temperature": null,
            "thinking_config": null,
            "parent_agent_id": 1,
            "agent_type": "subagent"
        }))
        .expect("legacy agent record should deserialize");

        assert!(record.subagent_names.is_empty());
    }
}
