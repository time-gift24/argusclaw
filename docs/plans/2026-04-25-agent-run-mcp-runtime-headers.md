# Agent Run MCP Runtime Headers Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Allow `POST /api/v1/agents/runs` to accept per-run HTTP/SSE MCP header overrides that apply to the root agent and all scheduler-dispatched child agents without persisting or leaking those headers.

**Architecture:** Treat runtime MCP headers as an in-memory run execution context, not as MCP server configuration. `argus-server` validates request headers and owns the run context registry; `argus-agent` passes thread context to MCP resolution; `argus-session` / `argus-job` propagate thread ancestry; `argus-mcp` merges persisted transport headers with run-scoped overlays while leaving repository records untouched.

**Tech Stack:** Rust workspace, axum, serde, tokio, sqlx/SQLite, rmcp/reqwest MCP transports, Vitest/Vue web client tests where API shape changes are exposed.

---

## Current Context

- Worktree: `/Users/wanyaozhong/Projects/argusclaw/.worktrees/agent-run-mcp-runtime-headers`
- Branch: `codex/agent-run-mcp-runtime-headers`
- Base: latest `origin/main` at `b61d2aed`
- Existing run API:
  - `crates/argus-server/src/routes/agent_runs.rs`
  - `crates/argus-server/src/server_core.rs`
  - `crates/argus-repository/src/types/agent_run.rs`
- Existing MCP binding model:
  - `crates/argus-repository/src/sqlite/mcp.rs`
  - `crates/argus-repository/src/traits/mcp.rs`
  - `crates/argus-mcp/src/runtime.rs`
- Existing resolver boundary:
  - `crates/argus-protocol/src/mcp.rs` defines `McpToolResolver::resolve_for_agent(agent_id)`
  - `crates/argus-agent/src/thread.rs` resolves MCP tools inside each thread before turn execution
  - `crates/argus-job/src/job_manager/execution.rs` builds child job threads for scheduler dispatch

## API Shape

Request extension:

```json
{
  "agent_id": 1,
  "prompt": "Use the runtime MCP token",
  "mcp_headers": {
    "12": {
      "Authorization": "Bearer runtime-token",
      "X-Tenant-ID": "tenant-a"
    },
    "analytics-mcp": {
      "X-Trace-ID": "run-123"
    }
  }
}
```

Rules:

- `mcp_headers` is optional.
- Keys are MCP `server_id` strings or MCP `display_name`.
- Numeric `server_id` is preferred and unambiguous.
- `display_name` must resolve to exactly one MCP server.
- Runtime headers only apply to HTTP/SSE MCP servers.
- Runtime headers override persisted headers with the same name.
- Runtime headers are not saved to `agent_runs`, `mcp_servers`, traces, or GET responses.
- Root run and all descendant scheduler job threads inherit the same overlay.

---

### Task 1: Add Protocol Types And Thread-Aware Resolver Context

**Files:**
- Modify: `crates/argus-protocol/src/mcp.rs`
- Modify: `crates/argus-protocol/src/lib.rs`
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: resolver test stubs in `crates/argus-session/src/manager.rs`, `crates/argus-mcp/src/runtime.rs`, and any compile failures from the trait change

**Step 1: Write the failing protocol/compiler-facing test**

Add a small test in `crates/argus-protocol/src/mcp.rs` that proves the new context can carry a thread id and that header overrides are keyed by server id:

```rust
#[test]
fn mcp_resolution_context_carries_thread_and_runtime_headers() {
    let thread_id = crate::ThreadId::new();
    let mut headers = std::collections::BTreeMap::new();
    headers.insert("Authorization".to_string(), "Bearer runtime".to_string());

    let mut overrides = McpRuntimeHeaderOverrides::default();
    overrides.insert(12, headers);

    let context = McpToolResolutionContext {
        thread_id: Some(thread_id),
        runtime_headers: overrides.clone(),
    };

    assert_eq!(context.thread_id, Some(thread_id));
    assert_eq!(
        context.runtime_headers.get(&12).unwrap().get("Authorization"),
        Some(&"Bearer runtime".to_string())
    );
}
```

