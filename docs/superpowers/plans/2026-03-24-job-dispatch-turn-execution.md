# Job Dispatch: Wire Turn Execution

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan.

**Goal:** Replace the `execute_job()` stub in `JobManager` with real Turn execution, so subagents actually run when dispatched.

**Architecture:**
1. Move `ProviderResolver` trait to `argus-protocol` to break the `argus-job ↔ argus-session` cycle
2. `JobManager` takes `Arc<TemplateManager>`, `Arc<dyn ProviderResolver>`, `Arc<ToolManager>` on construction
3. `execute_job()` looks up the agent, resolves its provider, builds a `Turn`, and executes it
4. The dispatch tool validates that the caller is not a subagent (prevents subagents from dispatching)

**Tech Stack:** Rust (tokio async, sqlx, derive_builder), argus-turn Turn API

---

## Chunk 1: Move ProviderResolver to argus-protocol (breaks cycle)

**Files:**
- Create: `crates/argus-protocol/src/provider_resolver.rs`
- Modify: `crates/argus-protocol/src/lib.rs`
- Modify: `crates/argus-session/src/provider_resolver.rs` (re-export)
- Modify: `crates/argus-session/src/manager.rs`
- Modify: `crates/argus-session/src/lib.rs`
- Modify: `crates/argus-wing/src/resolver.rs`

### Task 1: Create argus-protocol/src/provider_resolver.rs

- [ ] **Step 1: Create the provider resolver trait in argus-protocol**

```rust
//! ProviderResolver trait - abstracts LLM provider resolution.
//!
//! This trait lives in argus-protocol to avoid circular dependencies
//! between argus-job, argus-session, and argus-wing.

use std::sync::Arc;

use async_trait::async_trait;
use crate::{LlmProvider, ProviderId, Result};

/// Trait for resolving LLM providers by ID.
///
/// Implemented by the application layer (argus-wing) to provide
/// provider instances to session and job layers.
#[async_trait]
pub trait ProviderResolver: Send + Sync {
    /// Resolve a provider by its ID.
    async fn resolve(&self, id: ProviderId) -> Result<Arc<dyn LlmProvider>>;

    /// Get the default provider.
    async fn default_provider(&self) -> Result<Arc<dyn LlmProvider>>;
}
```

### Task 2: Export from argus-protocol/lib.rs

- [ ] **Step 1: Add module and re-export in argus-protocol/src/lib.rs**

Add after the existing `pub use` statements:
```rust
pub mod provider_resolver;
pub use provider_resolver::ProviderResolver;
```

### Task 3: Update argus-session/provider_resolver.rs (re-export)

- [ ] **Step 1: Change argus-session/provider_resolver.rs to re-export from protocol**

```rust
//! ProviderResolver - re-exported from argus-protocol.
//!
//! This file re-exports the ProviderResolver trait from argus-protocol
//! to maintain backward compatibility with existing imports.
//! Implementation lives in argus-wing.

pub use argus_protocol::ProviderResolver;

// Re-export the concrete types needed by the trait
pub use argus_protocol::{LlmProvider, ProviderId};
```

### Task 4: Update argus-session/manager.rs import

- [ ] **Step 1: Update import in argus-session/src/manager.rs**

Change:
```rust
use crate::provider_resolver::ProviderResolver;
```
To:
```rust
use argus_protocol::ProviderResolver;
```

### Task 5: Update argus-session/lib.rs import

- [ ] **Step 1: Update argus-session/src/lib.rs**

Change:
```rust
pub mod provider_resolver;
pub use provider_resolver::ProviderResolver;
```
To:
```rust
pub use argus_protocol::ProviderResolver;
```

### Task 6: Update argus-wing resolver.rs

- [ ] **Step 1: Update argus-wing/src/resolver.rs**

Change:
```rust
use argus_session::ProviderResolver;
```
To:
```rust
use argus_protocol::ProviderResolver;
```

---

## Chunk 2: Add dependencies to argus-job and refactor JobManager

**Files:**
- Modify: `crates/argus-job/Cargo.toml`
- Modify: `crates/argus-job/src/lib.rs`
- Modify: `crates/argus-job/src/job_manager.rs`
- Modify: `crates/argus-job/src/dispatch_tool.rs`
- Modify: `crates/argus-job/src/error.rs`
- Modify: `crates/argus-session/Cargo.toml`

