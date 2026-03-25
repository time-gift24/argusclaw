# Dispatch Job Unified Pipe Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `get_job_result` polling and fragmented communication with a single `broadcast::Channel<ThreadEvent>` per Thread. All inputs (user messages, job dispatches, job results, user interrupts) flow through the same pipe.

**Architecture:** Thread owns the pipe. When idle, Thread waits on the pipe and spawns Turns. When a Turn is running, it polls the pipe at each iteration start for job results and interrupts. The `dispatch_job` tool becomes fire-and-forget, sending `JobDispatched` into the pipe and returning immediately. The `get_job_result` tool is deleted entirely.

**Tech Stack:** Rust, tokio async, `broadcast` channel, `Arc`, derive_builder

---

## Chunk 1: Core Protocol Changes (argus-protocol)

### Task 1: Add `ToolExecutionContext` and update `NamedTool` trait

**Files:**
- Modify: `crates/argus-protocol/src/tool.rs`
- Modify: `crates/argus-protocol/src/lib.rs`

> **Important:** `ToolDefinition` is defined in `llm/mod.rs:428` and re-exported through `crate::llm::ToolDefinition`. Do NOT redefine it. `ToolError` is an enum with variants `NotFound`, `ExecutionFailed`, `SecurityBlocked` — keep it as an enum, don't replace with a struct. The current `NamedTool::execute` uses parameter name `args` (not `input`).

- [ ] **Step 1: Read current tool.rs**

```bash
cat crates/argus-protocol/src/tool.rs
```

- [ ] **Step 2: Add `ToolExecutionContext` and update `NamedTool::execute` signature**

Replace the entire `tool.rs` content with:

```rust
//! Tool types for agent/LLM tool management.
//!
//! This module contains shared types for tools used by argus-tool crate.

use async_trait::async_trait;

use std::sync::Arc;
use tokio::sync::broadcast;

use crate::llm::ToolDefinition;
use crate::ids::ThreadId;
use crate::RiskLevel;
use crate::ThreadEvent;

/// Context passed to tools at execution time.
#[derive(Debug, Clone)]
pub struct ToolExecutionContext {
    /// The thread ID in which the tool is executing.
    pub thread_id: ThreadId,
    /// The pipe sender for this thread. Tools can send ThreadEvent variants
    /// into this pipe. Failures are logged as warnings and do not block execution.
    pub pipe_tx: broadcast::Sender<ThreadEvent>,
}

/// Error type for tool operations.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// Tool not found in registry.
    #[error("Tool not found: {id}")]
    NotFound { id: String },

    /// Tool execution failed.
    #[error("Tool '{tool_name}' execution failed: {reason}")]
    ExecutionFailed { tool_name: String, reason: String },

    /// Request blocked by security policy (e.g., SSRF protection).
    #[error("HTTP request to '{url}' blocked: {reason}")]
    SecurityBlocked { url: String, reason: String },
}

/// Trait for defining tools that can be used by agents and LLMs.
#[async_trait]
pub trait NamedTool: Send + Sync {
    /// Returns the unique name of the tool.
    fn name(&self) -> &str;

    /// Returns the tool definition for LLM consumption.
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool with the provided arguments.
    ///
    /// `input` is the JSON arguments from the LLM.
    /// `ctx` provides execution context including the pipe for sending events.
    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError>;

    /// Returns the risk level of this tool for approval gating.
    /// Default is `RiskLevel::Low` for read-only/safe operations.
    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }
}
```

- [ ] **Step 3: Update lib.rs export**

Read `crates/argus-protocol/src/lib.rs`, find the tool exports (around line 72):
```rust
pub use tool::{NamedTool, ToolError};
```

Replace with:
```rust
pub use tool::{NamedTool, ToolError, ToolExecutionContext};
```

- [ ] **Step 4: Verify it compiles**

```bash
cargo check -p argus-protocol 2>&1 | tail -20
```
Expected: Compiles successfully

- [ ] **Step 5: Commit**

```bash
git add crates/argus-protocol/src/tool.rs crates/argus-protocol/src/lib.rs
git commit -m "feat(protocol): add ToolExecutionContext, update NamedTool::execute signature

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2: Update `ThreadEvent` enum

**Files:**
- Modify: `crates/argus-protocol/src/events.rs`

- [ ] **Step 1: Read current events.rs**

Read the full file to understand existing variants and imports.

- [ ] **Step 2: Remove `JobCompleted` variant**

Find and delete the `JobCompleted` variant (around line 96). Also remove its import dependencies (`JobStatusEvent` from `types.rs` if it was only used for this).

- [ ] **Step 3: Add new variants**

Add these variants inside the `ThreadEvent` enum (before the closing brace):

```rust
/// A job was dispatched by the dispatch_job tool.
JobDispatched {
    /// Job ID.
    job_id: String,
    /// Agent ID for this job.
    agent_id: AgentId,
    /// Prompt/task description for the job.
    prompt: String,
    /// Optional context JSON for the job.
    context: Option<serde_json::Value>,
},

