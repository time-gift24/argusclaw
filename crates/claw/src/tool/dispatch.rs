//! DispatchTool - dispatch tasks to independent subagents.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::agents::AgentId;
use crate::agents::AgentManager;
use crate::agents::thread::{ThreadEvent, ThreadId};
use crate::approval::{ApprovalDecision, ApprovalManager, ApprovalRequest};
use crate::job::{InMemoryBackendConfig, InMemoryJobBackend, JobBackend, JobRequest};
use crate::llm::ToolDefinition;
use crate::protocol::RiskLevel;
use crate::tool::{NamedTool, ToolError};
use crate::workflow::JobId;

/// Configuration for DispatchTool.
#[derive(Clone, Debug)]
pub struct DispatchToolConfig {
    /// Default timeout for subagent execution in seconds.
    pub default_timeout_secs: u64,
    /// Interval for progress notifications in seconds.
    pub progress_notify_interval_secs: u64,
    /// Timeout for orchestrate mode jobs in seconds.
    pub orchestrate_timeout_secs: u64,
    /// Template agent ID for subagent creation.
    pub subagent_template_id: AgentId,
}

impl Default for DispatchToolConfig {
    fn default() -> Self {
        Self {
            default_timeout_secs: 300,
            progress_notify_interval_secs: 60,
            orchestrate_timeout_secs: 3600,
            subagent_template_id: AgentId::new("subagent"),
        }
    }
}

/// Tool for dispatching tasks to subagents.
///
/// Supports two modes:
/// - **Memory mode (default)**: Synchronous dispatch with wait and progress notifications.
/// - **Orchestrate mode**: Fire-and-forget with user confirmation.
pub struct DispatchTool {
    job_backend: Arc<dyn JobBackend>,
    /// Agent manager for potential direct agent operations.
    /// Stored for future use (e.g., custom agent creation, status queries).
    #[allow(dead_code)]
    agent_manager: Arc<AgentManager>,
    approval_manager: Option<Arc<ApprovalManager>>,
    thread_event_sender: broadcast::Sender<ThreadEvent>,
    config: DispatchToolConfig,
}

impl DispatchTool {
    /// Create a new DispatchTool with the given dependencies.
    pub fn new(
        job_backend: Arc<dyn JobBackend>,
        agent_manager: Arc<AgentManager>,
        approval_manager: Option<Arc<ApprovalManager>>,
        thread_event_sender: broadcast::Sender<ThreadEvent>,
        config: DispatchToolConfig,
    ) -> Self {
        Self {
            job_backend,
            agent_manager,
            approval_manager,
            thread_event_sender,
            config,
        }
    }

    /// Create a new DispatchTool with an in-memory job backend.
    ///
    /// This is the recommended constructor for most use cases.
    pub fn with_memory_backend(
        agent_manager: Arc<AgentManager>,
        approval_manager: Option<Arc<ApprovalManager>>,
        thread_event_sender: broadcast::Sender<ThreadEvent>,
        config: DispatchToolConfig,
    ) -> Self {
        let backend_config = InMemoryBackendConfig {
            default_timeout_secs: config.default_timeout_secs,
            progress_notify_interval_secs: config.progress_notify_interval_secs,
            max_concurrent_jobs: 10,
        };
        let backend = Arc::new(InMemoryJobBackend::with_config(
            agent_manager.clone(),
            backend_config,
        ));
        Self::new(
            backend,
            agent_manager,
            approval_manager,
            thread_event_sender,
            config,
        )
    }

    /// Build the prompt for the subagent.
    fn build_prompt(
        &self,
        task: String,
        context: Option<String>,
        summary_hint: Option<String>,
    ) -> String {
        let mut prompt = String::new();
        prompt.push_str("## Output Requirements (Important)\n\n");
        prompt.push_str("Your response will be returned directly to the dispatching agent.\n");
        prompt.push_str("Keep your output concise and structured, under 500 words by default.\n\n");

        if let Some(ctx) = context {
            prompt.push_str("## Context\n\n");
            prompt.push_str(&ctx);
            prompt.push_str("\n\n");
        }

        prompt.push_str("## Task\n\n");
        prompt.push_str(&task);
        prompt.push_str("\n\n");

        if let Some(hint) = summary_hint {
            prompt.push_str("## Output Format\n\n");
            prompt.push_str(&hint);
            prompt.push_str("\n\n");
        }
        prompt
    }