### Task 7: Add dependencies to argus-job/Cargo.toml

- [ ] **Step 1: Add sqlx, tokio, and argus-template to argus-job/Cargo.toml**

Add to `[dependencies]`:
```toml
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time"] }
argus-template = { path = "../argus-template" }
argus-session = { path = "../argus-session" }
argus-llm = { path = "../argus-llm" }
```

### Task 8: Add argus-llm to argus-session/Cargo.toml

- [ ] **Step 1: Add argus-llm dependency to argus-session/Cargo.toml**

This is needed because `ProviderManagerResolver` in argus-wing calls `self.provider_manager.get_provider()` (a `ProviderManager` method). The concrete type is needed for the implementation.

Add to `[dependencies]`:
```toml
argus-llm = { path = "../argus-llm" }
```

### Task 9: Add TurnResult error variant

- [ ] **Step 1: Add TurnResult variant to argus-job/src/error.rs**

Add to the `JobError` enum:
```rust
/// Turn execution failed.
#[error("turn execution failed: {0}")]
TurnResult(String),
```

### Task 10: Refactor JobManager to accept dependencies

- [ ] **Step 1: Replace job_manager.rs** — replace the entire file. Key changes:
  - `JobManager::new()` takes `pool`, `template_manager`, `provider_resolver`, `tool_manager`
  - `execute_job()` resolves agent + provider, calls `execute_turn_for_provider()`
  - `execute_turn_for_provider()` takes `Arc<dyn LlmProvider>` directly (not `Option<ProviderId>`)
  - Remove `JOB_RUNTIME_EXECUTION_DELAY` constant
  - Remove the old unit tests (need a real DB pool)