Expected initial failure: `McpRuntimeHeaderOverrides` / `McpToolResolutionContext` are undefined.

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-protocol mcp_resolution_context_carries_thread_and_runtime_headers -- --nocapture
```

Expected: compile failure mentioning missing types.

**Step 3: Add minimal protocol types**

In `crates/argus-protocol/src/mcp.rs`, add:

```rust
pub type McpRuntimeHeaders = std::collections::BTreeMap<String, String>;
pub type McpRuntimeHeaderOverrides = std::collections::BTreeMap<i64, McpRuntimeHeaders>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpToolResolutionContext {
    pub thread_id: Option<crate::ThreadId>,
    pub runtime_headers: McpRuntimeHeaderOverrides,
}
```

Then update the trait:

```rust
#[async_trait]
pub trait McpToolResolver: Send + Sync {
    async fn resolve_for_agent(
        &self,
        agent_id: AgentId,
        context: McpToolResolutionContext,
    ) -> Result<ResolvedMcpTools>;
}
```

Re-export the new types from `crates/argus-protocol/src/lib.rs`.

**Step 4: Pass thread context from `argus-agent`**

In `crates/argus-agent/src/thread.rs`, update `resolve_mcp_tools` to pass the current thread id:

```rust
let resolved = resolver
    .resolve_for_agent(
        agent_record.id,
        McpToolResolutionContext {
            thread_id: Some(self.id),
            runtime_headers: Default::default(),
        },
    )
    .await
```

This does not yet apply run headers; it only wires thread context through the existing boundary.

**Step 5: Update resolver implementations/stubs**

Update these implementations to accept the new context and ignore it for now:

- `crates/argus-mcp/src/runtime.rs` `impl McpToolResolver for McpRuntimeHandle`
- `crates/argus-session/src/manager.rs` `NoopMcpResolver`
- Any test-only resolver compile failures

**Step 6: Run focused tests**

Run:

```bash
cargo test -p argus-protocol mcp_resolution_context_carries_thread_and_runtime_headers -- --nocapture
cargo test -p argus-agent build_shared_turn_tools_includes_scheduler_for_dispatch_capable_agents -- --nocapture
```

Expected: both pass.

**Step 7: Commit**

```bash
git add crates/argus-protocol/src/mcp.rs crates/argus-protocol/src/lib.rs crates/argus-agent/src/thread.rs crates/argus-session/src/manager.rs crates/argus-mcp/src/runtime.rs
git commit -m "refactor: pass MCP resolution context"
```

---

### Task 2: Add MCP Header Overlay Merge In `argus-mcp`

**Files:**
- Modify: `crates/argus-mcp/src/runtime.rs`
- Modify: `crates/argus-mcp/src/lib.rs` if new public helper/type exports are needed

**Step 1: Write failing unit tests**

Add tests near existing MCP runtime resolver tests in `crates/argus-mcp/src/runtime.rs`:

```rust
#[tokio::test]
async fn resolve_for_agent_runtime_headers_override_persisted_http_headers() {
    let harness = RuntimeHarness::new().await;
    let server_id = harness
        .insert_ready_http_server_with_headers(
            "Tenant MCP",
            [("Authorization", "Bearer persisted"), ("X-Stable", "keep")],
        )
        .await;
    harness.bind_agent_to_server(AgentId::new(7), server_id, None).await;

    let mut runtime_headers = std::collections::BTreeMap::new();
    runtime_headers.insert("Authorization".to_string(), "Bearer runtime".to_string());
    runtime_headers.insert("X-Run".to_string(), "run-1".to_string());

    let resolved = harness
        .handle
        .resolve_for_agent_with_runtime_headers(
            AgentId::new(7),
            [(server_id, runtime_headers)].into_iter().collect(),
        )
        .await
        .expect("runtime headers should resolve");

    assert_eq!(resolved.tools.len(), 1);
    assert_eq!(
        harness.observed_connect_headers(server_id).get("authorization"),
        Some(&"Bearer runtime".to_string())
    );
    assert_eq!(
        harness.observed_connect_headers(server_id).get("x-stable"),
        Some(&"keep".to_string())
    );
}

#[tokio::test]
async fn resolve_for_agent_rejects_runtime_headers_for_stdio_server() {
    let harness = RuntimeHarness::new().await;
    let server_id = harness.insert_ready_stdio_server("Local MCP").await;
    harness.bind_agent_to_server(AgentId::new(7), server_id, None).await;

    let mut runtime_headers = std::collections::BTreeMap::new();
    runtime_headers.insert("Authorization".to_string(), "Bearer runtime".to_string());

    let error = harness
        .handle
        .resolve_for_agent_with_runtime_headers(
            AgentId::new(7),
            [(server_id, runtime_headers)].into_iter().collect(),
        )
        .await
        .expect_err("stdio runtime headers should be rejected");

    assert!(error.to_string().contains("runtime headers only support HTTP/SSE MCP servers"));
}
```

Use existing runtime test harness patterns rather than inventing a new fake runtime if helper names differ.

Expected initial failure: method missing or overlay not applied.

**Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p argus-mcp runtime_headers -- --nocapture
```

