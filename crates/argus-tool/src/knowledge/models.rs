use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

use super::error::KnowledgeToolError;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum KnowledgeAction {
    ListRepos,
    ResolveSnapshot,
    ExploreTree,
    SearchNodes,
    GetNode,
    GetContent,
    GetNeighbors,
    CreateKnowledgePr,
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
            Self::CreateKnowledgePr => "create_knowledge_pr",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeFileWrite {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeManifestRepoPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub default_branch: Option<String>,
    #[serde(default)]
    pub include: Option<Vec<String>>,
    #[serde(default)]
    pub exclude: Option<Vec<String>>,
    #[serde(default)]
    pub entrypoints: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeManifestFilePatch {
    pub path: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub aliases: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeManifestNodeSourcePatch {
    pub path: String,
    #[serde(default)]
    pub heading: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeManifestNodePatch {
    pub id: String,
    pub source: KnowledgeManifestNodeSourcePatch,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub aliases: Option<Vec<String>>,
    #[serde(default)]
    pub relations: Option<Vec<KnowledgeRelation>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeManifestPatch {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub repo: Option<KnowledgeManifestRepoPatch>,
    #[serde(default)]
    pub files: Option<Vec<KnowledgeManifestFilePatch>>,
    #[serde(default)]
    pub nodes: Option<Vec<KnowledgeManifestNodePatch>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeCreatePrArgs {
    pub target_repo: String,
    #[serde(default)]
    pub base_ref: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    pub pr_title: String,
    pub pr_body: String,
    #[serde(default)]
    pub draft: Option<bool>,
    #[serde(default)]
    pub files: Vec<KnowledgeFileWrite>,
    #[serde(default)]
    pub manifest: Option<KnowledgeManifestPatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeCreatePrResult {
    pub target_repo: String,
    pub base_ref: String,
    pub branch: String,
    pub commit_sha: String,
    pub pr_url: String,
    pub manifest_path: String,
    pub changed_files: Vec<String>,
    pub created_files: Vec<String>,
    pub updated_files: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
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
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub depth: Option<usize>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub scope_path: Option<String>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub max_chars: Option<usize>,
    #[serde(default)]
    pub relation_types: Vec<String>,
    #[serde(default)]
    pub target_repo: Option<String>,
    #[serde(default)]
    pub base_ref: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub pr_title: Option<String>,
    #[serde(default)]
    pub pr_body: Option<String>,
    #[serde(default)]
    pub draft: Option<bool>,
    #[serde(default)]
    pub files: Vec<KnowledgeFileWrite>,
    #[serde(default)]
    pub manifest: Option<KnowledgeManifestPatch>,
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeRelation {
    #[serde(rename = "type")]
    pub relation_type: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnowledgeNodeKind {
    File,
    Section,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSource {
    pub path: String,
    pub blob_sha: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeNode {
    pub id: String,
    pub kind: KnowledgeNodeKind,
    pub title: String,
    pub path: String,
    pub anchor: Option<String>,
    pub summary: Option<String>,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    pub relations: Vec<KnowledgeRelation>,
    pub children: Vec<String>,
    pub source: KnowledgeSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExploreTreeEntry {
    pub path: String,
    pub title: String,
    pub child_count: usize,
    pub summary_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExploreTreeResult {
    pub entries: Vec<ExploreTreeEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentPage {
    pub content: String,
    pub truncated: bool,
    pub next_cursor: Option<String>,
    pub source: KnowledgeSource,
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
            KnowledgeAction::CreateKnowledgePr => {
                let target_repo = self
                    .target_repo
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if target_repo.is_none() {
                    return Err(KnowledgeToolError::InvalidArguments(
                        "target_repo is required for action create_knowledge_pr".to_string(),
                    ));
                }

                let pr_title = self
                    .pr_title
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if pr_title.is_none() {
                    return Err(KnowledgeToolError::InvalidArguments(
                        "pr_title is required for action create_knowledge_pr".to_string(),
                    ));
                }

                let pr_body = self
                    .pr_body
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if pr_body.is_none() {
                    return Err(KnowledgeToolError::InvalidArguments(
                        "pr_body is required for action create_knowledge_pr".to_string(),
                    ));
                }

                if self.files.is_empty() {
                    return Err(KnowledgeToolError::InvalidArguments(
                        "files is required for action create_knowledge_pr".to_string(),
                    ));
                }

                if self.files.iter().any(|file| file.path.trim().is_empty()) {
                    return Err(KnowledgeToolError::InvalidArguments(
                        "files[].path is required for action create_knowledge_pr".to_string(),
                    ));
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

impl TryFrom<KnowledgeToolArgs> for KnowledgeCreatePrArgs {
    type Error = KnowledgeToolError;

    fn try_from(args: KnowledgeToolArgs) -> Result<Self, Self::Error> {
        if args.action != KnowledgeAction::CreateKnowledgePr {
            return Err(KnowledgeToolError::invalid_arguments(format!(
                "cannot convert action {} into create_knowledge_pr args",
                args.action
            )));
        }

        let target_repo = args
            .target_repo
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                KnowledgeToolError::invalid_arguments(
                    "target_repo is required for action create_knowledge_pr",
                )
            })?;
        let pr_title = args.pr_title.filter(|value| !value.trim().is_empty()).ok_or_else(|| {
            KnowledgeToolError::invalid_arguments("pr_title is required for action create_knowledge_pr")
        })?;
        let pr_body = args.pr_body.filter(|value| !value.trim().is_empty()).ok_or_else(|| {
            KnowledgeToolError::invalid_arguments("pr_body is required for action create_knowledge_pr")
        })?;

        if args.files.is_empty() {
            return Err(KnowledgeToolError::invalid_arguments(
                "files is required for action create_knowledge_pr",
            ));
        }

        Ok(Self {
            target_repo,
            base_ref: args.base_ref,
            branch: args.branch,
            pr_title,
            pr_body,
            draft: args.draft,
            files: args.files,
            manifest: args.manifest,
        })
    }
}