/// A dispatched job produced a result.
JobResult {
    /// Job ID.
    job_id: String,
    /// Whether the job succeeded.
    success: bool,
    /// Output or error message.
    message: String,
    /// Token usage if available.
    token_usage: Option<TokenUsage>,
},

/// User wants to interrupt or redirect the current turn.
UserInterrupt {
    /// Interrupt content (e.g. "stop", "cancel", or a new instruction).
    content: String,
},

/// A new user message to process.
UserMessage {
    /// Message content.
    content: String,
    /// Optional per-message overrides (temperature, max_tokens, etc.).
    msg_override: Option<MessageOverride>,
},
```

- [ ] **Step 4: Add missing imports**

Ensure these are imported at the top of events.rs:
```rust
use crate::ids::AgentId;
use crate::token_usage::TokenUsage;
use crate::message_override::MessageOverride;
```

- [ ] **Step 5: Verify it compiles**

```bash
cargo check -p argus-protocol 2>&1 | tail -10
```

- [ ] **Step 6: Commit**

```bash
git add crates/argus-protocol/src/events.rs
git commit -m "feat(protocol): add JobDispatched, JobResult, UserInterrupt, UserMessage; remove JobCompleted

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 2: Update All Tool Implementations

**9 files to update** — each tool's `execute` signature changes from `(input)` to `(input, ctx: Arc<ToolExecutionContext>)`. Tools that don't use the context simply ignore the `ctx` parameter.

**Files:**
- Modify: `crates/argus-job/src/dispatch_tool.rs`
- Modify: `crates/argus-job/src/list_subagents_tool.rs`
- Modify: `crates/argus-thread/src/plan_tool.rs`
- Modify: `crates/argus-tool/src/glob.rs`
- Modify: `crates/argus-tool/src/grep.rs`
- Modify: `crates/argus-tool/src/http.rs`
- Modify: `crates/argus-tool/src/read.rs`
- Modify: `crates/argus-tool/src/shell.rs`
- Modify: `crates/argus-turn/src/execution.rs`
- Modify: `crates/argus-turn/src/bin/turn.rs` *(EchoTool implementation at line 149)*

### Task 3: Update each tool's execute signature

For **each** of the 10 files above, the change is identical in pattern:

- [ ] **Step 1: Update execute signature in each file**

Find the line:
```rust
async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError>
```

Replace with:
```rust
async fn execute(&self, input: serde_json::Value, _ctx: Arc<ToolExecutionContext>) -> Result<serde_json::Value, ToolError>
```

> **Note:** The parameter name changes from `args` to `input`. Update all internal usages of `args` → `input` within the execute body in each file.

Also add the import at the top of each file:
```rust
use std::sync::Arc;
use argus_protocol::ToolExecutionContext;
```

- [ ] **Step 2: Verify each crate compiles**

```bash
cargo check -p argus-job 2>&1 | grep -E "^error" | head -5
cargo check -p argus-thread 2>&1 | grep -E "^error" | head -5
cargo check -p argus-tool 2>&1 | grep -E "^error" | head -5
cargo check -p argus-turn 2>&1 | grep -E "^error" | head -5
```

Also verify argus-protocol still compiles (Step 3):
```bash
cargo check -p argus-protocol 2>&1 | tail -5
```

- [ ] **Step 3: Commit after all 10 tools updated**

```bash
git add crates/argus-job/src/dispatch_tool.rs \
    crates/argus-job/src/list_subagents_tool.rs \
    crates/argus-thread/src/plan_tool.rs \
    crates/argus-tool/src/glob.rs \
    crates/argus-tool/src/grep.rs \
    crates/argus-tool/src/http.rs \
    crates/argus-tool/src/read.rs \
    crates/argus-tool/src/shell.rs \
    crates/argus-turn/src/execution.rs \
    crates/argus-turn/src/bin/turn.rs
git commit -m "refactor(tool): update execute signature to accept ToolExecutionContext

All 10 tool implementations updated. Signature: execute(input, ctx) -> Result.
Parameter renamed args -> input in all implementations.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 3: argus-job Changes

### Task 4: Delete `get_job_result_tool.rs` and update `lib.rs`

**Files:**
- Delete: `crates/argus-job/src/get_job_result_tool.rs`
- Modify: `crates/argus-job/src/lib.rs`

- [ ] **Step 1: Delete get_job_result_tool.rs**

```bash
rm crates/argus-job/src/get_job_result_tool.rs
```

- [ ] **Step 2: Update lib.rs**

Read `crates/argus-job/src/lib.rs`. Remove:
```rust
pub mod get_job_result_tool;
pub use get_job_result_tool::GetJobResultTool;
```

- [ ] **Step 3: Verify compiles**

```bash
cargo check -p argus-job 2>&1 | tail -5
```

- [ ] **Step 4: Commit**

```bash
git add crates/argus-job/src/get_job_result_tool.rs crates/argus-job/src/lib.rs
git rm crates/argus-job/src/get_job_result_tool.rs
git commit -m "refactor(job): remove get_job_result tool

