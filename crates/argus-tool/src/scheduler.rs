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
use schemars::JsonSchema;
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

/// Arguments for dispatch_job.
#[derive(Debug, Deserialize, JsonSchema)]
struct DispatchJobArgs {
    /// The prompt/task description for the job
    prompt: String,
    /// The agent ID to use for this job
    agent_id: AgentId,
    /// Optional context JSON for the job
    #[serde(default)]
    context: Option<serde_json::Value>,
}

/// Arguments for get_job_result.
#[derive(Debug, Deserialize, JsonSchema)]
struct GetJobResultArgs {
    /// The job ID returned by dispatch_job
    job_id: String,
    /// When true, consume a completed queued result so it will not be auto-injected into a later turn
    #[serde(default)]
    consume: Option<bool>,
}

/// Empty arguments for list_subagents.
#[derive(Debug, Deserialize, JsonSchema)]
struct ListSubagentsArgs {}

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

    async fn list_subagents(&self) -> Result<Vec<SchedulerSubagent>, ToolError>;

    async fn get_job_result(
        &self,
        request: SchedulerLookupRequest,
    ) -> Result<SchedulerJobLookup, ToolError>;
}

fn parse_input<T: serde::de::DeserializeOwned>(
    input: serde_json::Value,
    tool_name: &str,
) -> Result<T, ToolError> {
    serde_json::from_value(input).map_err(|error| ToolError::ExecutionFailed {
        tool_name: tool_name.to_string(),
        reason: format!("invalid input: {error}"),
    })
}

fn serialize_value<T: Serialize>(
    value: T,
    tool_name: &str,
    target: &str,
) -> Result<serde_json::Value, ToolError> {
    serde_json::to_value(value).map_err(|error| ToolError::ExecutionFailed {
        tool_name: tool_name.to_string(),
        reason: format!("failed to serialize {target}: {error}"),
    })
}

fn scheduler_dispatch_variant() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["dispatch_job", "dispath_job"],
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
            }
        },
        "required": ["action", "prompt", "agent_id"],
        "additionalProperties": false
    })
}

fn scheduler_list_variant() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "action": {
                "const": "list_subagents",
                "description": "Scheduler operation to perform"
            }
        },
        "required": ["action"],
        "additionalProperties": false
    })
}

fn scheduler_get_result_variant() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "action": {
                "const": "get_job_result",
                "description": "Scheduler operation to perform"
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
        "required": ["action", "job_id"],
        "additionalProperties": false
    })
}

fn scheduler_definition() -> ToolDefinition {
    ToolDefinition {
        name: "scheduler".to_string(),
        description: "Unified scheduler skill for subagent orchestration. Supports list_subagents, dispatch_job, and get_job_result operations.".to_string(),
        parameters: serde_json::json!({
            "oneOf": [
                scheduler_dispatch_variant(),
                scheduler_list_variant(),
                scheduler_get_result_variant()
            ]
        }),
    }
}

fn legacy_dispatch_definition(name: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: "Dispatch a background job to a subagent. The job runs asynchronously; use get_job_result(job_id, consume=true) if you want to proactively check for completion and consume the result before it is replayed as a later queued message.".to_string(),
        parameters: serde_json::to_value(schemars::schema_for!(DispatchJobArgs))
            .unwrap_or_else(|_| serde_json::json!({"type": "object"})),
    }
}

fn legacy_list_subagents_definition() -> ToolDefinition {
    ToolDefinition {
        name: "list_subagents".to_string(),
        description: "List all subagents that belong to this agent. Returns the agent_id, display_name, and description of each subagent.".to_string(),
        parameters: serde_json::to_value(schemars::schema_for!(ListSubagentsArgs))
            .unwrap_or_else(|_| serde_json::json!({"type": "object"})),
    }
}

fn legacy_get_job_result_definition() -> ToolDefinition {
    ToolDefinition {
        name: "get_job_result".to_string(),
        description: "Check whether a background job has finished. Use consume=true when you are ready to use the result now and do not want it replayed as a future queued message.".to_string(),
        parameters: serde_json::to_value(schemars::schema_for!(GetJobResultArgs))
            .unwrap_or_else(|_| serde_json::json!({"type": "object"})),
    }
}

async fn execute_dispatch_job(
    backend: &Arc<dyn SchedulerBackend>,
    input: serde_json::Value,
    ctx: Arc<ToolExecutionContext>,
    tool_name: &str,
) -> Result<serde_json::Value, ToolError> {
    let args: DispatchJobArgs = parse_input(input, tool_name)?;

    dispatch_job_request(backend, ctx, args.prompt, args.agent_id, args.context).await
}

