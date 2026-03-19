use serde::Deserialize;
use std::fs;
use std::path::Path;

/// Builtin agent definition from TOML file
#[derive(Debug, Deserialize)]
pub struct TomlAgentDef {
    #[allow(dead_code)]
    id: String, // For reference, not used in DB
    display_name: String,
    description: String,
    version: String,
    system_prompt: String,
    tool_names: Vec<String>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    temperature: Option<f32>,
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
            system_prompt: self.system_prompt.clone(),
            tool_names: self.tool_names.clone(),
            max_tokens: self.max_tokens,
            temperature: self.temperature,
        }
    }
}

/// Load all builtin agent TOML files from a directory
pub fn load_builtin_agents_from_dir(dir: &Path) -> Result<Vec<TomlAgentDef>, String> {
    if !dir.exists() {
        return Err(format!("agents directory not found: {}", dir.display()));
    }

    let toml_files = fs::read_dir(dir)
        .map_err(|e| format!("failed to read agents directory: {}", e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|ext| ext == "toml")
                .unwrap_or(false)
        })
        .map(|entry| entry.path())
        .collect::<Vec<_>>();

    if toml_files.is_empty() {
        return Err(format!("no .toml files found in {}", dir.display()));
    }

    let mut agents = Vec::new();
    for toml_path in toml_files {
        let content = fs::read_to_string(&toml_path)
            .map_err(|e| format!("failed to read {}: {}", toml_path.display(), e))?;

        let def: TomlAgentDef = toml::from_str(&content)
            .map_err(|e| format!("failed to parse {}: {}", toml_path.display(), e))?;

        agents.push(def);
    }

    Ok(agents)
}