Deleted GetJobResultTool. Job results are now delivered via ThreadEvent::JobResult
through the unified pipe instead of polling.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 5: Update `types.rs` — remove `wait_for_result` and `JobDispatchResult`

**Files:**
- Modify: `crates/argus-job/src/types.rs`

- [ ] **Step 1: Read current types.rs**

- [ ] **Step 2: Remove `wait_for_result` from `JobDispatchArgs`**

Delete line:
```rust
pub wait_for_result: bool,
```

- [ ] **Step 3: Remove `JobDispatchResult` struct entirely**

Delete the entire `JobDispatchResult` struct (it returns `{ job_id, status: "dispatched" }` inline in dispatch_tool instead).

- [ ] **Step 4: Verify compiles**

```bash
cargo check -p argus-job 2>&1 | tail -5
```

- [ ] **Step 5: Commit**

```bash
git add crates/argus-job/src/types.rs
git commit -m "refactor(job): remove wait_for_result and JobDispatchResult

Jobs are now fire-and-forget. dispatch_job returns immediately with {job_id, status: "dispatched"}.
Results flow through the unified pipe as ThreadEvent::JobResult.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 6: Rewrite `dispatch_tool.rs` — fire-and-forget with pipe

**Files:**
- Modify: `crates/argus-job/src/dispatch_tool.rs`

- [ ] **Step 1: Read the current file completely**

- [ ] **Step 2: Replace the entire file with fire-and-forget implementation**

Replace with:

```rust
//! dispatch_job tool implementation.
//!
//! Fire-and-forget: generates job_id, sends ThreadEvent::JobDispatched into
//! the pipe, spawns background executor, returns immediately.

use std::sync::Arc;

use argus_protocol::{
    AgentId, NamedTool, RiskLevel, ThreadEvent, ToolDefinition, ToolError,
    ToolExecutionContext,
};
use async_trait::async_trait;
use uuid::Uuid;

use crate::error::JobError;
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
        RiskLevel::Medium
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let args: JobDispatchArgs = serde_json::from_value(input).map_err(|e| ToolError {
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

        // Send JobDispatched into the pipe
        let dispatch_event = ThreadEvent::JobDispatched {
            job_id: job_id.clone(),
            agent_id: args.agent_id,
            prompt: args.prompt.clone(),
            context: args.context.clone(),
        };
        if let Err(e) = ctx.pipe_tx.send(dispatch_event) {
            tracing::warn!("failed to send JobDispatched event: {}", e);
        }

        // Spawn background executor using the JobManager's spawn method
        // The executor will resolve agent_record, run Turn, and send JobResult back
        self.job_manager
            .spawn_job_executor(
                job_id.clone(),
                args.agent_id,
                args.prompt,
                args.context,
                ctx.pipe_tx.clone(),
            )
            .await
            .map_err(|e| ToolError {
                tool_name: self.name().to_string(),
                reason: e.to_string(),
            })?;

        Ok(serde_json::json!({
            "job_id": job_id,
            "status": "dispatched"
        }))
    }
}
```

- [ ] **Step 3: Verify compiles**

```bash
cargo check -p argus-job 2>&1 | tail -10
```

Expected: Should fail on `spawn_job_executor` — that method doesn't exist yet in JobManager. That's expected. We'll add it in Task 7.

- [ ] **Step 4: Commit (will fail compile, but commit the tool change)**

```bash
git add crates/argus-job/src/dispatch_tool.rs
git commit -m "refactor(job): rewrite dispatch_job as fire-and-forget

Tool now sends ThreadEvent::JobDispatched into the pipe and returns
immediately with {job_id, status: "dispatched"}. Background executor
handles job execution and sends JobResult back via pipe.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 7: Rewrite `job_manager.rs` — remove get_result, add spawn_job_executor, pipe-aware dispatch

**Files:**
- Modify: `crates/argus-job/src/job_manager.rs`

- [ ] **Step 1: Read the current file completely**

- [ ] **Step 2: Replace the entire file**

This is a larger rewrite. Replace the content with:

```rust
//! JobManager for dispatching and managing background jobs.
//!
//! Each dispatched job runs as a lightweight Turn (via TurnBuilder).
//! Results are sent back through the unified pipe as ThreadEvent::JobResult.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use argus_protocol::llm::{ChatMessage, LlmProvider, Role};
use argus_protocol::tool::NamedTool;
use argus_protocol::{AgentId, ProviderResolver, ThreadEvent};
use argus_template::TemplateManager;
use argus_tool::ToolManager;
use argus_turn::{TurnBuilder, TurnConfig, TurnOutput};
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use crate::dispatch_tool::DispatchJobTool;
use crate::error::JobError;
use crate::list_subagents_tool::ListSubagentsTool;
use crate::types::{JobDispatchArgs, JobResult};

/// Manages job dispatch and lifecycle.
pub struct JobManager {
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
    jobs: Arc<RwLock<std::collections::HashMap<String, JobState>>>,
}

impl fmt::Debug for JobManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobManager")
            .field("jobs", &self.jobs)
            .finish()
    }
}

#[derive(Debug, Clone)]
struct JobState {
    status: String,
    result: Option<JobResult>,
}

impl JobManager {
    /// Create a new JobManager.
    pub fn new(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
    ) -> Self {
        Self {
            template_manager,
            provider_resolver,
            tool_manager,
            jobs: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Create a DispatchJobTool for this manager.
    pub fn create_dispatch_tool(self: Arc<Self>) -> DispatchJobTool {
        DispatchJobTool::new(self)
    }

    /// Create a ListSubagentsTool for this manager.
    pub fn create_list_subagents_tool(self: Arc<Self>) -> ListSubagentsTool {
        ListSubagentsTool::new(Arc::clone(&self.template_manager))
    }

    /// Spawn a background job executor.
    ///
    /// Resolves the agent, builds a Turn, executes it, and sends
    /// ThreadEvent::JobResult into the pipe when done.
    pub async fn spawn_job_executor(
        &self,
        job_id: String,
        agent_id: AgentId,
        prompt: String,
        context: Option<serde_json::Value>,
        pipe_tx: broadcast::Sender<ThreadEvent>,
    ) -> Result<(), JobError> {
        if prompt.trim().is_empty() {
            return Err(JobError::ExecutionFailed(
                "prompt cannot be empty".to_string(),
            ));
        }

        // Store initial job state
        {
            let mut jobs = self.jobs.write().await;
            jobs.insert(
                job_id.clone(),
                JobState {
                    status: "running".to_string(),
                    result: None,
                },
            );
        }

        // Capture clones for the background task
        let jobs = Arc::clone(&self.jobs);
        let template_manager = Arc::clone(&self.template_manager);
        let provider_resolver = Arc::clone(&self.provider_resolver);
        let tool_manager = Arc::clone(&self.tool_manager);
        let pipe_tx_clone = pipe_tx.clone();

        tokio::spawn(async move {
            let thread_id = format!("job-{}", job_id);

            // Resolve agent_record
            let agent_record = match template_manager.get(agent_id).await {
                Ok(Some(record)) => record,
                Ok(None) => {
                    let msg = format!("agent {} not found", agent_id.inner());
                    Self::mark_failed(&jobs, &job_id, &msg).await;
                    let _ = pipe_tx_clone.send(ThreadEvent::JobResult {
                        job_id,
                        success: false,
                        message: msg,
                        token_usage: None,
                    });
                    return;
                }
                Err(e) => {
                    let msg = format!("failed to load agent: {}", e);
                    Self::mark_failed(&jobs, &job_id, &msg).await;
                    let _ = pipe_tx_clone.send(ThreadEvent::JobResult {
                        job_id,
                        success: false,
                        message: msg,
                        token_usage: None,
                    });
                    return;
                }
            };

            // Resolve provider
            let provider = match agent_record.provider_id {
                Some(pid) => match provider_resolver.resolve(pid).await {
                    Ok(p) => p,
                    Err(e) => {
                        let msg = format!("failed to resolve provider: {}", e);
                        Self::mark_failed(&jobs, &job_id, &msg).await;
                        let _ = pipe_tx_clone.send(ThreadEvent::JobResult {
                            job_id,
                            success: false,
                            message: msg,
                            token_usage: None,
                        });
                        return;
                    }
                },
                None => match provider_resolver.default_provider().await {
                    Ok(p) => p,
                    Err(e) => {
                        let msg = format!("no provider configured: {}", e);
                        Self::mark_failed(&jobs, &job_id, &msg).await;
                        let _ = pipe_tx_clone.send(ThreadEvent::JobResult {
                            job_id,
                            success: false,
                            message: msg,
                            token_usage: None,
                        });
                        return;
                    }
                },
            };

            // Collect tools filtered by agent_record.tool_names
            let enabled_tool_names: HashSet<_> = agent_record.tool_names.iter().collect();
            let tools: Vec<Arc<dyn NamedTool>> = tool_manager
                .list_ids()
                .iter()
                .filter(|name| enabled_tool_names.contains(*name))
                .filter_map(|name| tool_manager.get(name))
                .collect();

            // Create internal stream channel for the Turn
            let (stream_tx, _stream_rx) = broadcast::channel(256);

            // Build and execute the Turn
            let turn_result = TurnBuilder::default()
                .turn_number(1)
                .thread_id(thread_id.clone())
                .messages(vec![ChatMessage::user(&prompt)])
                .provider(provider)
                .tools(tools)
                .hooks(Vec::new())
                .config(TurnConfig::new())
                .agent_record(Arc::new(agent_record))
                .stream_tx(stream_tx)
                .thread_event_tx(pipe_tx_clone.clone())
                .build()
                .map_err(|e| e.to_string());

            let output = match turn_result {
                Ok(turn) => turn.execute().await,
                Err(e) => {
                    let msg = format!("failed to build turn: {}", e);
                    Self::mark_failed(&jobs, &job_id, &msg).await;
                    let _ = pipe_tx_clone.send(ThreadEvent::JobResult {
                        job_id,
                        success: false,
                        message: msg,
                        token_usage: None,
                    });
                    return;
                }
            };

            match output {
                Ok(o) => {
                    let message = Self::summarize_output(&o);
                    if let Some(state) = jobs.write().await.get_mut(&job_id) {
                        state.status = "completed".to_string();
                        state.result = Some(JobResult {
                            success: true,
                            message: message.clone(),
                            token_usage: Some(o.token_usage.clone()),
                        });
                    }
                    let _ = pipe_tx_clone.send(ThreadEvent::JobResult {
                        job_id,
                        success: true,
                        message,
                        token_usage: Some(o.token_usage),
                    });
                }
                Err(e) => {
                    let msg = e.to_string();
                    Self::mark_failed(&jobs, &job_id, &msg).await;
                    let _ = pipe_tx_clone.send(ThreadEvent::JobResult {
                        job_id,
                        success: false,
                        message: msg,
                        token_usage: None,
                    });
                }
            }
        });

        Ok(())
    }

    async fn mark_failed(jobs: &Arc<RwLock<std::collections::HashMap<String, JobState>>>, job_id: &str, message: &str) {
        let mut guard = jobs.write().await;
        if let Some(state) = guard.get_mut(job_id) {
            state.status = "failed".to_string();
            state.result = Some(JobResult {
                success: false,
                message: message.to_string(),
                token_usage: None,
            });
        }
    }

    /// Summarize turn output into a brief result message.
    fn summarize_output(output: &TurnOutput) -> String {
        for msg in output.messages.iter().rev() {
            if let ChatMessage { role: Role::Assistant, content, .. } = msg
                && !content.is_empty()
            {
                if content.len() > 500 {
                    return format!("{}...", &content[..500]);
                }
                return content.clone();
            }
        }
        format!("job completed, {} messages in turn", output.messages.len())
    }
}
```