async fn dispatch_job_request(
    backend: &Arc<dyn SchedulerBackend>,
    ctx: Arc<ToolExecutionContext>,
    prompt: String,
    agent_id: AgentId,
    context: Option<serde_json::Value>,
) -> Result<serde_json::Value, ToolError> {
    let job_id = backend
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

async fn execute_list_subagents(
    backend: &Arc<dyn SchedulerBackend>,
    tool_name: &str,
) -> Result<serde_json::Value, ToolError> {
    let subagents = backend.list_subagents().await?;
    serialize_value(subagents, tool_name, "subagents")
}

async fn execute_get_job_result(
    backend: &Arc<dyn SchedulerBackend>,
    input: serde_json::Value,
    ctx: Arc<ToolExecutionContext>,
    tool_name: &str,
) -> Result<serde_json::Value, ToolError> {
    let args: GetJobResultArgs = parse_input(input, tool_name)?;

    get_job_result_request(
        backend,
        ctx,
        args.job_id,
        args.consume.unwrap_or(false),
        tool_name,
    )
    .await
}

async fn get_job_result_request(
    backend: &Arc<dyn SchedulerBackend>,
    ctx: Arc<ToolExecutionContext>,
    job_id: String,
    consume: bool,
    tool_name: &str,
) -> Result<serde_json::Value, ToolError> {
    let lookup = backend
        .get_job_result(SchedulerLookupRequest {
            thread_id: ctx.thread_id,
            job_id: job_id.clone(),
            consume,
            control_tx: ctx.control_tx.clone(),
        })
        .await?;

    format_lookup_response(tool_name, &job_id, lookup)
}

fn format_lookup_response(
    tool_name: &str,
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
            "result": serialize_value(result, tool_name, "job result")?,
        })),
        SchedulerJobLookup::Consumed(result) => Ok(serde_json::json!({
            "job_id": job_id,
            "status": "consumed",
            "result": serialize_value(result, tool_name, "job result")?,
        })),
    }
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
}

#[async_trait]
impl NamedTool for SchedulerTool {
    fn name(&self) -> &str {
        "scheduler"
    }

    fn definition(&self) -> ToolDefinition {
        scheduler_definition()
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let args: SchedulerInput = parse_input(input.clone(), self.name())?;

        match args {
            SchedulerInput::DispatchJob {
                prompt,
                agent_id,
                context,
            } => dispatch_job_request(&self.backend, ctx, prompt, agent_id, context).await,
            SchedulerInput::ListSubagents => {
                execute_list_subagents(&self.backend, self.name()).await
            }
            SchedulerInput::GetJobResult { job_id, consume } => {
                get_job_result_request(
                    &self.backend,
                    ctx,
                    job_id,
                    consume.unwrap_or(false),
                    self.name(),
                )
                .await
            }
        }
    }
}

/// Compatibility wrapper for the legacy `dispatch_job` tool name.
pub struct DispatchJobTool {
    backend: Arc<dyn SchedulerBackend>,
    name: &'static str,
}

impl std::fmt::Debug for DispatchJobTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DispatchJobTool")
            .field("name", &self.name)
            .finish()
    }
}

impl DispatchJobTool {
    #[must_use]
    pub fn new(backend: Arc<dyn SchedulerBackend>) -> Self {
        Self::with_name("dispatch_job", backend)
    }

    #[must_use]
    pub fn with_name(name: &'static str, backend: Arc<dyn SchedulerBackend>) -> Self {
        Self { backend, name }
    }
}

#[async_trait]
impl NamedTool for DispatchJobTool {
    fn name(&self) -> &str {
        self.name
    }

    fn definition(&self) -> ToolDefinition {
        legacy_dispatch_definition(self.name)
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        execute_dispatch_job(&self.backend, input, ctx, self.name()).await
    }
}

/// Compatibility wrapper for the legacy `list_subagents` tool name.
pub struct ListSubagentsTool {
    backend: Arc<dyn SchedulerBackend>,
}

impl std::fmt::Debug for ListSubagentsTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListSubagentsTool").finish()
    }
}

impl ListSubagentsTool {
    #[must_use]
    pub fn new(backend: Arc<dyn SchedulerBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl NamedTool for ListSubagentsTool {
    fn name(&self) -> &str {
        "list_subagents"
    }

    fn definition(&self) -> ToolDefinition {
        legacy_list_subagents_definition()
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        execute_list_subagents(&self.backend, self.name()).await
    }
}

/// Compatibility wrapper for the legacy `get_job_result` tool name.
pub struct GetJobResultTool {
    backend: Arc<dyn SchedulerBackend>,
}

impl std::fmt::Debug for GetJobResultTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GetJobResultTool").finish()
    }
}

