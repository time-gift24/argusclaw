//! Unified scheduler tool implementation.
//!
//! This tool consolidates subagent scheduling operations behind one entrypoint:
//! - `dispatch_job`
//! - `list_subagents`
//! - `get_job_result`
//! - `send_message`
//! - `check_inbox`
//! - `mark_read`

use std::sync::Arc;

use argus_protocol::{
    AgentId, MailboxMessage, NamedTool, RiskLevel, ThreadControlEvent, ThreadEvent, ThreadId,
    ToolDefinition, ToolError, ToolExecutionContext,
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
    #[serde(rename = "send_message")]
    SendMessage {
        to: String,
        message: String,
        #[serde(default)]
        summary: Option<String>,
    },
    #[serde(rename = "check_inbox")]
    CheckInbox,
    #[serde(rename = "mark_read")]
    MarkRead { message_id: String },
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

/// Request payload for sending a mailbox message.
#[derive(Debug, Clone)]
pub struct SendMessageRequest {
    pub thread_id: ThreadId,
    pub to: String,
    pub message: String,
    pub summary: Option<String>,
}

/// Response payload for sending a mailbox message.
#[derive(Debug, Clone, Serialize)]
pub struct SendMessageResponse {
    pub delivered: usize,
    pub thread_ids: Vec<ThreadId>,
}

/// Request payload for checking a thread inbox.
#[derive(Debug, Clone)]
pub struct CheckInboxRequest {
    pub thread_id: ThreadId,
}

/// Request payload for marking an inbox message as read.
#[derive(Debug, Clone)]
pub struct MarkReadRequest {
    pub thread_id: ThreadId,
    pub message_id: String,
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

    async fn send_message(
        &self,
        request: SendMessageRequest,
    ) -> Result<SendMessageResponse, ToolError>;

    async fn check_inbox(
        &self,
        request: CheckInboxRequest,
    ) -> Result<Vec<MailboxMessage>, ToolError>;

    async fn mark_read(&self, request: MarkReadRequest) -> Result<(), ToolError>;
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
                "type": "integer",
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

fn scheduler_send_message_variant() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "action": {
                "const": "send_message",
                "description": "Scheduler operation to perform"
            },
            "to": {
                "type": "string",
                "description": "Mailbox target: job:<job_id>, thread:<thread_id>, parent, *, or a unique direct-child agent name"
            },
            "message": {
                "type": "string",
                "description": "Mailbox message content"
            },
            "summary": {
                "type": "string",
                "description": "Optional short summary metadata for the message"
            }
        },
        "required": ["action", "to", "message"],
        "additionalProperties": false
    })
}

fn scheduler_check_inbox_variant() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "action": {
                "const": "check_inbox",
                "description": "Scheduler operation to perform"
            }
        },
        "required": ["action"],
        "additionalProperties": false
    })
}

fn scheduler_mark_read_variant() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "action": {
                "const": "mark_read",
                "description": "Scheduler operation to perform"
            },
            "message_id": {
                "type": "string",
                "description": "Stable mailbox message ID to mark as read"
            }
        },
        "required": ["action", "message_id"],
        "additionalProperties": false
    })
}