> **Note:** `SseBroadcaster` and the old in-memory `broadcast` sender are removed from JobManager since events now flow through the unified pipe. `SseBroadcaster` can be deleted separately if unused elsewhere.

- [ ] **Step 3: Verify compiles**

```bash
cargo check -p argus-job 2>&1 | tail -10
```

Expected: Should compile. If errors, fix imports and types.

- [ ] **Step 4: Commit**

```bash
git add crates/argus-job/src/job_manager.rs
git commit -m "refactor(job): rewrite JobManager for unified pipe

Removed get_result(), SSE broadcaster, and wait_for_result.
Added spawn_job_executor() that runs a Turn and sends ThreadEvent::JobResult.
Jobs are now fire-and-forget through the pipe.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 4: argus-thread Changes

### Task 8: Update `thread.rs` — rename event_sender, add run(), send_user_message(), spawn_turn(), is_turn_running()

**Files:**
- Modify: `crates/argus-thread/src/thread.rs`

- [ ] **Step 1: Read the current thread.rs completely** (it is long, ~560 lines)

Key sections to understand:
- `event_sender` field and `subscribe()` method
- `send_message()` method (to be removed)
- `execute_turn_streaming()` method

- [ ] **Step 2: Rename `event_sender` → `pipe_tx`**

Find all occurrences of `event_sender` and rename to `pipe_tx`. This includes:
- The field definition in the Thread struct
- The `subscribe()` method return type
- The `broadcast_to_self()` method
- The build code in `ThreadBuilder::build()`

- [ ] **Step 3: Add `is_turn_running()` state tracking**

Add a new field to the Thread struct:
```rust
/// Whether a Turn is currently running.
#[builder(default)]
turn_running: bool,
```

Update ThreadBuilder build to initialize:
```rust
turn_running: false,
```

Add methods:
```rust
/// Returns true if a Turn is currently executing.
pub fn is_turn_running(&self) -> bool {
    self.turn_running
}

/// Mark that a turn has started.
fn set_turn_running(&mut self, running: bool) {
    self.turn_running = running;
}
```

- [ ] **Step 4: Add `send_user_message()`**

Replace the existing `send_message()` with:
```rust
/// Send a user message into the pipe for processing.
///
/// This is the entry point for external callers (CLI, Tauri).
/// The message is written to the pipe; Thread.run() picks it up.
pub fn send_user_message(
    &self,
    content: String,
    msg_override: Option<MessageOverride>,
) -> Result<(), ThreadError> {
    let event = ThreadEvent::UserMessage {
        content,
        msg_override,
    };
    if self.pipe_tx.send(event).is_err() {
        tracing::warn!("pipe send failed in send_user_message");
    }
    Ok(())
}
```

Also add the necessary import:
```rust
use argus_protocol::MessageOverride;
```

- [ ] **Step 5: Add `spawn_turn()` helper**

Add a private helper that wraps `send_message()` logic:
```rust
async fn spawn_turn(&mut self, msg: UserMessage) -> Result<(), ThreadError> {
    self.set_turn_running(true);
    // Use the existing send_message flow (compaction, override, message push)
    // but call it directly since it handles the turn execution
    self.send_message_internal(msg.content, msg.msg_override).await?;
    self.set_turn_running(false);
    Ok(())
}

