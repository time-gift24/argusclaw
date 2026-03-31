//! Unified scheduler tool implementation.
//!
//! This tool consolidates subagent scheduling operations behind one entrypoint:
//! - `dispatch_job`
//! - `list_subagents`
//! - `get_job_result`

use std::sync::Arc;

use argus_protocol::{
    AgentId, NamedTool, RiskLevel, ThreadControlEvent, ThreadEvent, ThreadId, ToolDefinition,
    ToolError, ToolExecutionContext,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};

#[derive(Debug, Deserialize)]
#[serde(tag = "action")]
enum SchedulerInput {
    #[serde(rename = "dispatch_job", alias = "dispath_job")]
    DispatchJob {
        prompt: String,
        agent_id: AgentId,
        #[serde(default)]
        context: Option<serde_json::Value>,
    },
    #[serde(rename = "list_subagents")]
    ListSubagents,
    #[serde(rename = "get_job_result")]
    GetJobResult {
        job_id: String,
        #[serde(default)]
        consume: Option<bool>,
    },
}

/// Serialized job result payload returned by scheduler lookups.
#[derive(Debug, Clone, Serialize)]
pub struct SchedulerJobResult {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<argus_protocol::TokenUsage>,
    pub agent_id: AgentId,
    pub agent_display_name: String,
    pub agent_description: String,
}

/// Scheduler lookup status for a specific job ID.
#[derive(Debug, Clone)]
pub enum SchedulerJobLookup {
    NotFound,
    Pending,
    Completed(SchedulerJobResult),
    Consumed(SchedulerJobResult),
}

/// Serialized subagent metadata returned by scheduler listing.
#[derive(Debug, Clone, Serialize)]
pub struct SchedulerSubagent {
    pub agent_id: AgentId,
    pub display_name: String,
    pub description: String,
}

/// Request payload for dispatching a background subagent job.
#[derive(Debug, Clone)]
pub struct SchedulerDispatchRequest {
    pub thread_id: ThreadId,
    pub prompt: String,
    pub agent_id: AgentId,
    pub context: Option<serde_json::Value>,
    pub pipe_tx: broadcast::Sender<ThreadEvent>,
    pub control_tx: mpsc::UnboundedSender<ThreadControlEvent>,
}

/// Request payload for looking up a background job.
#[derive(Debug, Clone)]
pub struct SchedulerLookupRequest {
    pub thread_id: ThreadId,
    pub job_id: String,
    pub consume: bool,
    pub control_tx: mpsc::UnboundedSender<ThreadControlEvent>,
}

/// Backend integration point implemented by orchestration crates.
#[async_trait]
pub trait SchedulerBackend: Send + Sync {
    async fn dispatch_job(&self, request: SchedulerDispatchRequest) -> Result<String, ToolError>;

    async fn list_subagents(&self, agent_id: AgentId) -> Result<Vec<SchedulerSubagent>, ToolError>;

    async fn get_job_result(
        &self,
        request: SchedulerLookupRequest,
    ) -> Result<SchedulerJobLookup, ToolError>;
}

/// Tool for scheduling and querying background subagent work.
pub struct SchedulerTool {
    backend: Arc<dyn SchedulerBackend>,
}

impl std::fmt::Debug for SchedulerTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchedulerTool").finish()
    }
}

impl SchedulerTool {
    #[must_use]
    pub fn new(backend: Arc<dyn SchedulerBackend>) -> Self {
        Self { backend }
    }

    fn format_lookup_response(
        &self,
        job_id: &str,
        lookup: SchedulerJobLookup,
    ) -> Result<serde_json::Value, ToolError> {
        match lookup {
            SchedulerJobLookup::NotFound => Ok(serde_json::json!({
                "job_id": job_id,
                "status": "not_found",
            })),
            SchedulerJobLookup::Pending => Ok(serde_json::json!({
                "job_id": job_id,
                "status": "pending",
            })),
            SchedulerJobLookup::Completed(result) => Ok(serde_json::json!({
                "job_id": job_id,
                "status": "completed",
                "result": serde_json::to_value(result).map_err(|error| ToolError::ExecutionFailed {
                    tool_name: self.name().to_string(),
                    reason: format!("failed to serialize job result: {error}"),
                })?,
            })),
            SchedulerJobLookup::Consumed(result) => Ok(serde_json::json!({
                "job_id": job_id,
                "status": "consumed",
                "result": serde_json::to_value(result).map_err(|error| ToolError::ExecutionFailed {
                    tool_name: self.name().to_string(),
                    reason: format!("failed to serialize job result: {error}"),
                })?,
            })),
        }
    }
}

