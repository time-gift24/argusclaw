//! UpdatePlanTool — LLM-usable tool for managing a per-Thread task plan.
//!
//! The tool accepts a full plan snapshot from the LLM, overwrites the Thread's
//! plan state, and returns the updated plan with metadata.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json, to_value};

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext, UpdatePlanArgs};

use super::plan_store::FilePlanStore;

/// A tool that lets the LLM update a per-Thread task plan.
///
/// The tool operates through FilePlanStore, which handles both in-memory storage
/// and file persistence for plan data.
pub struct UpdatePlanTool {
    /// FilePlanStore (shared with Thread).
    store: Arc<FilePlanStore>,
}

impl UpdatePlanTool {
    /// Create a new UpdatePlanTool backed by the given FilePlanStore.
    #[must_use]
    pub fn new(store: Arc<FilePlanStore>) -> Self {
        Self { store }
    }
}

impl Default for UpdatePlanTool {
    fn default() -> Self {
        Self::new(Arc::new(FilePlanStore::default()))
    }
}

#[async_trait]
impl NamedTool for UpdatePlanTool {
    fn name(&self) -> &str {
        "update_plan"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "update_plan".to_string(),
            description: "Update the task plan for this thread. The LLM sends the complete plan on each call, which fully overwrites the previous state. Use this to track and report progress on multi-step tasks.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "explanation": {
                        "type": "string",
                        "description": "Optional explanation for this plan update (logged but not stored)"
                    },
                    "plan": {
                        "type": "array",
                        "description": "The complete plan snapshot",
                        "items": {
                            "type": "object",
                            "properties": {
                                "step": {
                                    "type": "string",
                                    "description": "Description of the step"
                                },
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "completed"],
                                    "description": "Current status of the step"
                                }
                            },
                            "required": ["step", "status"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["plan"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: Arc<ToolExecutionContext>,
    ) -> Result<Value, ToolError> {
        // Parse arguments
        let args: UpdatePlanArgs =
            serde_json::from_value(input).map_err(|e| ToolError::ExecutionFailed {
                tool_name: "update_plan".to_string(),
                reason: format!("Invalid arguments: {}", e),
            })?;

        // Log explanation if provided
        if let Some(ref explanation) = args.explanation {
            tracing::debug!(explanation = %explanation, "update_plan explanation");
        }

        // Reject empty plan
        if args.plan.is_empty() {
            return Err(ToolError::ExecutionFailed {
                tool_name: "update_plan".to_string(),
                reason: "Plan cannot be empty".to_string(),
            });
        }

        // Serialize plan items for the result
        let plan_values: Vec<Value> = args.plan.iter().map(to_value).map(Result::unwrap).collect();

        // Update store (persists to file)
        self.store.write_from_items(args.plan);

        // Return updated plan with metadata
        Ok(json!({
            "plan": plan_values,
            "updated": plan_values.len(),
            "total": plan_values.len()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_protocol::ids::ThreadId;
    use tokio::sync::broadcast;

    fn make_tool() -> UpdatePlanTool {
        UpdatePlanTool::default()
    }

    fn make_ctx() -> Arc<ToolExecutionContext> {
        let (tx, _) = broadcast::channel(16);
        Arc::new(ToolExecutionContext {
            thread_id: ThreadId::new(),
            pipe_tx: tx,
        })
    }

    #[tokio::test]
    async fn name() {
        assert_eq!(make_tool().name(), "update_plan");
    }

    #[tokio::test]
    async fn risk_level_is_low() {
        assert_eq!(make_tool().risk_level(), RiskLevel::Low);
    }

    #[tokio::test]
    async fn definition_has_required_fields() {
        let def = make_tool().definition();
        assert_eq!(def.name, "update_plan");
        let params = &def.parameters;
        assert!(
            params
                .get("properties")
                .unwrap()
                .as_object()
                .unwrap()
                .contains_key("plan")
        );
        let plan_schema = params["properties"]["plan"].as_object().unwrap();
        assert!(
            plan_schema["items"].as_object().unwrap()["properties"]
                .as_object()
                .unwrap()
                .contains_key("status")
        );
    }

    #[tokio::test]
    async fn execute_single_item() {
        let store = Arc::new(FilePlanStore::default());
        let tool = UpdatePlanTool::new(store.clone());

        let args = json!({
            "plan": [{
                "step": "Implement feature X",
                "status": "completed"
            }]
        });

        let result = tool.execute(args, make_ctx()).await.unwrap();
        assert_eq!(result["updated"], 1);
        assert_eq!(result["total"], 1);
        assert_eq!(result["plan"].as_array().unwrap().len(), 1);
        assert_eq!(store.store().read().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn execute_multiple_items() {
        let store = Arc::new(FilePlanStore::default());
        let tool = UpdatePlanTool::new(store.clone());

        let args = json!({
            "plan": [
                { "step": "Step 1", "status": "completed" },
                { "step": "Step 2", "status": "in_progress" },
                { "step": "Step 3", "status": "pending" }
            ]
        });

        let result = tool.execute(args, make_ctx()).await.unwrap();
        assert_eq!(result["updated"], 3);
        assert_eq!(result["total"], 3);
        assert_eq!(store.store().read().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn execute_with_explanation() {
        let store = Arc::new(FilePlanStore::default());
        let tool = UpdatePlanTool::new(store.clone());

        let args = json!({
            "explanation": "Finished step 1",
            "plan": [{ "step": "Step 1", "status": "completed" }]
        });

        let result = tool.execute(args, make_ctx()).await.unwrap();
        // Explanation should NOT appear in result
        assert!(!result.as_object().unwrap().contains_key("explanation"));
        assert_eq!(result["total"], 1);
    }

    #[tokio::test]
    async fn execute_empty_plan_rejected() {
        let tool = make_tool();
        let args = json!({ "plan": [] });
        let result = tool.execute(args, make_ctx()).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            ToolError::ExecutionFailed { reason, .. } => {
                assert!(reason.to_lowercase().contains("empty"));
            }
            _ => panic!("Expected ExecutionFailed"),
        }
    }

    #[tokio::test]
    async fn execute_invalid_status_rejected() {
        let tool = make_tool();
        let args = json!({
            "plan": [{ "step": "Test", "status": "invalid_status" }]
        });
        let result = tool.execute(args, make_ctx()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn execute_overwrites_previous() {
        let store = Arc::new(FilePlanStore::default());
        let tool = UpdatePlanTool::new(store.clone());

        // First update
        let args1 = json!({
            "plan": [{ "step": "Step A", "status": "pending" }]
        });
        tool.execute(args1, make_ctx()).await.unwrap();
        assert_eq!(store.store().read().unwrap().len(), 1);

        // Second update (different content)
        let args2 = json!({
            "plan": [
                { "step": "Step A", "status": "completed" },
                { "step": "Step B", "status": "pending" }
            ]
        });
        tool.execute(args2, make_ctx()).await.unwrap();
        assert_eq!(store.store().read().unwrap().len(), 2);

        // Full overwrite: both items should be present
        let store_ref = store.store();
        let items = store_ref.read().unwrap();
        let steps: Vec<&str> = items.iter().map(|v| v["step"].as_str().unwrap()).collect();
        assert!(steps.contains(&"Step A"));
        assert!(steps.contains(&"Step B"));
    }

    #[tokio::test]
    async fn execute_unknown_field_rejected() {
        let tool = make_tool();
        let args = json!({
            "plan": [{ "step": "Test", "status": "pending", "extra": "field" }]
        });
        let result = tool.execute(args, make_ctx()).await;
        assert!(result.is_err());
    }
}
