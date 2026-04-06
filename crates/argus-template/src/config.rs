use serde::Deserialize;

use argus_protocol::{llm::ThinkingConfig, AgentType};

/// Builtin agent definition from TOML file
#[derive(Debug, Deserialize)]
pub struct TomlAgentDef {
    display_name: String,
    description: String,
    version: String,
    system_prompt: String,
    tool_names: Vec<String>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    thinking_config: Option<ThinkingConfig>,
}

impl TomlAgentDef {
    /// Convert to AgentRecord (id=0 for insert)
    pub fn to_agent_record(&self) -> argus_protocol::AgentRecord {
        argus_protocol::AgentRecord {
            id: argus_protocol::AgentId::new(0),
            display_name: self.display_name.clone(),
            description: self.description.clone(),
            version: self.version.clone(),
            provider_id: None,
            model_id: None,
            system_prompt: self.system_prompt.clone(),
            tool_names: self.tool_names.clone(),
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            thinking_config: self.thinking_config.clone(),
            parent_agent_id: None,
            agent_type: AgentType::Standard,
            is_enabled: true,
        }
    }
}