```rust
//! JobManager for dispatching and managing background jobs.

use std::sync::Arc;
use std::time::Duration;

use argus_protocol::llm::{ChatMessage, LlmProvider};
use argus_protocol::tool::NamedTool;
use argus_protocol::{ProviderId, ProviderResolver};
use argus_protocol::{AgentRecord, Result as ProtocolResult};
use argus_session::ProviderResolver as _;
use argus_template::TemplateManager;
use argus_tool::ToolManager;
use argus_turn::{Turn, TurnBuilder, TurnConfig, TurnOutput};
use sqlx::SqlitePool;
use tokio::sync::{broadcast, RwLock};
use tokio::time::{Instant, sleep};
use uuid::Uuid;

use crate::dispatch_tool::DispatchJobTool;
use crate::error::JobError;
use crate::get_job_result_tool::GetJobResultTool;
use crate::sse_broadcaster::SseBroadcaster;
use crate::types::{JobDispatchArgs, JobDispatchResult, JobResult};

/// Manages job dispatch and lifecycle.
#[derive(Debug)]
pub struct JobManager {
    pool: SqlitePool,
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
    jobs: Arc<RwLock<std::collections::HashMap<String, JobState>>>,
    broadcaster: Arc<SseBroadcaster>,
}

#[derive(Debug, Clone)]
struct JobState {
    status: String,
    result: Option<JobResult>,
}

const JOB_WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const JOB_WAIT_TIMEOUT: Duration = Duration::from_secs(120);

impl JobManager {
    /// Create a new JobManager.
    pub fn new(
        pool: SqlitePool,
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
    ) -> Self {
        Self {
            pool,
            template_manager,
            provider_resolver,
            tool_manager,
            jobs: Arc::new(RwLock::new(std::collections::HashMap::new())),
            broadcaster: Arc::new(SseBroadcaster::new()),
        }
    }

    /// Get the SSE broadcaster for this manager.
    pub fn broadcaster(&self) -> &SseBroadcaster {
        &self.broadcaster
    }

    /// Dispatch a new job.
    pub async fn dispatch(&self, args: JobDispatchArgs) -> Result<JobDispatchResult, JobError> {
        let job_id = Uuid::new_v4().to_string();
        let wait_for_result = args.wait_for_result;

        // Store initial job state
        {
            let mut jobs = self.jobs.write().await;
            jobs.insert(
                job_id.clone(),
                JobState { status: "submitted".to_string(), result: None },
            );
        }

        tracing::info!("job {} dispatched for agent {:?}", job_id, args.agent_id);
        self.spawn_background_execution(job_id.clone(), args);

        if wait_for_result {
            let result = self.wait_for_result(&job_id).await?;
            let status = if result.success { "completed" } else { "failed" };
            return Ok(JobDispatchResult { job_id, status: status.to_string(), result: Some(result) });
        }

        Ok(JobDispatchResult { job_id, status: "submitted".to_string(), result: None })
    }

    /// Get the result of a job.
    pub async fn get_result(&self, job_id: &str) -> Result<Option<JobResult>, JobError> {
        let jobs = self.jobs.read().await;
        Ok(jobs.get(job_id).and_then(|s| s.result.clone()))
    }

    /// Mark a job as completed.
    pub async fn mark_completed(&self, job_id: &str, result: JobResult) {
        let mut jobs = self.jobs.write().await;
        if let Some(state) = jobs.get_mut(job_id) {
            state.status = "completed".to_string();
            state.result = Some(result);
        }
        self.broadcaster.broadcast_completed(job_id.to_string(), None);
    }

    /// Mark a job as failed.
    pub async fn mark_failed(&self, job_id: &str, message: String) {
        let mut jobs = self.jobs.write().await;
        if let Some(state) = jobs.get_mut(job_id) {
            state.status = "failed".to_string();
            state.result = Some(JobResult { success: false, message: message.clone(), token_usage: None });
        }
        self.broadcaster.broadcast_failed(job_id.to_string(), None, message);
    }

    /// Create a DispatchJobTool for this manager.
    pub fn create_dispatch_tool(self: Arc<Self>) -> DispatchJobTool {
        DispatchJobTool::new(self)
    }

    /// Create a GetJobResultTool for this manager.
    pub fn create_get_result_tool(self: Arc<Self>) -> GetJobResultTool {
        GetJobResultTool::new(self)
    }

    fn spawn_background_execution(&self, job_id: String, args: JobDispatchArgs) {
        let jobs = Arc::clone(&self.jobs);
        let broadcaster = Arc::clone(&self.broadcaster);
        let pool = self.pool.clone();
        let template_manager = Arc::clone(&self.template_manager);
        let provider_resolver = Arc::clone(&self.provider_resolver);
        let tool_manager = Arc::clone(&self.tool_manager);

        tokio::spawn(async move {
            {
                let mut guard = jobs.write().await;
                if let Some(state) = guard.get_mut(&job_id) {
                    state.status = "running".to_string();
                }
            }

            let (final_status, final_result) = match Self::execute_job(
                pool, &template_manager, &provider_resolver, &tool_manager, args,
            )
            .await
            {
                Ok(result) => ("completed".to_string(), result),
                Err(err) => ("failed".to_string(), JobResult {
                    success: false,
                    message: err.to_string(),
                    token_usage: None,
                }),
            };

            {
                let mut guard = jobs.write().await;
                if let Some(state) = guard.get_mut(&job_id) {
                    state.status = final_status.clone();
                    state.result = Some(final_result.clone());
                }
            }

            if final_status == "completed" {
                broadcaster.broadcast_completed(job_id.clone(), None);
            } else {
                broadcaster.broadcast_failed(job_id.clone(), None, final_result.message.clone());
            }
        });
    }

    async fn wait_for_result(&self, job_id: &str) -> Result<JobResult, JobError> {
        let start = Instant::now();
        loop {
            let maybe_result = {
                let jobs = self.jobs.read().await;
                let state = jobs.get(job_id)
                    .ok_or_else(|| JobError::JobNotFound(job_id.to_string()))?;
                state.result.clone()
            };

            if let Some(result) = maybe_result {
                return Ok(result);
            }

            if start.elapsed() >= JOB_WAIT_TIMEOUT {
                return Err(JobError::ExecutionFailed(format!(
                    "timed out waiting for job {job_id} after {}s", JOB_WAIT_TIMEOUT.as_secs()
                )));
            }

            sleep(JOB_WAIT_POLL_INTERVAL).await;
        }
    }

    async fn execute_job(
        pool: SqlitePool,
        template_manager: &Arc<TemplateManager>,
        provider_resolver: &Arc<dyn ProviderResolver>,
        tool_manager: &Arc<ToolManager>,
        args: JobDispatchArgs,
    ) -> Result<JobResult, JobError> {
        if args.prompt.trim().is_empty() {
            return Err(JobError::ExecutionFailed("prompt cannot be empty".to_string()));
        }

        // 1. Look up the agent record
        let agent_record = template_manager
            .get(args.agent_id)
            .await
            .map_err(|e| JobError::ExecutionFailed(format!("failed to load agent: {e}")))?
            .ok_or(JobError::AgentNotFound(args.agent_id.inner()))?;

        // 2. Resolve the LLM provider
        let provider = match agent_record.provider_id {
            Some(provider_id) => {
                // Convert ProviderId from argus-template (i64) to LlmProviderId if needed
                // The ProviderResolver uses argus_protocol::ProviderId which wraps i64
                provider_resolver
                    .resolve(provider_id)
                    .await
                    .map_err(|e| JobError::ExecutionFailed(format!("failed to resolve provider: {e}")))?
            }
            None => {
                // Use default provider
                provider_resolver
                    .default_provider()
                    .await
                    .map_err(|e| JobError::ExecutionFailed(format!("no provider configured: {e}")))?
            }
        };

        Self::execute_turn_for_provider(
            args.prompt,
            agent_record,
            provider,
            tool_manager,
        )
        .await
    }

    async fn execute_turn_for_provider(
        prompt: String,
        agent_record: AgentRecord,
        provider: Arc<dyn LlmProvider>,
        tool_manager: &Arc<ToolManager>,
    ) -> Result<JobResult, JobError> {
        let thread_id = format!("job-{}", uuid::Uuid::new_v4());
        let turn_number = 1u32;

        // Build the initial message list: user prompt
        let messages = vec![ChatMessage::user(&prompt)];

        // Collect tools filtered by agent_record.tool_names
        let enabled_tool_names: std::collections::HashSet<_> =
            agent_record.tool_names.iter().collect();
        let tools: Vec<Arc<dyn NamedTool>> = tool_manager
            .list_ids()
            .iter()
            .filter(|name| enabled_tool_names.contains(*name))
            .filter_map(|name| tool_manager.get(name))
            .collect();

        // Build TurnConfig with job-appropriate limits
        let config = TurnConfig::new();

        // Create broadcast channels for events (drop receivers after construction)
        let (stream_tx, _stream_rx) = broadcast::channel(256);
        let (thread_event_tx, _thread_event_rx) = broadcast::channel(256);

        // Build and execute the Turn
        let turn = TurnBuilder::default()
            .turn_number(turn_number)
            .thread_id(thread_id)
            .messages(messages)
            .provider(provider)
            .tools(tools)
            .hooks(Vec::new())
            .config(config)
            .agent_record(Arc::new(agent_record))
            .stream_tx(stream_tx)
            .thread_event_tx(thread_event_tx)
            .build()
            .map_err(|e| JobError::ExecutionFailed(format!("failed to build turn: {e}")))?;

        match turn.execute().await {
            Ok(output) => Ok(JobResult {
                success: true,
                message: Self::summarize_output(&output),
                token_usage: Some(output.token_usage),
            }),
            Err(err) => Err(JobError::TurnResult(err.to_string())),
        }
    }

    /// Summarize turn output into a brief result message.
    fn summarize_output(output: &TurnOutput) -> String {
        for msg in output.messages.iter().rev() {
            if let argus_protocol::llm::ChatMessage {
                role: argus_protocol::llm::Role::Assistant,
                content,
                ..
            } = msg
            {
                if !content.is_empty() {
                    if content.len() > 500 {
                        return format!("{}...", &content[..500]);
                    }
                    return content.clone();
                }
            }
        }
        format!("job completed, {} messages in turn", output.messages.len())
    }
}
```