Expected: compile failure for missing method or failing assertion.

**Step 3: Add runtime-header aware resolver method**

In `McpRuntimeHandle`, add:

```rust
pub async fn resolve_for_agent_with_runtime_headers(
    &self,
    agent_id: AgentId,
    runtime_headers: McpRuntimeHeaderOverrides,
) -> Result<ResolvedMcpTools, McpRuntimeError> {
    let executor: Arc<dyn McpToolExecutor> = Arc::new(self.clone());
    self.inner
        .resolve_for_agent_with_executor_and_headers(agent_id, executor, runtime_headers)
        .await
}
```

Refactor existing `resolve_for_agent_with_executor` into a helper that accepts `McpRuntimeHeaderOverrides`.

**Step 4: Merge persisted and runtime headers per bound server**

When iterating bindings in `resolve_for_agent_with_executor...`, before creating tools/session for a server:

```rust
let runtime_headers_for_server = runtime_headers.get(&binding.server.server_id);
let effective_record = apply_runtime_headers(&entry.record, runtime_headers_for_server)?;
```

Implement:

```rust
fn apply_runtime_headers(
    record: &McpServerRecord,
    runtime_headers: Option<&McpRuntimeHeaders>,
) -> Result<McpServerRecord, McpRuntimeError> {
    let Some(runtime_headers) = runtime_headers else {
        return Ok(record.clone());
    };

    let mut record = record.clone();
    match &mut record.transport {
        McpTransportConfig::Http { headers, .. } | McpTransportConfig::Sse { headers, .. } => {
            for (name, value) in runtime_headers {
                headers.insert(name.clone(), value.clone());
            }
            Ok(record)
        }
        McpTransportConfig::Stdio { .. } => Err(McpRuntimeError::InvalidConfig {
            reason: "runtime headers only support HTTP/SSE MCP servers".to_string(),
        }),
    }
}
```

Use the project’s actual MCP error variant names if `InvalidConfig` differs.

**Step 5: Keep persisted records untouched**

Add/keep an assertion in the test that `repo.get_mcp_server(server_id)` still returns the persisted header value. If the existing harness makes this hard, add a focused helper.

**Step 6: Run focused tests**

Run:

```bash
cargo test -p argus-mcp runtime_headers -- --nocapture
```

Expected: tests pass.

**Step 7: Commit**

```bash
git add crates/argus-mcp/src/runtime.rs crates/argus-mcp/src/lib.rs
git commit -m "feat: merge runtime MCP headers"
```

---

### Task 3: Add Run Context Registry In `argus-server`

**Files:**
- Create: `crates/argus-server/src/agent_run_context.rs`
- Modify: `crates/argus-server/src/server_core.rs`
- Modify: `crates/argus-server/src/lib.rs` or `crates/argus-server/src/routes/mod.rs` only if module wiring requires it

**Step 1: Write failing registry tests**

Create `crates/argus-server/src/agent_run_context.rs` with tests first:

```rust
#[tokio::test]
async fn registry_inherits_run_headers_from_parent_to_child_thread() {
    let registry = AgentRunContextRegistry::default();
    let run_id = AgentRunId::new();
    let parent = ThreadId::new();
    let child = ThreadId::new();
    let mut headers = McpRuntimeHeaderOverrides::default();
    headers.insert(12, [("Authorization".to_string(), "Bearer runtime".to_string())].into());

    registry.register_run_thread(run_id, parent, headers.clone()).await;
    registry.inherit_thread(parent, child).await.unwrap();

    assert_eq!(registry.headers_for_thread(child).await, headers);
}

#[tokio::test]
async fn registry_cleanup_removes_all_thread_indexes_for_run() {
    let registry = AgentRunContextRegistry::default();
    let run_id = AgentRunId::new();
    let parent = ThreadId::new();
    let child = ThreadId::new();

    registry.register_run_thread(run_id, parent, Default::default()).await;
    registry.inherit_thread(parent, child).await.unwrap();
    registry.remove_run(run_id).await;

    assert!(registry.headers_for_thread(parent).await.is_empty());
    assert!(registry.headers_for_thread(child).await.is_empty());
}
```

