use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

use super::error::KnowledgeToolError;
use super::github::{GitHubKnowledgeBackend, ReqwestGitHubTransport};
use super::indexer::{KnowledgeBackend, KnowledgeIndexer};
use super::models::{
    ContentPage, ExploreTreeEntry, GitHubSnapshot, KnowledgeAction, KnowledgeNode,
    KnowledgeNodeKind, KnowledgeRepoDescriptor, KnowledgeToolArgs,
};
use super::registry::KnowledgeRepoRegistry;

#[async_trait]
pub trait KnowledgeRuntime: Send + Sync {
    async fn dispatch(
        &self,
        args: KnowledgeToolArgs,
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<Value, ToolError>;
}

#[async_trait]
pub trait KnowledgeRuntimeBackend: KnowledgeBackend {
    async fn list_repos(&self) -> Result<Vec<KnowledgeRepoDescriptor>, KnowledgeToolError>;

    fn repo_descriptor(&self, repo_id: &str) -> Option<KnowledgeRepoDescriptor>;

    async fn resolve_snapshot(
        &self,
        repo_id: &str,
        ref_name: &str,
    ) -> Result<(String, GitHubSnapshot), KnowledgeToolError>;
}

pub struct DefaultKnowledgeRuntime<B = GitHubKnowledgeBackend<ReqwestGitHubTransport>> {
    backend: Arc<B>,
    indexer: KnowledgeIndexer<Arc<B>>,
}

impl DefaultKnowledgeRuntime<GitHubKnowledgeBackend<ReqwestGitHubTransport>> {
    #[must_use]
    pub fn new() -> Self {
        Self::new_with_backend(GitHubKnowledgeBackend::new(
            KnowledgeRepoRegistry::load_default(),
            ReqwestGitHubTransport::new(),
        ))
    }
}

impl<B: KnowledgeRuntimeBackend + 'static> DefaultKnowledgeRuntime<B> {
    #[must_use]
    pub fn new_for_test(backend: B) -> Self {
        Self::new_with_backend(backend)
    }

    #[must_use]
    pub fn new_with_backend(backend: B) -> Self {
        let backend = Arc::new(backend);
        let indexer = KnowledgeIndexer::new(backend.clone());
        Self { backend, indexer }
    }

    async fn resolve_snapshot_id(&self, args: &KnowledgeToolArgs) -> Result<String, ToolError> {
        if let Some(snapshot_id) = &args.snapshot_id {
            return Ok(snapshot_id.clone());
        }

        let repo_id = args
            .repo_id
            .as_deref()
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool_name: "knowledge".to_string(),
                reason: "repo_id is required".to_string(),
            })?;

        let ref_name = self.resolve_ref_name(repo_id, args.r#ref.as_deref());
        let (snapshot_id, _) = self
            .backend
            .resolve_snapshot(repo_id, &ref_name)
            .await
            .map_err(|err| ToolError::ExecutionFailed {
                tool_name: "knowledge".to_string(),
                reason: err.to_string(),
            })?;

        Ok(snapshot_id)
    }

    fn resolve_ref_name(&self, repo_id: &str, requested: Option<&str>) -> String {
        requested
            .map(ToString::to_string)
            .or_else(|| {
                self.backend
                    .repo_descriptor(repo_id)
                    .map(|repo| repo.default_branch)
            })
            .unwrap_or_else(|| "main".to_string())
    }

    fn render_snapshot(
        &self,
        snapshot_id: &str,
        repo_id: &str,
        ref_name: &str,
        snapshot: &GitHubSnapshot,
    ) -> Value {
        json!({
            "snapshot_id": snapshot_id,
            "repo_id": repo_id,
            "ref": ref_name,
            "owner": snapshot.owner,
            "repo": snapshot.repo,
            "rev": snapshot.rev,
        })
    }

    fn render_repo(repo: &KnowledgeRepoDescriptor) -> Value {
        json!({
            "repo_id": repo.repo_id,
            "provider": repo.provider,
            "owner": repo.owner,
            "name": repo.name,
            "default_branch": repo.default_branch,
            "manifest_paths": repo.manifest_paths,
        })
    }

    fn render_tree_entry(entry: &ExploreTreeEntry) -> Value {
        json!({
            "path": entry.path,
            "title": entry.title,
            "child_count": entry.child_count,
            "summary_hint": entry.summary_hint,
        })
    }

    fn render_node(node: &KnowledgeNode) -> Value {
        json!({
            "id": node.id,
            "kind": match node.kind {
                KnowledgeNodeKind::File => "file",
                KnowledgeNodeKind::Section => "section",
            },
            "title": node.title,
            "path": node.path,
            "anchor": node.anchor,
            "summary": node.summary,
            "aliases": node.aliases,
            "tags": node.tags,
            "relations": node
                .relations
                .iter()
                .map(|relation| json!({
                    "type": relation.relation_type,
                    "target": relation.target,
                }))
                .collect::<Vec<_>>(),
            "children": node.children,
            "source": {
                "path": node.source.path,
                "blob_sha": node.source.blob_sha,
                "start_line": node.source.start_line,
                "end_line": node.source.end_line,
            }
        })
    }

    fn render_content(page: &ContentPage) -> Value {
        json!({
            "content": page.content,
            "truncated": page.truncated,
            "next_cursor": page.next_cursor,
            "source": {
                "path": page.source.path,
                "blob_sha": page.source.blob_sha,
                "start_line": page.source.start_line,
                "end_line": page.source.end_line,
            }
        })
    }
}

