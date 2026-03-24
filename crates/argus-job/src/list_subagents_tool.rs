//! list_subagents tool implementation.

use std::sync::Arc;

use argus_protocol::{NamedTool, RiskLevel, ToolDefinition, tool::ToolError, AgentId};
use async_trait::async_trait;
use argus_template::TemplateManager;

/// Tool for listing subagents of the current agent.
#[derive(Clone)]
pub struct ListSubagentsTool {
    template_manager: Arc<TemplateManager>,
}

impl std::fmt::Debug for ListSubagentsTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListSubagentsTool").finish()
    }
}

impl ListSubagentsTool {
    /// Create a new ListSubagentsTool.
    pub fn new(template_manager: Arc<TemplateManager>) -> Self {
        Self { template_manager }
    }
}

#[async_trait]
impl NamedTool for ListSubagentsTool {
    fn name(&self) -> &str {
        "list_subagents"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "List all subagents that belong to this agent. Returns the agent_id, display_name, and description of each subagent.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "number",
                        "description": "The agent ID of the parent agent whose subagents to list"
                    }
                },
                "required": ["agent_id"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    async fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let agent_id = input
            .get("agent_id")
            .and_then(|v| v.as_i64())
            .map(AgentId::new)
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool_name: self.name().to_string(),
                reason: "missing required parameter: agent_id".to_string(),
            })?;

        tracing::debug!("list_subagents called for agent {:?}", agent_id);

        let subagents = self
            .template_manager
            .list_subagents(agent_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool_name: self.name().to_string(),
                reason: e.to_string(),
            })?;

        let result: Vec<_> = subagents
            .into_iter()
            .map(|a| {
                serde_json::json!({
                    "agent_id": a.id.inner(),
                    "display_name": a.display_name,
                    "description": a.description,
                })
            })
            .collect();

        Ok(serde_json::to_value(&result).map_err(|e| ToolError::ExecutionFailed {
            tool_name: self.name().to_string(),
            reason: e.to_string(),
        })?)
    }
}