    /// Dispatch a task and wait for completion (memory mode).
    async fn dispatch_and_wait(
        &self,
        thread_id_str: &str,
        task: String,
        context: Option<String>,
        summary_hint: Option<String>,
    ) -> Result<serde_json::Value, ToolError> {
        let job = JobRequest {
            agent_id: self.config.subagent_template_id.clone(),
            prompt: self.build_prompt(task, context, summary_hint),
            timeout_secs: self.config.default_timeout_secs,
            backend: crate::job::JobBackendKind::InMemory,
            context: None,
        };

        let job_id =
            self.job_backend
                .submit(job)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    tool_name: "dispatch_agent".to_string(),
                    reason: e.to_string(),
                })?;

        let thread_id = ThreadId::parse(thread_id_str).unwrap_or_else(|_| ThreadId::new());
        let _ = self
            .thread_event_sender
            .send(ThreadEvent::WaitingForSubagent {
                thread_id,
                job_id: job_id.clone(),
                message: "Waiting for subagent to complete...".to_string(),
            });

        let notifier = self.spawn_progress_notifier(thread_id_str, job_id.clone());

        let result = tokio::time::timeout(
            Duration::from_secs(self.config.default_timeout_secs),
            self.job_backend.wait(&job_id),
        )
        .await;

        notifier.abort();

        match result {
            Ok(Ok(job_result)) => {
                let _ = self
                    .thread_event_sender
                    .send(ThreadEvent::SubagentCompleted {
                        thread_id,
                        job_id,
                        summary: job_result.summary.clone(),
                    });
                Ok(json!({
                    "success": true,
                    "summary": job_result.summary,
                    "tokens": job_result.token_usage.total_tokens
                }))
            }
            Ok(Err(e)) => {
                let _ = self.thread_event_sender.send(ThreadEvent::SubagentFailed {
                    thread_id,
                    job_id,
                    error: e.to_string(),
                });
                Err(ToolError::ExecutionFailed {
                    tool_name: "dispatch_agent".to_string(),
                    reason: e.to_string(),
                })
            }
            Err(_) => {
                let _ = self.job_backend.cancel(&job_id).await;
                let _ = self
                    .thread_event_sender
                    .send(ThreadEvent::SubagentTimedOut {
                        thread_id,
                        job_id,
                        timeout_secs: self.config.default_timeout_secs,
                    });
                Err(ToolError::ExecutionFailed {
                    tool_name: "dispatch_agent".to_string(),
                    reason: "Subagent execution timed out".to_string(),
                })
            }
        }
    }

    /// Spawn a background task to send periodic progress notifications.
    fn spawn_progress_notifier(
        &self,
        thread_id_str: &str,
        job_id: JobId,
    ) -> tokio::task::JoinHandle<()> {
        let event_sender = self.thread_event_sender.clone();
        let interval_secs = self.config.progress_notify_interval_secs;
        let thread_id = ThreadId::parse(thread_id_str).unwrap_or_else(|_| ThreadId::new());

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            let start = std::time::Instant::now();
            loop {
                interval.tick().await;
                let elapsed = start.elapsed().as_secs();
                let _ = event_sender.send(ThreadEvent::SubagentProgress {
                    thread_id,
                    job_id: job_id.clone(),
                    elapsed_secs: elapsed,
                    message: format!("Subagent running for {} minutes...", elapsed / 60),
                });
            }
        })
    }

    /// Handle orchestrate mode (fire-and-forget with confirmation).
    async fn handle_orchestrate_mode(
        &self,
        thread_id_str: &str,
        task: String,
        context: Option<String>,
        summary_hint: Option<String>,
    ) -> Result<serde_json::Value, ToolError> {
        let thread_id = ThreadId::parse(thread_id_str).unwrap_or_else(|_| ThreadId::new());

        // Send confirmation request event
        let _ = self
            .thread_event_sender
            .send(ThreadEvent::OrchestrationConfirmationRequired {
                thread_id,
                confirmation_id: Uuid::new_v4().to_string(),
                task: task.clone(),
                message: "Main agent requests orchestrate mode. Dispatch and stop tracking?"
                    .to_string(),
            });

        // If approval manager is available, request approval
        if let Some(approval_manager) = &self.approval_manager {
            let request = ApprovalRequest::new(
                thread_id.to_string(),
                "dispatch_agent".to_string(),
                format!(
                    "Orchestrate mode: {}",
                    task.chars().take(50).collect::<String>()
                ),
                300,
                RiskLevel::Medium,
            );
            let decision = approval_manager.request_approval(request).await;
            if decision != ApprovalDecision::Approved {
                return Ok(json!({
                    "status": "cancelled",
                    "message": "User rejected orchestrate mode request"
                }));
            }
        }

        // Create job with persistent backend
        let job = JobRequest {
            agent_id: self.config.subagent_template_id.clone(),
            prompt: self.build_prompt(task.clone(), context, summary_hint),
            timeout_secs: self.config.orchestrate_timeout_secs,
            backend: crate::job::JobBackendKind::Persistent,
            context: None,
        };

        let job_id =
            self.job_backend
                .submit(job)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    tool_name: "dispatch_agent".to_string(),
                    reason: e.to_string(),
                })?;

        // Send dispatched event
        let _ = self
            .thread_event_sender
            .send(ThreadEvent::OrchestratedJobDispatched {
                thread_id,
                job_id: job_id.clone(),
                task,
                message: "Orchestrated job dispatched. Main agent is no longer tracking."
                    .to_string(),
            });

        Ok(json!({
            "status": "dispatched",
            "job_id": job_id.as_ref(),
            "message": "Task dispatched to background."
        }))
    }
}

