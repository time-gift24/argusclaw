use serde::Deserialize;
use serde_json::Value;
use std::fmt;

use super::error::KnowledgeToolError;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeAction {
    ListRepos,
    ResolveSnapshot,
    ExploreTree,
    SearchNodes,
    GetNode,
    GetContent,
    GetNeighbors,
}

impl fmt::Display for KnowledgeAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::ListRepos => "list_repos",
            Self::ResolveSnapshot => "resolve_snapshot",
            Self::ExploreTree => "explore_tree",
            Self::SearchNodes => "search_nodes",
            Self::GetNode => "get_node",
            Self::GetContent => "get_content",
            Self::GetNeighbors => "get_neighbors",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeToolArgs {
    pub action: KnowledgeAction,
    #[serde(default)]
    pub repo_id: Option<String>,
    #[serde(default)]
    pub snapshot_id: Option<String>,
    #[serde(default, rename = "ref")]
    pub r#ref: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeRepoDescriptor {
    pub repo_id: String,
    pub provider: String,
    pub owner: String,
    pub name: String,
    #[serde(default = "default_branch")]
    pub default_branch: String,
    #[serde(default)]
    pub manifest_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubSnapshot {
    pub owner: String,
    pub repo: String,
    pub ref_name: String,
    pub rev: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubTree {
    pub rev: String,
    pub entries: Vec<GitHubTreeEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubTreeEntry {
    pub path: String,
    pub sha: String,
    pub kind: GitHubTreeEntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubTreeEntryKind {
    Blob,
    Tree,
    Other(String),
}

impl GitHubTreeEntryKind {
    pub fn from_api(kind: &str) -> Self {
        match kind {
            "blob" => Self::Blob,
            "tree" => Self::Tree,
            other => Self::Other(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubBlob {
    pub sha: String,
    pub text: String,
}

fn default_branch() -> String {
    "main".to_string()
}

impl KnowledgeToolArgs {
    pub fn parse(value: Value) -> Result<Self, KnowledgeToolError> {
        let args: Self = serde_json::from_value(value)
            .map_err(|err| KnowledgeToolError::invalid_arguments(err.to_string()))?;
        args.validate()?;
        Ok(args)
    }

    fn validate(&self) -> Result<(), KnowledgeToolError> {
        match self.action {
            KnowledgeAction::ListRepos => Ok(()),
            KnowledgeAction::ResolveSnapshot => {
                if self.repo_id.is_none() {
                    return Err(KnowledgeToolError::RepoIdRequired {
                        action: self.action.clone(),
                    });
                }
                Ok(())
            }
            _ => {
                if self.repo_id.is_none() && self.snapshot_id.is_none() {
                    return Err(KnowledgeToolError::SnapshotIdRequired {
                        action: self.action.clone(),
                    });
                }
                Ok(())
            }
        }
    }
}