impl Default for DefaultKnowledgeRuntime<GitHubKnowledgeBackend<ReqwestGitHubTransport>> {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<B: KnowledgeRuntimeBackend + 'static> KnowledgeRuntime for DefaultKnowledgeRuntime<B> {
    async fn dispatch(
        &self,
        args: KnowledgeToolArgs,
        _ctx: Arc<ToolExecutionContext>,
    ) -> Result<Value, ToolError> {
        match args.action {
            KnowledgeAction::ListRepos => {
                let repos =
                    self.backend
                        .list_repos()
                        .await
                        .map_err(|err| ToolError::ExecutionFailed {
                            tool_name: "knowledge".to_string(),
                            reason: err.to_string(),
                        })?;

                Ok(json!({
                    "repos": repos.iter().map(Self::render_repo).collect::<Vec<_>>(),
                }))
            }
            KnowledgeAction::ResolveSnapshot => {
                let repo_id =
                    args.repo_id
                        .as_deref()
                        .ok_or_else(|| ToolError::ExecutionFailed {
                            tool_name: "knowledge".to_string(),
                            reason: "repo_id is required".to_string(),
                        })?;
                let ref_name = self.resolve_ref_name(repo_id, args.r#ref.as_deref());
                let (snapshot_id, snapshot) = self
                    .backend
                    .resolve_snapshot(repo_id, &ref_name)
                    .await
                    .map_err(|err| ToolError::ExecutionFailed {
                        tool_name: "knowledge".to_string(),
                        reason: err.to_string(),
                    })?;

                Ok(self.render_snapshot(&snapshot_id, repo_id, &ref_name, &snapshot))
            }
            KnowledgeAction::ExploreTree => {
                let snapshot_id = self.resolve_snapshot_id(&args).await?;
                let path = args.path.as_deref().unwrap_or("/");
                let depth = args.depth.unwrap_or(1);
                let tree = self
                    .indexer
                    .explore_tree(&snapshot_id, path, depth)
                    .await
                    .map_err(|err| ToolError::ExecutionFailed {
                        tool_name: "knowledge".to_string(),
                        reason: err.to_string(),
                    })?;

                Ok(json!({
                    "snapshot_id": snapshot_id,
                    "path": path,
                    "entries": tree.entries.iter().map(Self::render_tree_entry).collect::<Vec<_>>(),
                    "truncated": tree.truncated,
                }))
            }
            KnowledgeAction::SearchNodes => {
                let snapshot_id = self.resolve_snapshot_id(&args).await?;
                let query = args
                    .query
                    .as_deref()
                    .ok_or_else(|| ToolError::ExecutionFailed {
                        tool_name: "knowledge".to_string(),
                        reason: "query is required".to_string(),
                    })?;
                let limit = args.limit.unwrap_or(20);
                let nodes = self
                    .indexer
                    .search_nodes(&snapshot_id, query, args.scope_path.as_deref(), limit)
                    .await
                    .map_err(|err| ToolError::ExecutionFailed {
                        tool_name: "knowledge".to_string(),
                        reason: err.to_string(),
                    })?;

                Ok(json!({
                    "snapshot_id": snapshot_id,
                    "query": query,
                    "results": nodes.iter().map(Self::render_node).collect::<Vec<_>>(),
                }))
            }
            KnowledgeAction::GetNode => {
                let snapshot_id = self.resolve_snapshot_id(&args).await?;
                let node_id =
                    args.node_id
                        .as_deref()
                        .ok_or_else(|| ToolError::ExecutionFailed {
                            tool_name: "knowledge".to_string(),
                            reason: "node_id is required".to_string(),
                        })?;
                let node = self
                    .indexer
                    .get_node(&snapshot_id, node_id)
                    .await
                    .map_err(|err| ToolError::ExecutionFailed {
                        tool_name: "knowledge".to_string(),
                        reason: err.to_string(),
                    })?;

                Ok(Self::render_node(&node))
            }
            KnowledgeAction::GetContent => {
                let snapshot_id = self.resolve_snapshot_id(&args).await?;
                let node_id =
                    args.node_id
                        .as_deref()
                        .ok_or_else(|| ToolError::ExecutionFailed {
                            tool_name: "knowledge".to_string(),
                            reason: "node_id is required".to_string(),
                        })?;
                let page = self
                    .indexer
                    .get_content(
                        &snapshot_id,
                        node_id,
                        args.max_chars,
                        args.cursor.as_deref(),
                    )
                    .await
                    .map_err(|err| ToolError::ExecutionFailed {
                        tool_name: "knowledge".to_string(),
                        reason: err.to_string(),
                    })?;

                Ok(Self::render_content(&page))
            }
            KnowledgeAction::GetNeighbors => {
                let snapshot_id = self.resolve_snapshot_id(&args).await?;
                let node_id =
                    args.node_id
                        .as_deref()
                        .ok_or_else(|| ToolError::ExecutionFailed {
                            tool_name: "knowledge".to_string(),
                            reason: "node_id is required".to_string(),
                        })?;
                let neighbors = self
                    .indexer
                    .get_neighbors(&snapshot_id, node_id)
                    .await
                    .map_err(|err| ToolError::ExecutionFailed {
                        tool_name: "knowledge".to_string(),
                        reason: err.to_string(),
                    })?;

                Ok(json!({
                    "snapshot_id": snapshot_id,
                    "node_id": node_id,
                    "results": neighbors.iter().map(Self::render_node).collect::<Vec<_>>(),
                }))
            }
        }
    }
}

pub struct KnowledgeTool<R = DefaultKnowledgeRuntime> {
    runtime: Arc<R>,
}

impl Default for KnowledgeTool<DefaultKnowledgeRuntime> {
    fn default() -> Self {
        Self::new()
    }
}

impl KnowledgeTool<DefaultKnowledgeRuntime> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            runtime: Arc::new(DefaultKnowledgeRuntime::new()),
        }
    }
}