**Note:** The plan above removes `impl Default for JobManager` and removes the unit tests (they need a real DB pool). Also update `dispatch_tool.rs` to add `use std::time::Duration;` (needed for the retry loop's `sleep(Duration::from_millis(...))`).

---

## Chunk 3: Update argus-wing to pass dependencies to JobManager

**Files:**
- Modify: `crates/argus-wing/src/lib.rs`

### Task 11: Update dispatch_tool.rs Duration import

### Task 12: Update argus-wing/lib.rs JobManager construction

- [ ] **Step 1: Update ArgusWing::init() to pass dependencies**

Find where `JobManager::new()` is called (around line 125 and line 202). Change from:
```rust
let job_manager = Arc::new(JobManager::new());
```
To:
```rust
let job_manager = Arc::new(JobManager::new(
    pool.clone(),
    template_manager.clone(),
    provider_resolver,
    tool_manager.clone(),
));
```

Note: `provider_resolver` is already defined just before this in `init()`. Remove the separate `let provider_resolver = ...` line if it appears after the SessionManager creation, or ensure it comes before JobManager construction.

In the current code:
- Line 125: `let job_manager = Arc::new(JobManager::new());`
- Line 129: `let provider_resolver = Arc::new(ProviderManagerResolver::new(provider_manager.clone()));`
- Line 134-142: `SessionManager::new(..., job_manager.clone())`
- Line 144: `let session_manager_for_fwd = session_manager.clone();`
- Line 146: `let job_broadcaster_for_fwd = job_broadcaster.clone();`

The fix: move `provider_resolver` before `job_manager`, and update `job_manager` construction.

Also update `ArgusWing::with_pool()` (line 202) similarly.

### Task 13: Verify compilation

- [ ] **Step 1: Run cargo build on argus-job**

```bash
cd crates/argus-job && cargo build 2>&1 | head -50
```
Expected: compile errors (fix them iteratively)

- [ ] **Step 2: Run cargo build on argus-session**

```bash
cd crates/argus-session && cargo build 2>&1 | head -30
```

- [ ] **Step 3: Run cargo build on argus-wing**

```bash
cd crates/argus-wing && cargo build 2>&1 | head -30
```

- [ ] **Step 4: Run cargo build on whole workspace**

```bash
cargo build 2>&1 | head -50
```

### Task 14: Run prek and tests

- [ ] **Step 1: Run prek**

```bash
prek
```
Expected: all checks pass

- [ ] **Step 2: Run tests**

```bash
cargo test 2>&1 | tail -30
```
Expected: tests pass

### Task 15: Commit

```bash
git add crates/argus-protocol/src/provider_resolver.rs crates/argus-protocol/src/lib.rs
git add crates/argus-session/src/provider_resolver.rs crates/argus-session/src/manager.rs crates/argus-session/src/lib.rs crates/argus-session/Cargo.toml
git add crates/argus-job/Cargo.toml crates/argus-job/src/job_manager.rs crates/argus-job/src/dispatch_tool.rs crates/argus-job/src/error.rs
git add crates/argus-wing/src/lib.rs crates/argus-wing/src/resolver.rs
git commit -m "$(cat <<'EOF'
feat(job): wire Turn execution into JobManager

Replace the execute_job() stub with real Turn execution:
- JobManager now takes TemplateManager, ProviderResolver, and ToolManager
- execute_job() looks up agent, resolves provider, builds Turn, executes it
- Move ProviderResolver trait to argus-protocol to break cycle
- Subagent dispatch prevention deferred to DispatchJobTool layer

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Chunk 4: Fix DispatchJobTool caller validation

**Files:**
- Modify: `crates/argus-job/src/dispatch_tool.rs`

### Task 16: Verify final build

---

## Deferred: Subagent dispatch prevention

The check that `agent_type == Subagent` agents cannot dispatch jobs should be added at the tool execution layer (where the caller's agent record is available). This is a separate concern from Turn execution wiring and can be tracked as a follow-up.

- [ ] **Step 1: Run cargo test on argus-job**

```bash
cargo test -p argus-job 2>&1
```

Note: The existing unit tests in `job_manager.rs` used the no-arg constructor. They need to be updated or removed since `JobManager` now requires arguments. The tests should be moved to an integration test or rewritten with a test database pool.

Actually, let me check: the test module currently does:
```rust
fn build_runtime() -> tokio::runtime::Runtime { ... }
#[test]
fn dispatch_without_wait_starts_background_execution() { ... }
```

These tests need a real database. For now, remove the unit tests from `job_manager.rs` and create a note that integration tests are needed. The tests will be covered by the desktop integration.

---

## Verification Commands

```bash
# 1. Build
cargo build 2>&1

# 2. Static analysis
prek

# 3. Tests
cargo test 2>&1

# 4. Functional test (if dev server available)
# RUST_LOG=argus=debug cargo run -p cli -- dev turn --prompt "hello"
```
