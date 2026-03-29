use serde::Deserialize;
use serde_json::Value;

use super::error::KnowledgeToolError;
use super::models::KnowledgeRelation;

pub const DEFAULT_MANIFEST_PATHS: &[&str] = &[".knowledge/repo.json", "knowledge.json"];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RepositoryManifest {
    pub version: u32,
    #[serde(default)]
    pub repo: Option<RepositoryManifestMeta>,
    #[serde(default)]
    pub files: Vec<FileOverride>,
    #[serde(default)]
    pub nodes: Vec<NodeOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RepositoryManifestMeta {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub default_branch: Option<String>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub entrypoints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FileOverride {
    pub path: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NodeOverride {
    pub id: String,
    pub source: NodeSource,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub relations: Vec<KnowledgeRelation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NodeSource {
    pub path: String,
    #[serde(default)]
    pub heading: Option<String>,
}

impl RepositoryManifest {
    pub fn from_json(value: Value) -> Result<Self, KnowledgeToolError> {
        serde_json::from_value(value).map_err(|err| KnowledgeToolError::manifest_parse(err.to_string()))
    }

    pub fn file_override(&self, path: &str) -> Option<&FileOverride> {
        self.files.iter().find(|file| file.path == path)
    }

    pub fn resolve_section_id(&self, path: &str, heading: &str, generated_id: &str) -> String {
        self.nodes
            .iter()
            .find(|node| node.source.path == path && node.source.heading.as_deref() == Some(heading))
            .map(|node| node.id.clone())
            .unwrap_or_else(|| generated_id.to_string())
    }
}
