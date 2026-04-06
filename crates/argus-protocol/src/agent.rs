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

/// The type of agent, determining its capabilities.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    /// Standard agent - can dispatch jobs and create subagents.
    #[default]
    Standard,
    /// Subagent - cannot dispatch jobs (prevents infinite recursion).
    Subagent,
}

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
    /// Maximum tokens for LLM requests.
    pub max_tokens: Option<u32>,
    /// Sampling temperature (0.0-2.0).
    pub temperature: Option<f32>,
    /// Thinking configuration for reasoning mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<ThinkingConfig>,
    /// Parent agent ID (for subagents).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<AgentId>,
    /// Agent type determining its capabilities.
    #[serde(default)]
    pub agent_type: AgentType,
    /// Whether this agent is visible to end users via the server API.
    /// Defaults to `true` for backward compatibility with existing records.
    #[serde(default = "default_true")]
    pub is_enabled: bool,
}

/// Helper for serde default on `is_enabled`.
fn default_true() -> bool {
    true
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
            max_tokens: None,
            temperature: None,
            thinking_config: None,
            parent_agent_id: None,
            agent_type: AgentType::Standard,
            is_enabled: true,
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
            max_tokens: None,
            temperature: None,
            thinking_config: Some(ThinkingConfig::enabled()),
            parent_agent_id: None,
            agent_type: AgentType::Standard,
            is_enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_record_is_enabled_defaults_to_true_when_missing() {
        let json = r#"{"id":1,"display_name":"A","description":"B","version":"1","system_prompt":"C","tool_names":[]}"#;
        let record: AgentRecord = serde_json::from_str(json).expect("should deserialize");
        assert!(record.is_enabled, "is_enabled should default to true");
    }

    #[test]
    fn agent_record_is_enabled_can_be_set_to_false() {
        let json = r#"{"id":1,"display_name":"A","description":"B","version":"1","system_prompt":"C","tool_names":[],"is_enabled":false}"#;
        let record: AgentRecord = serde_json::from_str(json).expect("should deserialize");
        assert!(!record.is_enabled, "is_enabled should be false");
    }

    #[test]
    fn agent_record_is_enabled_round_trips() {
        let mut record = AgentRecord::default();
        record.id = AgentId::new(1);
        record.is_enabled = false;
        let json = serde_json::to_string(&record).expect("serialize");
        let back: AgentRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.is_enabled, false);
    }
}