Expected initial failure: registry types missing.

**Step 2: Run test to verify failure**

Run:

```bash
cargo test -p argus-server agent_run_context -- --nocapture
```

Expected: compile failure.

**Step 3: Implement registry**

Implement a narrow in-memory registry:

```rust
#[derive(Clone, Default)]
pub struct AgentRunContextRegistry {
    inner: Arc<RwLock<AgentRunContextState>>,
}

#[derive(Default)]
struct AgentRunContextState {
    runs: HashMap<AgentRunId, AgentRunContext>,
    thread_to_run: HashMap<ThreadId, AgentRunId>,
}

#[derive(Clone)]
struct AgentRunContext {
    headers: McpRuntimeHeaderOverrides,
    threads: HashSet<ThreadId>,
}
```

Public methods:

- `register_run_thread(run_id, thread_id, headers)`
- `inherit_thread(parent_thread_id, child_thread_id) -> Result<(), ArgusError>`
- `headers_for_thread(thread_id) -> McpRuntimeHeaderOverrides`
- `remove_run(run_id)`

Keep methods async only if using `tokio::sync::RwLock`; otherwise sync methods are fine.

**Step 4: Wire registry into `ServerCore`**

Add a field:

```rust
agent_run_contexts: AgentRunContextRegistry,
```

Initialize it in `ServerCore::init` and test constructors.

**Step 5: Run focused tests**

Run:

```bash
cargo test -p argus-server agent_run_context -- --nocapture
```

Expected: registry tests pass.

**Step 6: Commit**

```bash
git add crates/argus-server/src/agent_run_context.rs crates/argus-server/src/server_core.rs crates/argus-server/src/lib.rs
git commit -m "feat: add agent run context registry"
```

---

### Task 4: Validate And Parse `mcp_headers` In Agent Run Route

**Files:**
- Modify: `crates/argus-server/src/routes/agent_runs.rs`
- Modify: `crates/argus-server/src/server_core.rs`
- Test: `crates/argus-server/tests/chat_api.rs` or create `crates/argus-server/tests/agent_runs_api.rs` if the suite has been split

**Step 1: Write failing route tests**

Add tests:

```rust
#[tokio::test]
async fn agent_run_accepts_runtime_mcp_headers_without_exposing_them() {
    let ctx = TestServerContext::new().await;
    let template = ctx.seed_template("Runner").await;
    let mcp_server = ctx.seed_http_mcp_server("tenant-mcp").await;

    let created = ctx
        .post_json(
            "/api/v1/agents/runs",
            &serde_json::json!({
                "agent_id": template.id,
                "prompt": "Use tenant MCP",
                "mcp_headers": {
                    mcp_server.to_string(): {
                        "Authorization": "Bearer runtime",
                        "X-Tenant-ID": "tenant-a"
                    }
                }
            }),
        )
        .await;

    assert_eq!(created.status(), StatusCode::CREATED);
    let body = created.json().await;
    assert!(body["data"]["run_id"].as_str().is_some());
    assert!(body.to_string().contains("Bearer runtime") == false);
}

#[tokio::test]
async fn agent_run_rejects_headers_for_unknown_mcp_server() {
    let ctx = TestServerContext::new().await;
    let template = ctx.seed_template("Runner").await;

    let response = ctx
        .post_json(
            "/api/v1/agents/runs",
            &serde_json::json!({
                "agent_id": template.id,
                "prompt": "Use tenant MCP",
                "mcp_headers": {
                    "999999": { "Authorization": "Bearer runtime" }
                }
            }),
        )
        .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
```

Adapt to actual `TestServerContext` helper names.

**Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p argus-server agent_run_accepts_runtime_mcp_headers_without_exposing_them agent_run_rejects_headers_for_unknown_mcp_server -- --nocapture
```

Expected: request field ignored or helper missing until implemented.

**Step 3: Extend request DTO**

In `CreateAgentRunRequest`:

```rust
#[serde(default)]
pub mcp_headers: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
```

Do not add these headers to response structs.

**Step 4: Validate and normalize in `ServerCore`**

Add:

```rust
async fn resolve_mcp_header_overrides(
    &self,
    raw: BTreeMap<String, BTreeMap<String, String>>,
) -> Result<McpRuntimeHeaderOverrides>
```

Validation rules:

- Empty map is OK.
- Numeric key resolves via `mcp_repo.get_mcp_server(id)`.
- Non-numeric key resolves by exact `display_name` from `mcp_repo.list_mcp_servers()`.
- Missing server returns bad request.
- Duplicate display name match returns bad request.
- Target transport must be HTTP/SSE.
- Header names/values must pass reqwest/header parsing. Reuse or expose `argus-mcp` header parsing helper if possible; otherwise use `http::HeaderName` and `http::HeaderValue` in server.

Use structured `ArgusError` / `ApiError` style already used by server. Keep route handler thin: it should call `ServerCore`.

**Step 5: Register context before sending message**

Change `create_agent_run(agent_id, prompt)` to accept normalized `McpRuntimeHeaderOverrides`.

After root `thread_id` and `run_id` are known, before `send_message`:

```rust
self.agent_run_contexts
    .register_run_thread(run_id, thread_id, mcp_headers.clone());
