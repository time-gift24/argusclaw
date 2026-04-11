use serde::Deserialize;

use argus_protocol::llm::ThinkingConfig;

/// Builtin agent definition from TOML file
#[derive(Debug, Deserialize)]
pub struct TomlAgentDef {
    display_name: String,
    description: String,
    version: String,
    system_prompt: String,
    tool_names: Vec<String>,
    #[serde(default)]
    subagent_names: Option<Vec<String>>,
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
            subagent_names: self.subagent_names.clone().unwrap_or_default(),
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            thinking_config: self.thinking_config.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TomlAgentDef;

    #[test]
    fn to_agent_record_carries_subagent_names_from_toml() {
        let def: TomlAgentDef = toml::from_str(
            r#"
display_name = "Planner"
description = "Plans work"
version = "1.0.0"
system_prompt = "Plan carefully."
tool_names = ["scheduler"]
subagent_names = ["Researcher", "Writer"]
"#,
        )
        .expect("toml should parse");

        let record = def.to_agent_record();

        assert_eq!(record.subagent_names, vec!["Researcher", "Writer"]);
    }
}
