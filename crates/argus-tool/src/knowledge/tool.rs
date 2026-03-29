use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

use super::models::KnowledgeToolArgs;

#[async_trait]
pub trait KnowledgeRuntime: Send + Sync {
    async fn dispatch(
        &self,
        args: KnowledgeToolArgs,
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError>;
}

#[derive(Debug, Default)]
pub struct DefaultKnowledgeRuntime;

impl DefaultKnowledgeRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl KnowledgeRuntime for DefaultKnowledgeRuntime {
    async fn dispatch(
        &self,
        _args: KnowledgeToolArgs,
        _ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        Err(ToolError::ExecutionFailed {
            tool_name: "knowledge".to_string(),
            reason: "knowledge runtime is not wired yet".to_string(),
        })
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
            description: "Explore GitHub-backed knowledge bases progressively through snapshot, tree, search, and node actions.".to_string(),
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
                        "description": "Git reference to resolve, defaults to main"
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
        input: serde_json::Value,
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let args = KnowledgeToolArgs::parse(input).map_err(|err| ToolError::ExecutionFailed {
            tool_name: "knowledge".to_string(),
            reason: err.to_string(),
        })?;

        self.runtime.dispatch(args, ctx).await
    }
}