```

On failure before run starts, cleanup the registry.

**Step 6: Run focused tests**

Run:

```bash
cargo test -p argus-server agent_run -- --nocapture
```

Expected: route tests pass.

**Step 7: Commit**

```bash
git add crates/argus-server/src/routes/agent_runs.rs crates/argus-server/src/server_core.rs crates/argus-server/tests
git commit -m "feat: accept run-scoped MCP headers"
```

---

### Task 5: Apply Registry Headers During MCP Resolution

**Files:**
- Modify: `crates/argus-server/src/server_core.rs`
- Modify: `crates/argus-server/src/agent_run_context.rs`
- Modify: `crates/argus-mcp/src/runtime.rs`

**Step 1: Write failing resolver wrapper test**

Add a server unit test for a wrapper resolver:

```rust
#[tokio::test]
async fn run_scoped_resolver_uses_thread_headers() {
    let runtime = StubMcpResolver::default();
    let registry = AgentRunContextRegistry::default();
    let run_id = AgentRunId::new();
    let thread_id = ThreadId::new();
    let mut headers = McpRuntimeHeaderOverrides::default();
    headers.insert(12, [("Authorization".to_string(), "Bearer runtime".to_string())].into());
    registry.register_run_thread(run_id, thread_id, headers.clone()).await;

    let resolver = RunScopedMcpToolResolver::new(runtime.clone(), registry);
    resolver
        .resolve_for_agent(
            AgentId::new(7),
            McpToolResolutionContext {
                thread_id: Some(thread_id),
                runtime_headers: Default::default(),
            },
        )
        .await
        .unwrap();

    assert_eq!(runtime.last_runtime_headers(), headers);
}
```

Use a test-only fake that records headers.

**Step 2: Run test to verify failure**

Run:

```bash
cargo test -p argus-server run_scoped_resolver_uses_thread_headers -- --nocapture
```

Expected: wrapper missing.

**Step 3: Add run-scoped resolver**

In `crates/argus-server/src/server_core.rs` or a small private module:

```rust
struct RunScopedMcpToolResolver {
    inner: McpRuntimeHandle,
    registry: AgentRunContextRegistry,
}
```

Implement `McpToolResolver`:

```rust
async fn resolve_for_agent(
    &self,
    agent_id: AgentId,
    context: McpToolResolutionContext,
) -> argus_protocol::Result<ResolvedMcpTools> {
    let headers = match context.thread_id {
        Some(thread_id) => self.registry.headers_for_thread(thread_id).await,
        None => Default::default(),
    };
    self.inner
        .resolve_for_agent_with_runtime_headers(agent_id, headers)
        .await
        .map_err(ArgusError::from)
}
```

Use this wrapper instead of raw `McpRuntime::handle(&mcp_runtime)` when wiring `SessionManager` and `JobManager` in `ServerCore`.

**Step 4: Ensure normal paths still work**

If no run context exists for the thread, `headers_for_thread` returns empty overrides and behavior is identical to today.

**Step 5: Run focused tests**

Run:

```bash
cargo test -p argus-server run_scoped_resolver_uses_thread_headers -- --nocapture
cargo test -p argus-server -- --nocapture
```

Expected: pass.

**Step 6: Commit**

```bash
git add crates/argus-server/src/server_core.rs crates/argus-server/src/agent_run_context.rs crates/argus-mcp/src/runtime.rs
git commit -m "feat: resolve MCP tools with run headers"
```

---

### Task 6: Propagate Run Context To Scheduler Child Job Threads

**Files:**
- Modify: `crates/argus-session/src/manager.rs`
- Modify: `crates/argus-job/src/job_manager/execution.rs`
- Modify: `crates/argus-job/src/job_manager/mod.rs`
- Modify: `crates/argus-server/src/server_core.rs`

**Step 1: Write failing propagation test**

Add a server or session test that exercises inheritance:

```rust
#[tokio::test]
async fn dispatch_job_inherits_agent_run_mcp_headers_to_child_thread() {
    let ctx = TestServerContext::new().await;
    let root_agent = ctx.seed_agent_with_subagent("Root", "Worker").await;
    let child_agent = ctx.find_agent("Worker").await;
    let mcp_server = ctx.seed_http_mcp_server("tenant-mcp").await;
    ctx.bind_agent_to_mcp(child_agent.id, mcp_server, None).await;

    let run = ctx
        .create_agent_run_with_headers(
            root_agent.id,
            "dispatch a job to Worker",
            [(mcp_server, [("Authorization", "Bearer runtime")])],
        )
        .await;

    let child_thread_id = ctx.dispatch_child_job_for_run(run.run_id, child_agent.id).await;

    assert_eq!(
        ctx.agent_run_contexts()
            .headers_for_thread(child_thread_id)
            .await
            .get(&mcp_server)
            .unwrap()
            .get("Authorization"),
        Some(&"Bearer runtime".to_string())
    );
}
```

If direct integration is hard, write a narrower test around a new `JobDispatchObserver`/callback that inherits `originating_thread_id -> execution_thread_id`.

**Step 2: Run test to verify failure**

Run:

```bash
cargo test -p argus-server dispatch_job_inherits_agent_run_mcp_headers_to_child_thread -- --nocapture
```

Expected: child thread not registered in context.

**Step 3: Add a narrow child-thread inheritance callback**

Avoid making `argus-job` understand agent runs or MCP headers. Add a generic callback to `JobManager`:

```rust
pub type JobThreadCreatedHook =
    Arc<dyn Fn(ThreadId, ThreadId) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;