impl<R> KnowledgeTool<R> {
    #[must_use]
    pub fn new_for_test(runtime: R) -> Self {
        Self {
            runtime: Arc::new(runtime),
        }
    }
}

#[async_trait]
impl<R: KnowledgeRuntime> NamedTool for KnowledgeTool<R> {
    fn name(&self) -> &str {
        "knowledge"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "knowledge".to_string(),
            description:
                "Explore GitHub-backed knowledge bases progressively through snapshot, tree, search, and node actions."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "The knowledge action to run",
                        "enum": [
                            "list_repos",
                            "resolve_snapshot",
                            "explore_tree",
                            "search_nodes",
                            "get_node",
                            "get_content",
                            "get_neighbors"
                        ]
                    },
                    "repo_id": {
                        "type": "string",
                        "description": "Knowledge repository identifier"
                    },
                    "snapshot_id": {
                        "type": "string",
                        "description": "Resolved snapshot identifier"
                    },
                    "ref": {
                        "type": "string",
                        "description": "Git reference to resolve, defaults to the repository default branch"
                    },
                    "cursor": {
                        "type": "string",
                        "description": "Pagination cursor for bounded content reads"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results to return"
                    },
                    "path": {
                        "type": "string",
                        "description": "Repository path scope for tree exploration"
                    },
                    "depth": {
                        "type": "integer",
                        "description": "Directory exploration depth"
                    },
                    "query": {
                        "type": "string",
                        "description": "Search query for progressive node search"
                    },
                    "scope_path": {
                        "type": "string",
                        "description": "Optional scope path for search"
                    },
                    "node_id": {
                        "type": "string",
                        "description": "Knowledge node identifier"
                    },
                    "max_chars": {
                        "type": "integer",
                        "description": "Maximum characters to return for content"
                    },
                    "relation_types": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Relationship types to include when fetching neighbors"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    async fn execute(
        &self,
        input: Value,
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<Value, ToolError> {
        let args = KnowledgeToolArgs::parse(input).map_err(|err| ToolError::ExecutionFailed {
            tool_name: "knowledge".to_string(),
            reason: err.to_string(),
        })?;

        self.runtime.dispatch(args, ctx).await
    }
}
