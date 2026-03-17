//! Built-in agent definitions embedded at compile time.

use super::types::{AgentId, AgentRecord};

/// Default ArgusWing agent definition embedded at compile time.
const ARGUSWING_TOML: &str = include_str!("../../../../agents/arguswing.toml");

/// Default agent display name for lookup.
pub const DEFAULT_AGENT_DISPLAY_NAME: &str = "ArgusWing";

/// Load the built-in ArgusWing agent record.
///
/// # Errors
///
/// Returns an error if the embedded TOML is malformed or missing required fields.
pub fn load_arguswing() -> Result<AgentRecord, toml::de::Error> {
    #[derive(serde::Deserialize)]
    struct AgentDef {
        display_name: String,
        description: String,
        version: String,
        system_prompt: String,
        tool_names: Vec<String>,
    }

    let def: AgentDef = toml::from_str(ARGUSWING_TOML)?;
    Ok(AgentRecord {
        id: AgentId::new(0), // Placeholder ID; will be assigned by database
        display_name: def.display_name,
        description: def.description,
        version: def.version,
        provider_id: None, // Provider is bound at runtime
        system_prompt: def.system_prompt,
        tool_names: def.tool_names,
        max_tokens: None,
        temperature: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_arguswing_parses_embedded_toml() {
        let agent = load_arguswing().expect("embedded TOML should parse");
        assert_eq!(agent.display_name, "ArgusWing");
        assert_eq!(agent.tool_names, vec!["shell", "read", "grep", "glob"]);
        assert!(agent.provider_id.is_none());
    }
}