```

Or simpler if sync registry methods are used:

```rust
pub type JobThreadCreatedHook = Arc<dyn Fn(ThreadId, ThreadId) + Send + Sync>;
```

Fields/methods:

- `job_thread_created_hook: Arc<StdMutex<Option<JobThreadCreatedHook>>>`
- `set_job_thread_created_hook(...)`

Call the hook after `enqueue_job_runtime` returns `execution_thread_id`:

```rust
self.notify_job_thread_created(originating_thread_id, execution_thread_id);
```

**Step 4: Wire hook in `ServerCore`**

After constructing `JobManager`, install:

```rust
let registry = agent_run_contexts.clone();
job_manager.set_job_thread_created_hook(Arc::new(move |parent, child| {
    registry.inherit_thread_sync(parent, child);
}));
```

If the registry uses async locks, spawn a small task instead.

**Step 5: Ensure nested dispatch inherits naturally**

Because child jobs become originating threads for grandchildren, the same hook should inherit context at every depth.

**Step 6: Cleanup on run completion**

In `track_agent_run`, after marking completed/failed, call:

```rust
agent_run_contexts.remove_run(run_id).await;
```

Pass registry into `track_agent_run`.

**Step 7: Run focused tests**

Run:

```bash
cargo test -p argus-server dispatch_job_inherits_agent_run_mcp_headers_to_child_thread -- --nocapture
cargo test -p argus-session scheduler -- --nocapture
cargo test -p argus-job dispatch_job -- --nocapture
```

Expected: pass.

**Step 8: Commit**

```bash
git add crates/argus-session/src/manager.rs crates/argus-job/src/job_manager crates/argus-server/src/server_core.rs crates/argus-server/src/agent_run_context.rs
git commit -m "feat: inherit run context for child jobs"
```

---

### Task 7: Prevent Header Leakage And Add Concurrency Coverage

**Files:**
- Modify: `crates/argus-server/tests/chat_api.rs` or `crates/argus-server/tests/agent_runs_api.rs`
- Modify: `crates/argus-repository/tests/agent_run_repository.rs`
- Modify: `crates/argus-mcp/src/runtime.rs`

**Step 1: Write no-leak tests**

Repository test:

```rust
#[tokio::test]
async fn agent_run_repository_does_not_persist_runtime_headers() {
    let repo = make_repo().await;
    let run = run_record();

    AgentRunRepository::insert_agent_run(&repo, &run).await.unwrap();
    let stored = AgentRunRepository::get_agent_run(&repo, &run.id)
        .await
        .unwrap()
        .unwrap();

    let serialized = serde_json::to_string(&stored).unwrap();
    assert!(!serialized.contains("Authorization"));
    assert!(!serialized.contains("Bearer"));
}
```

Route test:

```rust
#[tokio::test]
async fn get_agent_run_never_returns_runtime_mcp_headers() {
    let ctx = TestServerContext::new().await;
    let run = ctx
        .create_agent_run_with_headers(/* Authorization: Bearer runtime */)
        .await;

    let response = ctx.get(&format!("/api/v1/agents/runs/{}", run.run_id)).await;
    let text = response.text().await;

    assert!(!text.contains("Bearer runtime"));
    assert!(!text.contains("Authorization"));
}
```

**Step 2: Write concurrency test**

In MCP runtime tests, resolve the same server twice with different overlays and assert each fake connection observes its own header:

```rust
#[tokio::test]
async fn concurrent_runtime_headers_for_same_server_do_not_cross_contaminate() {
    let harness = RuntimeHarness::new().await;
    let server_id = harness.insert_ready_http_server_with_headers("Tenant MCP", []).await;
    harness.bind_agent_to_server(AgentId::new(7), server_id, None).await;

    let first = harness.handle.resolve_for_agent_with_runtime_headers(
        AgentId::new(7),
        [(server_id, [("Authorization".to_string(), "Bearer first".to_string())].into())]
            .into_iter()
            .collect(),
    );
    let second = harness.handle.resolve_for_agent_with_runtime_headers(
        AgentId::new(7),
        [(server_id, [("Authorization".to_string(), "Bearer second".to_string())].into())]
            .into_iter()
            .collect(),
    );

    let (first, second) = tokio::join!(first, second);
    first.unwrap();
    second.unwrap();

    assert!(harness.observed_authorizations().contains(&"Bearer first".to_string()));
    assert!(harness.observed_authorizations().contains(&"Bearer second".to_string()));
}
```

Use actual harness observation APIs.

**Step 3: Run tests to verify failure/pass**

Run:

```bash
cargo test -p argus-repository agent_run_repository_does_not_persist_runtime_headers -- --nocapture
cargo test -p argus-server get_agent_run_never_returns_runtime_mcp_headers -- --nocapture
cargo test -p argus-mcp concurrent_runtime_headers -- --nocapture
```

Expected: pass after previous tasks; if any fail, fix leakage before continuing.

**Step 4: Commit**

```bash
git add crates/argus-server/tests crates/argus-repository/tests crates/argus-mcp/src/runtime.rs
git commit -m "test: cover MCP runtime header isolation"
```

---

### Task 8: Update Web API Types Without Adding UI

**Files:**
- Modify: `apps/web/src/lib/api.ts`
- Modify: `apps/web/src/lib/api.test.ts`
- Do not modify Agent Runs UI unless explicitly requested later

**Step 1: Write failing API client test**

In `apps/web/src/lib/api.test.ts`, extend create-run request expectations:

```ts
it("passes optional runtime MCP headers when creating agent runs", async () => {
  const fetchMock = vi.fn().mockResolvedValue({
    ok: true,
    headers: { get: () => "application/json" },
    json: async () => ({
      data: {
        run_id: "run-1",
        agent_id: 7,
        status: "queued",
        created_at: "2026-04-25T00:00:00Z",
        updated_at: "2026-04-25T00:00:00Z",
      },
    }),
  });
  vi.stubGlobal("fetch", fetchMock);

  await getApiClient().createAgentRun!({
    agent_id: 7,
    prompt: "Inspect",
    mcp_headers: {
      "12": {
        Authorization: "Bearer runtime",
      },
    },
  });

  expect(fetchMock).toHaveBeenCalledWith("/api/v1/agents/runs", {
    body: JSON.stringify({
      agent_id: 7,
      prompt: "Inspect",
      mcp_headers: {
        "12": {
          Authorization: "Bearer runtime",
        },
      },
    }),
    headers: { "Content-Type": "application/json" },
    method: "POST",
  });
});
```

**Step 2: Run test to verify failure**

Run:

```bash
pnpm --dir apps/web exec vitest run src/lib/api.test.ts
```

Expected: TypeScript compile failure until type is added, or test passes if request shape is already unconstrained. If it passes immediately, keep the test as regression coverage.

**Step 3: Update `CreateAgentRunRequest`**

In `apps/web/src/lib/api.ts`:

```ts
export type McpRuntimeHeaders = Record<string, string>;
export type McpRuntimeHeaderOverrides = Record<string, McpRuntimeHeaders>;