fn scheduler_definition() -> ToolDefinition {
    ToolDefinition {
        name: "scheduler".to_string(),
        description: "Unified scheduler skill for subagent orchestration. Supports list_subagents, dispatch_job, get_job_result, send_message, check_inbox, and mark_read operations.".to_string(),
        parameters: serde_json::json!({
            "oneOf": [
                scheduler_dispatch_variant(),
                scheduler_list_variant(),
                scheduler_get_result_variant(),
                scheduler_send_message_variant(),
                scheduler_check_inbox_variant(),
                scheduler_mark_read_variant()
            ]
        }),
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
                let subagents = self.backend.list_subagents().await?;
                serialize_value(subagents, self.name(), "subagents")
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
                        "result": serialize_value(result, self.name(), "job result")?,
                    })),
                    SchedulerJobLookup::Consumed(result) => Ok(serde_json::json!({
                        "job_id": job_id,
                        "status": "consumed",
                        "result": serialize_value(result, self.name(), "job result")?,
                    })),
                }
            }
            SchedulerInput::SendMessage {
                to,
                message,
                summary,
            } => {
                let response = self
                    .backend
                    .send_message(SendMessageRequest {
                        thread_id: ctx.thread_id,
                        to,
                        message,
                        summary,
                    })
                    .await?;
                serialize_value(response, self.name(), "send message response")
            }
            SchedulerInput::CheckInbox => {
                let messages = self
                    .backend
                    .check_inbox(CheckInboxRequest {
                        thread_id: ctx.thread_id,
                    })
                    .await?;
                serialize_value(messages, self.name(), "mailbox messages")
            }
            SchedulerInput::MarkRead { message_id } => {
                self.backend
                    .mark_read(MarkReadRequest {
                        thread_id: ctx.thread_id,
                        message_id: message_id.clone(),
                    })
                    .await?;
                Ok(serde_json::json!({
                    "message_id": message_id,
                    "status": "marked_read",
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use argus_protocol::{MailboxMessageType, ThreadJobResult};
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
        send_calls: Mutex<Vec<SendMessageRequest>>,
        send_response: SendMessageResponse,
        inbox_response: Vec<MailboxMessage>,
        mark_read_calls: Mutex<Vec<MarkReadRequest>>,
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

        async fn send_message(
            &self,
            request: SendMessageRequest,
        ) -> Result<SendMessageResponse, ToolError> {
            self.send_calls
                .lock()
                .expect("send_calls mutex poisoned")
                .push(request);
            Ok(self.send_response.clone())
        }

        async fn check_inbox(
            &self,
            _request: CheckInboxRequest,
        ) -> Result<Vec<MailboxMessage>, ToolError> {
            Ok(self.inbox_response.clone())
        }

        async fn mark_read(&self, request: MarkReadRequest) -> Result<(), ToolError> {
            self.mark_read_calls
                .lock()
                .expect("mark_read_calls mutex poisoned")
                .push(request);
            Ok(())
        }
    }

    fn make_ctx() -> Arc<ToolExecutionContext> {
        let (pipe_tx, _) = broadcast::channel(8);
        let (control_tx, _control_rx) = tokio::sync::mpsc::unbounded_channel();
        Arc::new(ToolExecutionContext {
            thread_id: ThreadId::new(),
            agent_id: None,
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

    fn sample_mailbox_message() -> MailboxMessage {
        MailboxMessage {
            id: "msg-1".to_string(),
            from_thread_id: ThreadId::new(),
            to_thread_id: ThreadId::new(),
            from_label: "Planner".to_string(),
            message_type: MailboxMessageType::Plain,
            text: "hello from planner".to_string(),
            timestamp: "2026-04-01T00:00:00Z".to_string(),
            read: false,
            summary: Some("hello".to_string()),
        }
    }

    #[test]
    fn scheduler_name_and_risk_level() {
        let backend = Arc::new(MockSchedulerBackend {
            dispatch_job_id: "job-1".to_string(),
            dispatch_calls: Mutex::new(Vec::new()),
            list_response: Vec::new(),
            lookup_response: SchedulerJobLookup::Pending,
            send_calls: Mutex::new(Vec::new()),
            send_response: SendMessageResponse {
                delivered: 0,
                thread_ids: Vec::new(),
            },
            inbox_response: Vec::new(),
            mark_read_calls: Mutex::new(Vec::new()),
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
            send_calls: Mutex::new(Vec::new()),
            send_response: SendMessageResponse {
                delivered: 0,
                thread_ids: Vec::new(),
            },
            inbox_response: Vec::new(),
            mark_read_calls: Mutex::new(Vec::new()),
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

        let send_message_variant = variants
            .iter()
            .find(|variant| {
                variant["properties"]["action"]["const"] == serde_json::json!("send_message")
            })
            .expect("send_message variant should exist");
        assert_eq!(
            send_message_variant["required"],
            serde_json::json!(["action", "to", "message"])
        );
    }

    #[tokio::test]
    async fn dispatch_job_action_calls_backend_and_returns_job_id() {
        let backend = Arc::new(MockSchedulerBackend {
            dispatch_job_id: "job-42".to_string(),
            dispatch_calls: Mutex::new(Vec::new()),
            list_response: Vec::new(),
            lookup_response: SchedulerJobLookup::Pending,
            send_calls: Mutex::new(Vec::new()),
            send_response: SendMessageResponse {
                delivered: 0,
                thread_ids: Vec::new(),
            },
            inbox_response: Vec::new(),
            mark_read_calls: Mutex::new(Vec::new()),
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
    async fn dispatch_job_accepts_numeric_string_agent_id() {
        let backend = Arc::new(MockSchedulerBackend {
            dispatch_job_id: "job-43".to_string(),
            dispatch_calls: Mutex::new(Vec::new()),
            list_response: Vec::new(),
            lookup_response: SchedulerJobLookup::Pending,
            send_calls: Mutex::new(Vec::new()),
            send_response: SendMessageResponse {
                delivered: 0,
                thread_ids: Vec::new(),
            },
            inbox_response: Vec::new(),
            mark_read_calls: Mutex::new(Vec::new()),
        });
        let tool = SchedulerTool::new(backend.clone());

        let response = tool
            .execute(
                serde_json::json!({
                    "action": "dispatch_job",
                    "prompt": "summarize logs",
                    "agent_id": "7",
                }),
                make_ctx(),
            )
            .await
            .expect("dispatch_job should accept numeric string agent ids");

        assert_eq!(response["job_id"], serde_json::json!("job-43"));
        let calls = backend
            .dispatch_calls
            .lock()
            .expect("dispatch_calls mutex poisoned");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].agent_id, AgentId::new(7));
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
            send_calls: Mutex::new(Vec::new()),
            send_response: SendMessageResponse {
                delivered: 0,
                thread_ids: Vec::new(),
            },
            inbox_response: Vec::new(),
            mark_read_calls: Mutex::new(Vec::new()),
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
            send_calls: Mutex::new(Vec::new()),
            send_response: SendMessageResponse {
                delivered: 0,
                thread_ids: Vec::new(),
            },
            inbox_response: Vec::new(),
            mark_read_calls: Mutex::new(Vec::new()),
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
    async fn send_message_action_calls_backend_and_returns_delivery_metadata() {
        let delivered_thread_id = ThreadId::new();
        let backend = Arc::new(MockSchedulerBackend {
            dispatch_job_id: "job-42".to_string(),
            dispatch_calls: Mutex::new(Vec::new()),
            list_response: Vec::new(),
            lookup_response: SchedulerJobLookup::Pending,
            send_calls: Mutex::new(Vec::new()),
            send_response: SendMessageResponse {
                delivered: 1,
                thread_ids: vec![delivered_thread_id],
            },
            inbox_response: Vec::new(),
            mark_read_calls: Mutex::new(Vec::new()),
        });
        let tool = SchedulerTool::new(backend.clone());
        let ctx = make_ctx();

        let response = tool
            .execute(
                serde_json::json!({
                    "action": "send_message",
                    "to": "parent",
                    "message": "hello",
                    "summary": "greeting"
                }),
                ctx.clone(),
            )
            .await
            .expect("send_message should succeed");

        assert_eq!(response["delivered"], serde_json::json!(1));
        assert_eq!(
            response["thread_ids"][0],
            serde_json::json!(delivered_thread_id)
        );
        let calls = backend
            .send_calls
            .lock()
            .expect("send_calls mutex poisoned");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].thread_id, ctx.thread_id);
        assert_eq!(calls[0].to, "parent");
        assert_eq!(calls[0].message, "hello");
        assert_eq!(calls[0].summary, Some("greeting".to_string()));
    }

    #[tokio::test]
    async fn check_inbox_action_returns_serialized_mailbox_messages() {
        let backend = Arc::new(MockSchedulerBackend {
            dispatch_job_id: "job-42".to_string(),
            dispatch_calls: Mutex::new(Vec::new()),
            list_response: Vec::new(),
            lookup_response: SchedulerJobLookup::Pending,
            send_calls: Mutex::new(Vec::new()),
            send_response: SendMessageResponse {
                delivered: 0,
                thread_ids: Vec::new(),
            },
            inbox_response: vec![sample_mailbox_message()],
            mark_read_calls: Mutex::new(Vec::new()),
        });
        let tool = SchedulerTool::new(backend);

        let response = tool
            .execute(serde_json::json!({"action": "check_inbox"}), make_ctx())
            .await
            .expect("check_inbox should succeed");

        assert_eq!(response[0]["id"], serde_json::json!("msg-1"));
        assert_eq!(response[0]["text"], serde_json::json!("hello from planner"));
    }

    #[tokio::test]
    async fn mark_read_action_passes_message_id_to_backend() {
        let backend = Arc::new(MockSchedulerBackend {
            dispatch_job_id: "job-42".to_string(),
            dispatch_calls: Mutex::new(Vec::new()),
            list_response: Vec::new(),
            lookup_response: SchedulerJobLookup::Pending,
            send_calls: Mutex::new(Vec::new()),
            send_response: SendMessageResponse {
                delivered: 0,
                thread_ids: Vec::new(),
            },
            inbox_response: Vec::new(),
            mark_read_calls: Mutex::new(Vec::new()),
        });
        let tool = SchedulerTool::new(backend.clone());
        let ctx = make_ctx();

        let response = tool
            .execute(
                serde_json::json!({
                    "action": "mark_read",
                    "message_id": "msg-1",
                }),
                ctx.clone(),
            )
            .await
            .expect("mark_read should succeed");

        assert_eq!(response["status"], serde_json::json!("marked_read"));
        let calls = backend
            .mark_read_calls
            .lock()
            .expect("mark_read_calls mutex poisoned");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].thread_id, ctx.thread_id);
        assert_eq!(calls[0].message_id, "msg-1");
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
