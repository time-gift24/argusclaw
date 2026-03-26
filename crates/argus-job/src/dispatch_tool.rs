//! dispatch_job tool implementation.

use std::sync::Arc;

use argus_protocol::{
    NamedTool, RiskLevel, ThreadEvent, ToolDefinition, ToolError, ToolExecutionContext,
};
use async_trait::async_trait;
use uuid::Uuid;

use crate::job_manager::JobManager;
use crate::types::JobDispatchArgs;

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

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let args: JobDispatchArgs = serde_json::from_value(input).map_err(|e| ToolError::ExecutionFailed {
            tool_name: self.name().to_string(),
            reason: format!("invalid input: {}", e),
        })?;

        let job_id = Uuid::new_v4().to_string();

        tracing::info!(
            "dispatch_job called: job_id={}, prompt_len={}, agent_id={:?}",
            job_id,
            args.prompt.len(),
            args.agent_id
        );

        // Send JobDispatched into the pipe (includes thread_id for desktop routing)
        let dispatch_event = ThreadEvent::JobDispatched {
            thread_id: ctx.thread_id,
            job_id: job_id.clone(),
            agent_id: args.agent_id,
            prompt: args.prompt.clone(),
            context: args.context.clone(),
        };
        if let Err(e) = ctx.pipe_tx.send(dispatch_event) {
            tracing::warn!("failed to send JobDispatched event: {}", e);
        }

        // Spawn background executor using the JobManager's spawn method
        self.job_manager
            .spawn_job_executor(
                ctx.thread_id,
                job_id.clone(),
                args.agent_id,
                args.prompt,
                args.context,
                ctx.pipe_tx.clone(),
                ctx.control_tx.clone(),
            )
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool_name: self.name().to_string(),
                reason: e.to_string(),
            })?;

        Ok(serde_json::json!({
            "job_id": job_id,
            "status": "dispatched"
        }))
    }
}