export interface CreateAgentRunRequest {
  agent_id: number;
  prompt: string;
  mcp_headers?: McpRuntimeHeaderOverrides;
}
```

**Step 4: Run web checks**

Run:

```bash
pnpm --dir apps/web exec vitest run src/lib/api.test.ts
pnpm --dir apps/web exec vue-tsc --noEmit
```

Expected: pass.

**Step 5: Commit**

```bash
git add apps/web/src/lib/api.ts apps/web/src/lib/api.test.ts
git commit -m "chore: type agent run MCP header requests"
```

---

### Task 9: Document The Contract And Boundaries

**Files:**
- Modify: `crates/argus-server/AGENTS.md`
- Modify: `crates/argus-mcp/AGENTS.md` if present; otherwise do not create a new one unless needed
- Modify: `apps/web/AGENTS.md` only if web API typing changed
- Modify: `apps/web/DESIGN.md` API section if it documents `POST /api/v1/agents/runs`

**Step 1: Update server guidance**

Add a concise note under `argus-server` public API / modification rules:

```markdown
- `POST /api/v1/agents/runs` may accept run-scoped HTTP/SSE MCP header overrides. These headers are memory-only execution context, inherited by scheduler child job threads, and must never be persisted or returned by `GET /agents/runs/{run_id}`.
```

**Step 2: Update MCP guidance if file exists**

If `crates/argus-mcp/AGENTS.md` exists, add:

```markdown
- Runtime MCP header overlays may override persisted HTTP/SSE transport headers for a single resolution context. Do not mutate persisted server records when applying overlays.
```

**Step 3: Update web/API docs**

If `apps/web/DESIGN.md` lists request body details, add the optional `mcp_headers` note to Agent Runs. Keep docs concise; do not create extra docs/plans noise.

**Step 4: Run docs grep**

Run:

```bash
rg -n "mcp_headers|runtime MCP header|Agent Runs" crates/argus-server apps/web crates/argus-mcp
```

Expected: only intentional docs/code/test references.

**Step 5: Commit**

```bash
git add crates/argus-server/AGENTS.md crates/argus-mcp/AGENTS.md apps/web/AGENTS.md apps/web/DESIGN.md
git commit -m "docs: describe run-scoped MCP headers"
```

---

### Task 10: Final Verification

**Files:**
- No code edits expected

**Step 1: Format**

Run:

```bash
cargo fmt --check
```

Expected: no output, exit 0.

**Step 2: Rust tests**

Run:

```bash
cargo test -p argus-protocol -- --nocapture
cargo test -p argus-mcp -- --nocapture
cargo test -p argus-repository -- --nocapture
cargo test -p argus-server -- --nocapture
cargo test -p argus-session scheduler -- --nocapture
cargo test -p argus-job dispatch_job -- --nocapture
```

Expected: all pass.

**Step 3: Web tests**

Run:

```bash
pnpm --dir apps/web exec vitest run src/lib/api.test.ts
pnpm --dir apps/web exec vue-tsc --noEmit
```

Expected: pass.

**Step 4: Architecture checks**

Run:

```bash
cargo tree -p argus-server | rg argus-wing
rg 'argus_wing|ArgusWing' crates/argus-server
rg -n "admin_settings|settings" crates/argus-server crates/argus-repository apps/web
```

Expected:

- `cargo tree ... | rg argus-wing` has no matches.
- `argus_wing` has no matches in server.
- `ArgusWing` only appears as product/brand copy if present.
- No settings/admin_settings logic is reintroduced.

**Step 5: Full pre-commit**

Run:

```bash
prek
```

Expected: all hooks pass.

**Step 6: Final commit if needed**

If any verification-only fixes were made:

```bash
git add -A
git commit -m "fix: stabilize run-scoped MCP headers"
```

---

## Rollback And Safety Notes

- Runtime headers must never be written into SQLite or trace files.
- `GET /api/v1/agents/runs/{run_id}` must never expose header names/values from the create request.
- If the server restarts while a run is still in progress, the in-memory header context is gone. The run may still be queryable, but continuing MCP calls that require the dynamic header should fail clearly rather than silently falling back to persisted credentials.
- Stdio env overrides are explicitly out of scope for this change.
- Inline temporary MCP server creation is explicitly out of scope for this change.
- `apps/web` UI for editing runtime headers is out of scope unless requested separately.