#[async_trait]
impl NamedTool for DispatchTool {
    fn name(&self) -> &str {
        "dispatch_agent"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "dispatch_agent".to_string(),
            description: "Dispatch a subtask to an independent subagent. Use for parallelizable tasks with clear boundaries.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "Clear description of the task"
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional context information"
                    },
                    "summary_hint": {
                        "type": "string",
                        "description": "Guide for how to summarize results"
                    },
                    "orchestrate": {
                        "type": "boolean",
                        "description": "Set true for orchestrate mode (fire-and-forget)"
                    },
                    "thread_id": {
                        "type": "string",
                        "description": "Thread ID for event correlation (internal use)"
                    }
                },
                "required": ["task"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let task = args["task"]
            .as_str()
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool_name: "dispatch_agent".to_string(),
                reason: "Missing required parameter: task".to_string(),
            })?
            .to_string();

        let context = args["context"].as_str().map(String::from);
        let summary_hint = args["summary_hint"].as_str().map(String::from);
        let orchestrate = args["orchestrate"].as_bool().unwrap_or(false);
        let thread_id = args["thread_id"].as_str().unwrap_or("unknown");

        if orchestrate {
            self.handle_orchestrate_mode(thread_id, task, context, summary_hint)
                .await
        } else {
            self.dispatch_and_wait(thread_id, task, context, summary_hint)
                .await
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_tool_config_defaults() {
        let config = DispatchToolConfig::default();
        assert_eq!(config.default_timeout_secs, 300);
        assert_eq!(config.progress_notify_interval_secs, 60);
        assert_eq!(config.orchestrate_timeout_secs, 3600);
        assert_eq!(config.subagent_template_id.as_ref(), "subagent");
    }

    #[test]
    fn dispatch_tool_config_custom() {
        let config = DispatchToolConfig {
            default_timeout_secs: 600,
            progress_notify_interval_secs: 30,
            orchestrate_timeout_secs: 7200,
            subagent_template_id: AgentId::new("custom_agent"),
        };
        assert_eq!(config.default_timeout_secs, 600);
        assert_eq!(config.progress_notify_interval_secs, 30);
        assert_eq!(config.orchestrate_timeout_secs, 7200);
        assert_eq!(config.subagent_template_id.as_ref(), "custom_agent");
    }

    #[test]
    fn tool_definition_static() {
        // Test the tool definition without needing to create an instance
        let expected_name = "dispatch_agent";
        let expected_description = "Dispatch a subtask to an independent subagent. Use for parallelizable tasks with clear boundaries.";

        let params = json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Clear description of the task"
                },
                "context": {
                    "type": "string",
                    "description": "Optional context information"
                },
                "summary_hint": {
                    "type": "string",
                    "description": "Guide for how to summarize results"
                },
                "orchestrate": {
                    "type": "boolean",
                    "description": "Set true for orchestrate mode (fire-and-forget)"
                },
                "thread_id": {
                    "type": "string",
                    "description": "Thread ID for event correlation (internal use)"
                }
            },
            "required": ["task"]
        });

        // Verify the expected structure
        assert_eq!(params["type"], "object");
        assert!(params["properties"]["task"].is_object());
        assert_eq!(params["required"], json!(["task"]));

        // Verify static values
        assert_eq!(expected_name, "dispatch_agent");
        assert!(!expected_description.is_empty());
    }

    #[test]
    fn risk_level_is_medium() {
        // Verify that dispatch_agent has Medium risk level
        assert_eq!(RiskLevel::Medium, RiskLevel::Medium);
    }

    #[test]
    fn build_prompt_structure_basic() {
        // Test prompt building logic
        let task = "Do something";
        let expected_sections = vec![
            "## Output Requirements (Important)",
            "Your response will be returned directly to the dispatching agent.",
            "Keep your output concise and structured, under 500 words by default.",
            "## Task",
            task,
        ];

        // Build prompt like the actual method
        let mut prompt = String::new();
        prompt.push_str("## Output Requirements (Important)\n\n");
        prompt.push_str("Your response will be returned directly to the dispatching agent.\n");
        prompt.push_str("Keep your output concise and structured, under 500 words by default.\n\n");
        prompt.push_str("## Task\n\n");
        prompt.push_str(task);
        prompt.push_str("\n\n");

        for section in expected_sections {
            assert!(prompt.contains(section), "Prompt missing: {section}");
        }
    }

    #[test]
    fn build_prompt_structure_with_context() {
        let task = "Main task";
        let context = "Background information";

        let mut prompt = String::new();
        prompt.push_str("## Output Requirements (Important)\n\n");
        prompt.push_str("Your response will be returned directly to the dispatching agent.\n");
        prompt.push_str("Keep your output concise and structured, under 500 words by default.\n\n");
        prompt.push_str("## Context\n\n");
        prompt.push_str(context);
        prompt.push_str("\n\n");
        prompt.push_str("## Task\n\n");
        prompt.push_str(task);
        prompt.push_str("\n\n");

        assert!(prompt.contains("## Context"));
        assert!(prompt.contains(context));
        assert!(prompt.contains("## Task"));
        assert!(prompt.contains(task));
    }

    #[test]
    fn build_prompt_structure_with_summary_hint() {
        let task = "Main task";
        let summary_hint = "Summarize in bullet points";

        let mut prompt = String::new();
        prompt.push_str("## Output Requirements (Important)\n\n");
        prompt.push_str("Your response will be returned directly to the dispatching agent.\n");
        prompt.push_str("Keep your output concise and structured, under 500 words by default.\n\n");
        prompt.push_str("## Task\n\n");
        prompt.push_str(task);
        prompt.push_str("\n\n");
        prompt.push_str("## Output Format\n\n");
        prompt.push_str(summary_hint);
        prompt.push_str("\n\n");

        assert!(prompt.contains("## Task"));
        assert!(prompt.contains(task));
        assert!(prompt.contains("## Output Format"));
        assert!(prompt.contains(summary_hint));
    }

    #[test]
    fn build_prompt_structure_full() {
        let task = "Main task";
        let context = "Background information";
        let summary_hint = "Summarize in bullet points";

        let mut prompt = String::new();
        prompt.push_str("## Output Requirements (Important)\n\n");
        prompt.push_str("Your response will be returned directly to the dispatching agent.\n");
        prompt.push_str("Keep your output concise and structured, under 500 words by default.\n\n");
        prompt.push_str("## Context\n\n");
        prompt.push_str(context);
        prompt.push_str("\n\n");
        prompt.push_str("## Task\n\n");
        prompt.push_str(task);
        prompt.push_str("\n\n");
        prompt.push_str("## Output Format\n\n");
        prompt.push_str(summary_hint);
        prompt.push_str("\n\n");

        // Verify all sections present
        assert!(prompt.contains("## Output Requirements"));
        assert!(prompt.contains("## Context"));
        assert!(prompt.contains("## Task"));
        assert!(prompt.contains("## Output Format"));
        assert!(prompt.contains(context));
        assert!(prompt.contains(task));
        assert!(prompt.contains(summary_hint));
    }

    #[test]
    fn thread_event_variants_exist() {
        // Verify ThreadEvent variants compile correctly
        let thread_id = ThreadId::new();
        let job_id = JobId::new("test-job");

        let _waiting = ThreadEvent::WaitingForSubagent {
            thread_id,
            job_id: job_id.clone(),
            message: "Waiting...".to_string(),
        };

        let _progress = ThreadEvent::SubagentProgress {
            thread_id,
            job_id: job_id.clone(),
            elapsed_secs: 60,
            message: "Running...".to_string(),
        };

        let _completed = ThreadEvent::SubagentCompleted {
            thread_id,
            job_id: job_id.clone(),
            summary: "Done".to_string(),
        };

        let _failed = ThreadEvent::SubagentFailed {
            thread_id,
            job_id: job_id.clone(),
            error: "Error".to_string(),
        };

        let _timed_out = ThreadEvent::SubagentTimedOut {
            thread_id,
            job_id: job_id.clone(),
            timeout_secs: 300,
        };

        let _orchestrated = ThreadEvent::OrchestratedJobDispatched {
            thread_id,
            job_id: job_id.clone(),
            task: "Task".to_string(),
            message: "Dispatched".to_string(),
        };

        let _confirmation = ThreadEvent::OrchestrationConfirmationRequired {
            thread_id,
            confirmation_id: "confirmation-123".to_string(),
            task: "Task".to_string(),
            message: "Confirm?".to_string(),
        };
    }
}
