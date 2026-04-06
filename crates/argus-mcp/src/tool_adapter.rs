use std::sync::Arc;

use async_trait::async_trait;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{McpDiscoveredToolRecord, NamedTool, ToolError, ToolExecutionContext};

use crate::error::McpRuntimeError;

#[async_trait]
pub trait McpToolExecutor: Send + Sync {
    async fn execute_mcp_tool(
        &self,
        server_id: i64,
        tool_name_original: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, McpRuntimeError>;
}

pub struct McpToolAdapter {
    executor: Arc<dyn McpToolExecutor>,
    server_id: i64,
    tool_name_original: String,
    name: String,
    definition: ToolDefinition,
    risk_level: RiskLevel,
}

impl McpToolAdapter {
    #[must_use]
    pub fn new(
        executor: Arc<dyn McpToolExecutor>,
        server_display_name: &str,
        tool: &McpDiscoveredToolRecord,
    ) -> Self {
        let name = format!(
            "mcp__{}__{}__{}__{}",
            tool.server_id,
            slugify(server_display_name, "server"),
            slugify(&tool.tool_name_original, "tool"),
            stable_suffix(&tool.tool_name_original),
        );

        let description = if tool.description.is_empty() {
            format!(
                "Execute MCP tool '{}' exposed by '{}'.",
                tool.tool_name_original, server_display_name
            )
        } else {
            tool.description.clone()
        };

        Self {
            executor,
            server_id: tool.server_id,
            tool_name_original: tool.tool_name_original.clone(),
            name: name.clone(),
            risk_level: infer_risk_level(tool),
            definition: ToolDefinition {
                name,
                description,
                parameters: tool.schema.clone(),
            },
        }
    }
}

#[async_trait]
impl NamedTool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn risk_level(&self) -> RiskLevel {
        self.risk_level
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        self.executor
            .execute_mcp_tool(self.server_id, &self.tool_name_original, input)
            .await
            .map_err(|error| ToolError::ExecutionFailed {
                tool_name: self.name.clone(),
                reason: error.to_string(),
            })
    }
}

fn slugify(input: &str, fallback: &str) -> String {
    let mut slug = String::with_capacity(input.len());
    let mut last_was_separator = false;

    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_separator = false;
            continue;
        }

        if !last_was_separator && !slug.is_empty() {
            slug.push('_');
            last_was_separator = true;
        }
    }

    while slug.ends_with('_') {
        slug.pop();
    }

    if slug.is_empty() {
        fallback.to_string()
    } else {
        slug
    }
}