#[async_trait]
impl NamedTool for SchedulerTool {
    fn name(&self) -> &str {
        "scheduler"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Unified scheduler skill for subagent orchestration. Supports list_subagents, dispatch_job, and get_job_result operations.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list_subagents", "dispatch_job", "dispath_job", "get_job_result"],
                        "description": "Scheduler operation to perform"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Task prompt for dispatch_job"
                    },
                    "agent_id": {
                        "type": "number",
                        "description": "Subagent ID for dispatch_job"
                    },
                    "context": {
                        "type": "object",
                        "description": "Optional context payload for dispatch_job"
                    },
                    "job_id": {
                        "type": "string",
                        "description": "Job ID for get_job_result"
                    },
                    "consume": {
                        "type": "boolean",
                        "description": "When true, consume result and prevent queued replay"
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
        let args: SchedulerInput =
            serde_json::from_value(input).map_err(|error| ToolError::ExecutionFailed {
                tool_name: self.name().to_string(),
                reason: format!("invalid input: {error}"),
            })?;

        match args {
            SchedulerInput::DispatchJob {
                prompt,
                agent_id,
                context,
            } => {
                let job_id = self
                    .backend
                    .dispatch_job(SchedulerDispatchRequest {
                        thread_id: ctx.thread_id,
                        prompt,
                        agent_id,
                        context,
                        pipe_tx: ctx.pipe_tx.clone(),
                        control_tx: ctx.control_tx.clone(),
                    })
                    .await?;

                Ok(serde_json::json!({
                    "job_id": job_id,
                    "status": "dispatched"
                }))
            }
            SchedulerInput::ListSubagents => {
                let agent_id = ctx.agent_id.ok_or_else(|| ToolError::ExecutionFailed {
                    tool_name: self.name().to_string(),
                    reason: "current agent_id not available in tool context".to_string(),
                })?;

                let subagents = self.backend.list_subagents(agent_id).await?;
                serde_json::to_value(subagents).map_err(|error| ToolError::ExecutionFailed {
                    tool_name: self.name().to_string(),
                    reason: format!("failed to serialize subagents: {error}"),
                })
            }
            SchedulerInput::GetJobResult { job_id, consume } => {
                let lookup = self
                    .backend
                    .get_job_result(SchedulerLookupRequest {
                        thread_id: ctx.thread_id,
                        job_id: job_id.clone(),
                        consume: consume.unwrap_or(false),
                        control_tx: ctx.control_tx.clone(),
                    })
                    .await?;
                self.format_lookup_response(&job_id, lookup)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use argus_protocol::ThreadJobResult;
    use tokio::sync::broadcast;

    #[derive(Debug, Clone)]
    struct RecordedDispatch {
        thread_id: ThreadId,
        prompt: String,
        agent_id: AgentId,
        context: Option<serde_json::Value>,
    }

    struct MockSchedulerBackend {
        dispatch_job_id: String,
        dispatch_calls: Mutex<Vec<RecordedDispatch>>,
        list_response: Vec<SchedulerSubagent>,
        lookup_response: SchedulerJobLookup,
    }

    #[async_trait]
    impl SchedulerBackend for MockSchedulerBackend {
        async fn dispatch_job(
            &self,
            request: SchedulerDispatchRequest,
        ) -> Result<String, ToolError> {
            self.dispatch_calls
                .lock()
                .expect("dispatch_calls mutex poisoned")
                .push(RecordedDispatch {
                    thread_id: request.thread_id,
                    prompt: request.prompt,
                    agent_id: request.agent_id,
                    context: request.context,
                });
            Ok(self.dispatch_job_id.clone())
        }

        async fn list_subagents(
            &self,
            _agent_id: AgentId,
        ) -> Result<Vec<SchedulerSubagent>, ToolError> {
            Ok(self.list_response.clone())
        }

        async fn get_job_result(
            &self,
            _request: SchedulerLookupRequest,
        ) -> Result<SchedulerJobLookup, ToolError> {
            Ok(self.lookup_response.clone())
        }
    }

    fn make_ctx(agent_id: Option<AgentId>) -> Arc<ToolExecutionContext> {
        let (pipe_tx, _) = broadcast::channel(8);
        let (control_tx, _control_rx) = tokio::sync::mpsc::unbounded_channel();
        Arc::new(ToolExecutionContext {
            thread_id: ThreadId::new(),
            agent_id,
            pipe_tx,
            control_tx,
        })
    }

    fn sample_result() -> SchedulerJobResult {
        SchedulerJobResult {
            success: true,
            message: "finished".to_string(),
            token_usage: None,
            agent_id: AgentId::new(7),
            agent_display_name: "Worker".to_string(),
            agent_description: "Background worker".to_string(),
        }
    }

    #[test]
    fn scheduler_name_and_risk_level() {
        let backend = Arc::new(MockSchedulerBackend {
            dispatch_job_id: "job-1".to_string(),
            dispatch_calls: Mutex::new(Vec::new()),
            list_response: Vec::new(),
            lookup_response: SchedulerJobLookup::Pending,
        });
        let tool = SchedulerTool::new(backend);
        assert_eq!(tool.name(), "scheduler");
        assert_eq!(tool.risk_level(), RiskLevel::Medium);
    }

    #[tokio::test]
    async fn dispatch_job_action_calls_backend_and_returns_job_id() {
        let backend = Arc::new(MockSchedulerBackend {
            dispatch_job_id: "job-42".to_string(),
            dispatch_calls: Mutex::new(Vec::new()),
            list_response: Vec::new(),
            lookup_response: SchedulerJobLookup::Pending,
        });
        let tool = SchedulerTool::new(backend.clone());
        let ctx = make_ctx(Some(AgentId::new(99)));
        let thread_id = ctx.thread_id;

        let response = tool
            .execute(
                serde_json::json!({
                    "action": "dispatch_job",
                    "prompt": "summarize logs",
                    "agent_id": 7,
                    "context": {"env": "staging"}
                }),
                ctx,
            )
            .await
            .expect("dispatch_job should succeed");

        assert_eq!(response["job_id"], serde_json::json!("job-42"));
        assert_eq!(response["status"], serde_json::json!("dispatched"));

        let calls = backend
            .dispatch_calls
            .lock()
            .expect("dispatch_calls mutex poisoned");
        assert_eq!(calls.len(), 1);
        let call = &calls[0];
        assert_eq!(call.thread_id, thread_id);
        assert_eq!(call.prompt, "summarize logs");
        assert_eq!(call.agent_id, AgentId::new(7));
        assert_eq!(call.context, Some(serde_json::json!({"env": "staging"})));
    }

    #[tokio::test]
    async fn list_subagents_requires_agent_id_in_context() {
        let backend = Arc::new(MockSchedulerBackend {
            dispatch_job_id: "job-42".to_string(),
            dispatch_calls: Mutex::new(Vec::new()),
            list_response: Vec::new(),
            lookup_response: SchedulerJobLookup::Pending,
        });
        let tool = SchedulerTool::new(backend);

        let error = tool
            .execute(
                serde_json::json!({"action": "list_subagents"}),
                make_ctx(None),
            )
            .await
            .expect_err("missing agent_id should fail");

        assert!(
            error
                .to_string()
                .contains("current agent_id not available in tool context")
        );
    }

    #[tokio::test]
    async fn get_job_result_action_formats_completed_payload() {
        let backend = Arc::new(MockSchedulerBackend {
            dispatch_job_id: "job-42".to_string(),
            dispatch_calls: Mutex::new(Vec::new()),
            list_response: Vec::new(),
            lookup_response: SchedulerJobLookup::Completed(sample_result()),
        });
        let tool = SchedulerTool::new(backend);

        let response = tool
            .execute(
                serde_json::json!({
                    "action": "get_job_result",
                    "job_id": "job-42",
                    "consume": true
                }),
                make_ctx(Some(AgentId::new(9))),
            )
            .await
            .expect("get_job_result should succeed");

        assert_eq!(response["job_id"], serde_json::json!("job-42"));
        assert_eq!(response["status"], serde_json::json!("completed"));
        assert_eq!(response["result"]["message"], serde_json::json!("finished"));
    }

    #[test]
    fn thread_job_result_maps_to_scheduler_payload_shape() {
        let thread_result = ThreadJobResult {
            job_id: "job-8".to_string(),
            success: true,
            message: "ok".to_string(),
            token_usage: None,
            agent_id: AgentId::new(5),
            agent_display_name: "Worker".to_string(),
            agent_description: "Background worker".to_string(),
        };

        let scheduler_payload = SchedulerJobResult {
            success: thread_result.success,
            message: thread_result.message,
            token_usage: thread_result.token_usage,
            agent_id: thread_result.agent_id,
            agent_display_name: thread_result.agent_display_name,
            agent_description: thread_result.agent_description,
        };

        assert_eq!(scheduler_payload.agent_id, AgentId::new(5));
        assert_eq!(scheduler_payload.message, "ok");
    }
}
