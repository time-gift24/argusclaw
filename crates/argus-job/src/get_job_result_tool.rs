//! get_job_result tool implementation.

use std::sync::Arc;

use argus_protocol::{NamedTool, RiskLevel, ToolDefinition, tool::ToolError};
use async_trait::async_trait;

use crate::job_manager::JobManager;

/// Tool for polling job results.
#[derive(Debug)]
pub struct GetJobResultTool {
    job_manager: Arc<JobManager>,
}

impl GetJobResultTool {
    /// Create a new GetJobResultTool.
    pub fn new(job_manager: Arc<JobManager>) -> Self {
        Self { job_manager }
    }
}

#[async_trait]
impl NamedTool for GetJobResultTool {
    fn name(&self) -> &str {
        "get_job_result"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Get the result of a previously dispatched job. Call this after dispatching a job to check if it has completed.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "job_id": {
                        "type": "string",
                        "description": "The job ID returned from dispatch_job"
                    }
                },
                "required": ["job_id"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        // Low risk - only reads job state
        RiskLevel::Low
    }

    async fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let job_id = input
            .get("job_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool_name: self.name().to_string(),
                reason: "missing required parameter: job_id".to_string(),
            })?;

        tracing::debug!("get_job_result called for job_id: {}", job_id);

        match self.job_manager.get_result(job_id).await {
            Ok(Some(result)) => {
                serde_json::to_value(&result).map_err(|e| ToolError::ExecutionFailed {
                    tool_name: self.name().to_string(),
                    reason: e.to_string(),
                })
            }
            Ok(None) => {
                // Job not found or not yet completed
                Ok(serde_json::json!({
                    "job_id": job_id,
                    "status": "pending",
                    "result": null
                }))
            }
            Err(e) => Err(ToolError::ExecutionFailed {
                tool_name: self.name().to_string(),
                reason: e.to_string(),
            }),
        }
    }
}
