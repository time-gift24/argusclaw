//! dispatch_job tool implementation.

use std::sync::Arc;
use std::time::Duration;

use argus_protocol::{NamedTool, RiskLevel, ToolDefinition, tool::ToolError, ToolExecutionContext};
use async_trait::async_trait;
use tokio::time::sleep;

use crate::error::JobError;
use crate::job_manager::JobManager;
use crate::types::JobDispatchArgs;

/// Maximum number of retry attempts for dispatch.
const MAX_RETRIES: u32 = 3;

/// Base delay for exponential backoff (100ms).
const BASE_DELAY_MS: u64 = 100;

/// Tool for dispatching background jobs.
#[derive(Debug)]
pub struct DispatchJobTool {
    job_manager: Arc<JobManager>,
}

impl DispatchJobTool {
    /// Create a new DispatchJobTool.
    pub fn new(job_manager: Arc<JobManager>) -> Self {
        Self { job_manager }
    }
}

#[async_trait]
impl NamedTool for DispatchJobTool {
    fn name(&self) -> &str {
        "dispatch_job"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Dispatch a background job to a subagent. The job runs asynchronously and you will be notified when it completes.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "The prompt/task description for the job"
                    },
                    "agent_id": {
                        "type": "number",
                        "description": "The agent ID to use for this job"
                    },
                    "context": {
                        "type": "object",
                        "description": "Optional context JSON for the job",
                    }
                },
                "required": ["prompt", "agent_id"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        // Medium risk - dispatches background tasks
        RiskLevel::Medium
    }

    async fn execute(&self, input: serde_json::Value, _ctx: Arc<ToolExecutionContext>) -> Result<serde_json::Value, ToolError> {
        // Parse the input
        let args: JobDispatchArgs =
            serde_json::from_value(input).map_err(|e| ToolError::ExecutionFailed {
                tool_name: self.name().to_string(),
                reason: format!("invalid input: {}", e),
            })?;

        tracing::info!(
            "dispatch_job called with prompt length {} for agent {:?}",
            args.prompt.len(),
            args.agent_id
        );

        // Retry loop with exponential backoff
        let mut last_error = None;

        for attempt in 0..=MAX_RETRIES {
            match self.job_manager.dispatch(args.clone()).await {
                Ok(result) => {
                    if attempt > 0 {
                        tracing::info!("dispatch_job succeeded after {} retries", attempt);
                    }
                    return serde_json::to_value(&result).map_err(|e| ToolError::ExecutionFailed {
                        tool_name: self.name().to_string(),
                        reason: e.to_string(),
                    });
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    last_error = Some(e);
                    if attempt == MAX_RETRIES {
                        break;
                    }
                    let delay_ms = BASE_DELAY_MS * 2u64.pow(attempt);
                    tracing::warn!(
                        "dispatch_job attempt {} failed: {}, retrying in {}ms",
                        attempt + 1,
                        error_msg,
                        delay_ms
                    );
                    sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }

        // All retries exhausted
        let final_error = last_error
            .unwrap_or_else(|| JobError::Internal("dispatch failed without error".to_string()));
        tracing::error!(
            "dispatch_job failed after {} retries: {}",
            MAX_RETRIES,
            final_error
        );
        Err(ToolError::ExecutionFailed {
            tool_name: self.name().to_string(),
            reason: JobError::RetryExhausted(final_error.to_string()).to_string(),
        })
    }
}