impl GetJobResultTool {
    #[must_use]
    pub fn new(backend: Arc<dyn SchedulerBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl NamedTool for GetJobResultTool {
    fn name(&self) -> &str {
        "get_job_result"
    }

    fn definition(&self) -> ToolDefinition {
        legacy_get_job_result_definition()
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        execute_get_job_result(&self.backend, input, ctx, self.name()).await
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

        async fn list_subagents(&self) -> Result<Vec<SchedulerSubagent>, ToolError> {
            Ok(self.list_response.clone())
        }

        async fn get_job_result(
            &self,
            _request: SchedulerLookupRequest,
        ) -> Result<SchedulerJobLookup, ToolError> {
            Ok(self.lookup_response.clone())
        }
    }

    fn make_ctx() -> Arc<ToolExecutionContext> {
        let (pipe_tx, _) = broadcast::channel(8);
        let (control_tx, _control_rx) = tokio::sync::mpsc::unbounded_channel();
        Arc::new(ToolExecutionContext {
            thread_id: ThreadId::new(),
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

    #[test]
    fn scheduler_definition_declares_action_specific_required_fields() {
        let backend = Arc::new(MockSchedulerBackend {
            dispatch_job_id: "job-1".to_string(),
            dispatch_calls: Mutex::new(Vec::new()),
            list_response: Vec::new(),
            lookup_response: SchedulerJobLookup::Pending,
        });
        let tool = SchedulerTool::new(backend);
        let definition = tool.definition();
        let variants = definition.parameters["oneOf"]
            .as_array()
            .expect("scheduler definition should use oneOf variants");

        let dispatch_variant = variants
            .iter()
            .find(|variant| {
                variant["properties"]["action"]["enum"]
                    .as_array()
                    .is_some_and(|values| {
                        values.contains(&serde_json::json!("dispatch_job"))
                            && values.contains(&serde_json::json!("dispath_job"))
                    })
            })
            .expect("dispatch variant should exist");
        assert_eq!(
            dispatch_variant["required"],
            serde_json::json!(["action", "prompt", "agent_id"])
        );

        let get_result_variant = variants
            .iter()
            .find(|variant| {
                variant["properties"]["action"]["const"] == serde_json::json!("get_job_result")
            })
            .expect("get_job_result variant should exist");
        assert_eq!(
            get_result_variant["required"],
            serde_json::json!(["action", "job_id"])
        );
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
        let ctx = make_ctx();
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
    async fn list_subagents_action_uses_backend_without_context_agent_id() {
        let backend = Arc::new(MockSchedulerBackend {
            dispatch_job_id: "job-42".to_string(),
            dispatch_calls: Mutex::new(Vec::new()),
            list_response: vec![SchedulerSubagent {
                agent_id: AgentId::new(3),
                display_name: "Planner".to_string(),
                description: "Plans work".to_string(),
            }],
            lookup_response: SchedulerJobLookup::Pending,
        });
        let tool = SchedulerTool::new(backend);

        let response = tool
            .execute(serde_json::json!({"action": "list_subagents"}), make_ctx())
            .await
            .expect("list_subagents should succeed");

        assert_eq!(response[0]["agent_id"], serde_json::json!(3));
        assert_eq!(response[0]["display_name"], serde_json::json!("Planner"));
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
                make_ctx(),
            )
            .await
            .expect("get_job_result should succeed");

        assert_eq!(response["job_id"], serde_json::json!("job-42"));
        assert_eq!(response["status"], serde_json::json!("completed"));
        assert_eq!(response["result"]["message"], serde_json::json!("finished"));
    }

    #[tokio::test]
    async fn legacy_dispatch_job_tool_accepts_original_argument_shape() {
        let backend = Arc::new(MockSchedulerBackend {
            dispatch_job_id: "job-77".to_string(),
            dispatch_calls: Mutex::new(Vec::new()),
            list_response: Vec::new(),
            lookup_response: SchedulerJobLookup::Pending,
        });
        let tool = DispatchJobTool::with_name("dispath_job", backend.clone());
        let ctx = make_ctx();
        let thread_id = ctx.thread_id;

        let response = tool
            .execute(
                serde_json::json!({
                    "prompt": "review docs",
                    "agent_id": 11
                }),
                ctx,
            )
            .await
            .expect("legacy dispatch tool should succeed");

        assert_eq!(tool.name(), "dispath_job");
        assert_eq!(response["job_id"], serde_json::json!("job-77"));

        let calls = backend
            .dispatch_calls
            .lock()
            .expect("dispatch_calls mutex poisoned");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].thread_id, thread_id);
        assert_eq!(calls[0].agent_id, AgentId::new(11));
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