fn stable_suffix(input: &str) -> String {
    // FNV-1a keeps the suffix deterministic without adding another dependency.
    let mut hash = 0x811c9dc5_u32;
    for byte in input.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{hash:08x}")
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolAnnotationsHint {
    #[serde(default)]
    read_only_hint: Option<bool>,
    #[serde(default)]
    destructive_hint: Option<bool>,
}

fn infer_risk_level(tool: &McpDiscoveredToolRecord) -> RiskLevel {
    let Some(annotations) = tool.annotations.as_ref() else {
        return RiskLevel::Medium;
    };
    let Ok(annotations) = serde_json::from_value::<ToolAnnotationsHint>(annotations.clone()) else {
        return RiskLevel::Medium;
    };

    if annotations.destructive_hint == Some(true) {
        RiskLevel::High
    } else if annotations.read_only_hint == Some(true) {
        RiskLevel::Low
    } else {
        RiskLevel::Medium
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use tokio::sync::broadcast;

    use argus_protocol::ids::ThreadId;

    use super::*;

    fn tool_record() -> McpDiscoveredToolRecord {
        McpDiscoveredToolRecord {
            server_id: 7,
            tool_name_original: "post-message.v2".to_string(),
            description: "Send a Slack message".to_string(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                }
            }),
            annotations: None,
        }
    }

    fn make_ctx() -> Arc<ToolExecutionContext> {
        let (pipe_tx, _) = broadcast::channel(8);
        Arc::new(ToolExecutionContext {
            thread_id: ThreadId::new(),
            agent_id: None,
            pipe_tx,
        })
    }

    struct FakeExecutor {
        calls: Mutex<Vec<(i64, String, serde_json::Value)>>,
    }

    #[async_trait]
    impl McpToolExecutor for FakeExecutor {
        async fn execute_mcp_tool(
            &self,
            server_id: i64,
            tool_name_original: &str,
            input: serde_json::Value,
        ) -> Result<serde_json::Value, McpRuntimeError> {
            self.calls.lock().unwrap().push((
                server_id,
                tool_name_original.to_string(),
                input.clone(),
            ));
            Ok(serde_json::json!({ "ok": true, "echo": input }))
        }
    }

    #[tokio::test]
    async fn adapter_namespaces_tool_definitions() {
        let adapter = McpToolAdapter::new(
            Arc::new(FakeExecutor {
                calls: Mutex::new(Vec::new()),
            }),
            "Slack Workspace",
            &tool_record(),
        );

        assert_eq!(
            adapter.name(),
            "mcp__7__slack_workspace__post_message_v2__0486ea8d"
        );
        let definition = adapter.definition();
        assert_eq!(
            definition.name,
            "mcp__7__slack_workspace__post_message_v2__0486ea8d"
        );
        assert_eq!(definition.description, "Send a Slack message");
        assert_eq!(definition.parameters["type"], "object");
    }

    #[tokio::test]
    async fn adapter_forwards_execution_to_runtime_executor() {
        let executor = Arc::new(FakeExecutor {
            calls: Mutex::new(Vec::new()),
        });
        let adapter = McpToolAdapter::new(executor.clone(), "Slack Workspace", &tool_record());

        let result = adapter
            .execute(serde_json::json!({ "text": "hello" }), make_ctx())
            .await
            .expect("execution should succeed");

        assert_eq!(result["ok"], true);
        let calls = executor.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, 7);
        assert_eq!(calls[0].1, "post-message.v2");
        assert_eq!(calls[0].2["text"], "hello");
    }

    #[tokio::test]
    async fn adapter_names_are_collision_safe_for_lossy_slugs() {
        let executor = Arc::new(FakeExecutor {
            calls: Mutex::new(Vec::new()),
        });
        let first = McpToolAdapter::new(
            executor.clone(),
            "Slack-Prod",
            &McpDiscoveredToolRecord {
                tool_name_original: "post.message".to_string(),
                ..tool_record()
            },
        );
        let second = McpToolAdapter::new(
            executor,
            "Slack Prod",
            &McpDiscoveredToolRecord {
                tool_name_original: "post_message".to_string(),
                ..tool_record()
            },
        );

        assert_ne!(first.name(), second.name());
    }

    #[tokio::test]
    async fn adapter_uses_conservative_risk_defaults() {
        let adapter = McpToolAdapter::new(
            Arc::new(FakeExecutor {
                calls: Mutex::new(Vec::new()),
            }),
            "Slack Workspace",
            &McpDiscoveredToolRecord {
                annotations: None,
                ..tool_record()
            },
        );
        assert_eq!(adapter.risk_level(), RiskLevel::Medium);

        let read_only_adapter = McpToolAdapter::new(
            Arc::new(FakeExecutor {
                calls: Mutex::new(Vec::new()),
            }),
            "Slack Workspace",
            &McpDiscoveredToolRecord {
                annotations: Some(serde_json::json!({
                    "readOnlyHint": true,
                    "destructiveHint": false
                })),
                ..tool_record()
            },
        );
        assert_eq!(read_only_adapter.risk_level(), RiskLevel::Low);

        let conflicting_adapter = McpToolAdapter::new(
            Arc::new(FakeExecutor {
                calls: Mutex::new(Vec::new()),
            }),
            "Slack Workspace",
            &McpDiscoveredToolRecord {
                annotations: Some(serde_json::json!({
                    "readOnlyHint": true,
                    "destructiveHint": true
                })),
                ..tool_record()
            },
        );
        assert_eq!(conflicting_adapter.risk_level(), RiskLevel::High);
    }
}