/// Internal: send message and execute turn (extracted from send_message).
async fn send_message_internal(
    &mut self,
    user_input: String,
    msg_override: Option<MessageOverride>,
) -> Result<(), ThreadError> {
    let compactor = self.compactor.clone();
    {
        let mut context =
            CompactContext::new(&self.provider, &mut self.token_count, &mut self.messages);
        if let Err(e) = compactor.compact(&mut context).await {
            tracing::warn!("Compact failed: {}", e);
        }
    }

    let effective_record = if let Some(overrides) = msg_override {
        let record = Arc::make_mut(&mut self.agent_record);
        if let Some(v) = overrides.max_tokens {
            record.max_tokens = Some(v);
        }
        if let Some(v) = overrides.temperature {
            record.temperature = Some(v);
        }
        if let Some(v) = overrides.thinking_config {
            record.thinking_config = Some(v);
        }
        self.agent_record.clone()
    } else {
        self.agent_record.clone()
    };

    self.messages.push(ChatMessage::user(user_input));
    self.recalculate_token_count();
    self.execute_turn_streaming(effective_record).await
}
```

- [ ] **Step 6: Add `run()` main orchestration loop**

Add this method to the `impl Thread` block:
```rust
/// Main orchestration loop.
///
/// Runs as a background task (spawned by session). Waits on the pipe,
/// spawning turns when UserMessage arrives. Queues one pending message
/// if a turn is already running.
pub async fn run(&mut self) {
    let mut rx = self.pipe_tx.subscribe();
    let mut pending_user_message: Option<UserMessage> = None;

    loop {
        match rx.recv().await {
            Ok(event) => {
                match event {
                    ThreadEvent::UserMessage(msg) => {
                        if self.is_turn_running() {
                            if pending_user_message.is_none() {
                                pending_user_message = Some(msg);
                            }
                        } else {
                            if let Err(e) = self.spawn_turn(msg).await {
                                tracing::error!("turn failed: {}", e);
                            }
                        }
                    }
                    ThreadEvent::Idle { .. } => {
                        if let Some(msg) = pending_user_message.take() {
                            if let Err(e) = self.spawn_turn(msg).await {
                                tracing::error!("turn failed: {}", e);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Err(e) => {
                tracing::warn!("pipe recv error in run(): {}", e);
            }
        }
    }
}
```

- [ ] **Step 7: Remove `send_message()` from public API**

Change `pub async fn send_message` to `async fn send_message_internal`. The public API is now `send_user_message()`.

- [ ] **Step 8: Verify compiles**

```bash
cargo check -p argus-thread 2>&1 | tail -10
```

Expected: May have errors in downstream (argus-session, argus-turn) — those will be fixed in later tasks.

- [ ] **Step 9: Commit**

```bash
git add crates/argus-thread/src/thread.rs
git commit -m "feat(thread): add unified pipe, run(), send_user_message(), turn state

- Renamed event_sender -> pipe_tx
- Added is_turn_running(), set_turn_running(), turn_running state
- Added send_user_message() as the public entry point
- Added spawn_turn() helper
- Added run() main orchestration loop
- Removed public send_message() (replaced by send_user_message)

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 5: argus-turn Changes

### Task 9: Update `turn.rs` — add pipe receiver, poll at iteration start

**Files:**
- Modify: `crates/argus-turn/src/turn.rs`

- [ ] **Step 1: Read the Turn struct definition and `execute()` method**

Key areas:
- Turn struct fields (around line 185)
- `execute()` method (around line 287)
- `execute_loop()` method (around line 534) — the loop start

- [ ] **Step 2: Add `rx` field to Turn struct**

Add this field inside the Turn struct (after `stream_tx`). Do NOT use `#[builder(default)]` — `broadcast::Receiver` does not implement `Default`:

```rust
/// Receiver for the unified pipe (Thread-owned).
/// Initialized at execute() start via thread_event_tx.subscribe().
rx: broadcast::Receiver<ThreadEvent>,
```

- [ ] **Step 3: Update `execute()` to subscribe and initialize rx**

At the start of `execute()` (right after the trace writer setup), add:

```rust
// Subscribe to the unified pipe for polling at iteration start.
// Each Turn gets its own receiver via broadcast::Sender::subscribe().
self.rx = self.thread_event_tx.subscribe();
```

- [ ] **Step 4: Clarify — no internal channel removal needed**

The `stream_tx` is passed into TurnBuilder from Thread's `execute_turn_streaming()`. It is NOT created internally in Turn's `execute()`. The internal event forwarder creates its own `stream_rx` by subscribing to `stream_tx` — that is correct and should NOT be changed.

Skip Step 4.

- [ ] **Step 5: Update `execute_loop()` to poll the pipe at iteration start**

At the very beginning of the `for iteration in 0..max_iterations` loop (before the `tracing::debug!` line), add:

```rust
// Non-blocking drain of the unified pipe.
// Inject JobResult and UserInterrupt as user messages into the turn.
while let Ok(event) = self.rx.try_recv() {
    match event {
        ThreadEvent::JobResult { job_id, success, message, .. } => {
            let status = if success { "completed" } else { "failed" };
            let content = format!("Job {} {}: {}", job_id, status, message);
            tracing::debug!("injecting JobResult into turn: {}", content);
            messages.push(ChatMessage::user(&content));
        }
        ThreadEvent::UserInterrupt { content } => {
            tracing::debug!("injecting UserInterrupt into turn: {}", content);
            messages.push(ChatMessage::user(&content));
        }
        _ => {}
    }
}
```

Add the import for `broadcast` at the top of the file (it should already be there).

- [ ] **Step 6: Verify compiles**

```bash
cargo check -p argus-turn 2>&1 | tail -10
```

Expected: May have errors in downstream (argus-session) — those will be fixed later.

- [ ] **Step 7: Commit**

```bash
git add crates/argus-turn/src/turn.rs
git commit -m "feat(turn): poll unified pipe at iteration start

- Added rx field to Turn (broadcast::Receiver<ThreadEvent>)
- execute() now subscribes to thread_event_tx to get the receiver
- execute_loop() drains the pipe at each iteration start, injecting
  JobResult and UserInterrupt as user messages

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 6: argus-session Changes

### Task 10: Update session manager — remove get_job_result, spawn thread.run()

**Files:**
- Modify: `crates/argus-session/src/manager.rs`

- [ ] **Step 1: Read the current manager.rs completely**

Key sections:
- Tool registration (lines 42-52)
- `send_message()` method (around line 417)
- `load()` method (around line 220)
- `create_thread()` method (around line 340)

- [ ] **Step 2: Remove get_job_result tool registration**

Find and delete:
```rust
// Register the get_job_result tool for polling job status
let get_result_tool = job_manager.clone().create_get_result_tool();
tool_manager.register(Arc::new(get_result_tool));
```

Also in `crates/argus-job/src/job_manager.rs`, remove the `create_get_result_tool()` method and its import of `GetJobResultTool`:
```rust
// Delete:
use crate::get_job_result_tool::GetJobResultTool;

// Delete this method from JobManager impl:
/// Create a GetJobResultTool for this manager.
pub fn create_get_result_tool(self: Arc<Self>) -> GetJobResultTool {
    GetJobResultTool::new(self)
}
```

- [ ] **Step 3: Update `send_message()` to use `send_user_message()`**

Find `send_message` method (around line 417). Replace the entire method with:

```rust
/// Send a message to a thread via the unified pipe.
pub async fn send_message(
    &self,
    session_id: SessionId,
    thread_id: &ThreadId,
    message: String,
) -> Result<(), ArgusError> {
    let session = self
        .sessions
        .get(&session_id)
        .ok_or(ArgusError::SessionNotFound(session_id))?;

    let thread = session
        .get_thread(thread_id)
        .ok_or(ArgusError::ThreadNotFound(*thread_id))?;

    let thread = thread.lock().await;
    thread
        .send_user_message(message, None)
        .map_err(|e| ArgusError::LlmError {
            reason: e.to_string(),
        })
}
```

- [ ] **Step 4: Add thread.run() spawning in two locations**

There are two places where a Thread is added to a Session:

**Location A — `load()` method (around line 226):**
Find `session.add_thread(thread.clone());`. Before that line, add:
```rust
// Spawn the thread's main orchestration loop
let thread_clone = Arc::clone(&thread);
tokio::spawn(async move {
    let mut t = thread_clone.lock().await;
    t.run().await;
});
session.add_thread(thread.clone());
```

**Location B — `create_thread()` method (around line 348):**
Same pattern — before `session.add_thread(thread.clone())`, add the spawn.

Note: `add_thread` calls `thread.try_lock().map(|t| t.id())` to get the thread ID before inserting. Make sure the spawn is added after the Arc is created but before or after the lock attempt — the Arc keeps the thread alive for the spawned task.

- [ ] **Step 5: Verify compiles**

```bash
cargo check -p argus-session 2>&1 | tail -10
```

Expected: Should compile with all previous chunks. May have errors in argus-wing and desktop.

- [ ] **Step 6: Commit**

```bash
git add crates/argus-session/src/manager.rs
git commit -m "feat(session): remove get_job_result, wire send_message to pipe

- Removed get_job_result tool registration
- send_message() now calls thread.send_user_message() into the pipe
- Spawn thread.run() as background task when thread is added to session

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Chunk 7: argus-wing + Desktop Changes

### Task 11: Update argus-wing — fix JobManager construction and remove broadcaster

**Files:**
- Modify: `crates/argus-wing/src/lib.rs`

> The `send_message` public API is unchanged — it flows through session_manager correctly. The actual changes are to `JobManager::new()` (pool arg removed) and removal of `broadcaster()` calls.

- [ ] **Step 1: Remove pool arg from both JobManager::new() calls**

In `crates/argus-wing/src/lib.rs`, find both calls to `JobManager::new()`. At **each** call site, remove `pool.clone()` as the first argument:

Find:
```rust
let job_manager = Arc::new(JobManager::new(
    pool.clone(),
    template_manager.clone(),
    ...
```

Replace with:
```rust
let job_manager = Arc::new(JobManager::new(
    template_manager.clone(),
    ...
```

- [ ] **Step 2: Remove broadcaster() calls**

In `crates/argus-wing/src/lib.rs`, find and delete:
```rust
let job_broadcaster = Arc::new(job_manager.broadcaster().clone());
```

This line appears twice (once per construction path). Delete both occurrences.

- [ ] **Step 3: Verify compiles**

```bash
cargo check -p argus-wing 2>&1 | tail -10
```

- [ ] **Step 4: Commit**

```bash
git add crates/argus-wing/src/lib.rs
git commit -m "fix(wing): remove pool arg and broadcaster calls from JobManager

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 12: Update desktop Tauri commands

**Files:**
- Modify: `crates/desktop/src-tauri/src/commands.rs`
- Modify: `crates/desktop/lib/tauri.ts`

- [ ] **Step 1: Read the send_message command**

Read `crates/desktop/src-tauri/src/commands.rs`, find the `send_message` function (around line 363).

The desktop command calls `wing.send_message()` which goes through argus-wing → session_manager → thread. Since all those layers now use the pipe internally, the desktop command itself needs no changes. The behavior shifts from blocking-until-complete to fire-and-forget, but the API stays the same.

However, verify:
```bash
cargo check -p desktop 2>&1 | tail -10
```

- [ ] **Step 2: Commit if needed**

If it compiles, skip. If not, fix any type mismatches.

---

## Chunk 8: Verification

### Task 13: Full project check and test

- [ ] **Step 1: Run full cargo check**

```bash
cargo check 2>&1 | tail -30
```

Fix all compilation errors. Common issues:
- Missing imports (`Arc`, `ToolExecutionContext`)
- Tool implementations still using old `(args)` parameter name instead of `(input, ctx)`
- `broadcast::Sender::send` return values not handled (use `.ok()` or `.is_err()`)
- `job_manager.broadcaster()` calls (removed from JobManager) — delete these
- `create_get_result_tool()` calls — delete these
- `get_job_result` in tool registration
- `JobDispatchResult` type usage (removed)

- [ ] **Step 2: Run cargo check on downstream crates**

Also check these crates individually since they may use tool-related types:
```bash
cargo check -p argus-repository 2>&1 | tail -10
cargo check -p desktop 2>&1 | tail -10
```

- [ ] **Step 3: Run cargo test**

```bash
cargo test 2>&1 | tail -20
```

Fix any test failures. Tests may fail because:
- Tests call tools with old `(input)` signature
- Tests expect `JobDispatchResult` type which was removed
- Tests use `send_message()` directly instead of through the pipe

- [ ] **Step 4: Run prek**

```bash
prek 2>&1 | tail -20
```

Fix any lint issues.

- [ ] **Step 5: Commit verification**

```bash
git add -A && git commit -m "fix: resolve compilation and test errors

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Summary of Commits

| Chunk | Task | Commit Message |
|-------|------|----------------|
| 1 | Task 1 | feat(protocol): add ToolExecutionContext, update NamedTool::execute signature |
| 1 | Task 2 | feat(protocol): add JobDispatched, JobResult, UserInterrupt, UserMessage; remove JobCompleted |
| 2 | Task 3 | refactor(tool): update execute signature to accept ToolExecutionContext |
| 3 | Task 4 | refactor(job): remove get_job_result tool |
| 3 | Task 5 | refactor(job): remove wait_for_result and JobDispatchResult |
| 3 | Task 6 | refactor(job): rewrite dispatch_job as fire-and-forget |
| 3 | Task 7 | refactor(job): rewrite JobManager for unified pipe |
| 4 | Task 8 | feat(thread): add unified pipe, run(), send_user_message(), turn state |
| 5 | Task 9 | feat(turn): poll unified pipe at iteration start |
| 6 | Task 10 | feat(session): remove get_job_result, wire send_message to pipe |
| 7 | Task 11 | (skip or fix argus-wing) |
| 7 | Task 12 | (skip or fix desktop) |
| 8 | Task 13 | fix: resolve compilation and test errors |
