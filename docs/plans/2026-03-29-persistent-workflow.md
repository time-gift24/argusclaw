# Persistent Workflow Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Persist workflow templates and executions, materialize each execution into grouped jobs, and expose start/progress workflow tools to the main agent.

**Architecture:** Add durable workflow-template and workflow-execution persistence to `argus-repository`, then layer a thin `WorkflowManager` in `argus-job` that validates append-only instantiation patches, materializes execution nodes into `jobs`, and reuses existing ready-job dispatch semantics. Keep v1 backend-focused: no runtime graph mutation, no retries, no workflow UI editor.

**Tech Stack:** Rust, SQLx, SQLite, Tokio, Argus repository traits, Argus job execution, agent tools

---

### Task 1: Add Persistent Workflow Schema and Domain Types

**Files:**
- Create: `crates/argus-repository/migrations/20260329160000_persistent_workflow.sql`
- Modify: `crates/argus-repository/src/types/workflow.rs`
- Modify: `crates/argus-repository/src/types/job.rs`
- Modify: `crates/argus-repository/src/types/mod.rs`
- Test: `crates/argus-repository/src/types/workflow.rs`

**Step 1: Write the failing type test**

Add a unit test that asserts the new workflow template node type round-trips dependency keys and that appended execution nodes can carry a stable `node_key`.

```rust
#[test]
fn workflow_template_node_keeps_depends_on_keys() {
    let node = WorkflowTemplateNodeRecord {
        template_id: WorkflowTemplateId::new("tpl-1"),
        node_key: "summarize".to_string(),
        name: "Summarize".to_string(),
        agent_id: AgentId::new(7),
        prompt: "Summarize the repo".to_string(),
        context: None,
        depends_on_keys: vec!["collect".to_string()],
    };

    assert_eq!(node.depends_on_keys, vec!["collect"]);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-repository workflow_template_node_keeps_depends_on_keys -- --exact`
Expected: FAIL with missing workflow template types and fields such as `WorkflowTemplateNodeRecord` or `node_key`.

**Step 3: Write the minimal schema and type changes**

Add a migration that:

- creates `workflow_templates`
- creates `workflow_template_nodes`
- extends `workflows` with `template_id`, `template_version`, and `initiating_thread_id`
- extends `jobs` with `node_key`
- adds a unique index on `(group_id, node_key)`

Extend `workflow.rs` with:

- `WorkflowTemplateId`
- `WorkflowTemplateRecord`
- `WorkflowTemplateNodeRecord`
- `WorkflowExecutionRecord` or expanded `WorkflowRecord`
- instantiate-time helper records for append-only nodes

Add `node_key: Option<String>` to `JobRecord`.

```rust
pub struct WorkflowTemplateNodeRecord {
    pub template_id: WorkflowTemplateId,
    pub node_key: String,
    pub name: String,
    pub agent_id: AgentId,
    pub prompt: String,
    pub context: Option<String>,
    pub depends_on_keys: Vec<String>,
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-repository workflow_template_node_keeps_depends_on_keys -- --exact`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-repository/migrations/20260329160000_persistent_workflow.sql \
        crates/argus-repository/src/types/workflow.rs \
        crates/argus-repository/src/types/job.rs \
        crates/argus-repository/src/types/mod.rs
git commit -m "feat(repository): add persistent workflow schema"
```

### Task 2: Expand Workflow Repository CRUD and Progress Queries

**Files:**
- Modify: `crates/argus-repository/src/traits/workflow.rs`
- Modify: `crates/argus-repository/src/traits/mod.rs`
- Modify: `crates/argus-repository/src/sqlite/workflow.rs`
- Modify: `crates/argus-repository/src/sqlite/job.rs`
- Test: `crates/argus-repository/src/sqlite/workflow.rs`

**Step 1: Write the failing repository test**

Add an async test that:

- creates a template and two template nodes
- creates one workflow execution
- inserts two workflow jobs in the same `group_id`
- asserts the repository can read the execution header and aggregate progress correctly

```rust
#[tokio::test]
async fn list_workflow_execution_progress_counts_grouped_jobs() {
    let repo = test_repo().await;
    let execution_id = WorkflowId::new("wf-1");

    repo.create_workflow_execution(&WorkflowRecord {
        id: execution_id.clone(),
        name: "demo".to_string(),
        status: WorkflowStatus::Pending,
        template_id: Some(WorkflowTemplateId::new("tpl-1")),
        template_version: Some(1),
        initiating_thread_id: None,
    }).await.unwrap();

    let progress = repo.get_workflow_progress(&execution_id).await.unwrap().unwrap();

    assert_eq!(progress.total_jobs, 2);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-repository list_workflow_execution_progress_counts_grouped_jobs -- --exact`
Expected: FAIL with missing repository methods like `create_workflow_execution` or `get_workflow_progress`.

**Step 3: Write the minimal repository API**

Expand `WorkflowRepository` to include:

- template CRUD
- template node list/get
- execution create/get/update
- progress summary query
- execution list by initiating thread

Add SQL in `sqlite/workflow.rs` for:

- inserting templates and template nodes
- inserting and reading workflow executions
- aggregating grouped job counts by status

Add `node_key` binding and reading in `sqlite/job.rs`.

```rust
async fn get_workflow_progress(
    &self,
    id: &WorkflowId,
) -> Result<Option<WorkflowProgressRecord>, DbError>;
```

**Step 4: Run repository tests**

Run: `cargo test -p argus-repository workflow -- --nocapture`
Expected: PASS for the new workflow repository coverage.

**Step 5: Commit**

```bash
git add crates/argus-repository/src/traits/workflow.rs \
        crates/argus-repository/src/traits/mod.rs \
        crates/argus-repository/src/sqlite/workflow.rs \
        crates/argus-repository/src/sqlite/job.rs
git commit -m "feat(repository): add workflow template and progress queries"
```

### Task 3: Persist Job Lifecycle in JobManager

**Files:**
- Modify: `crates/argus-job/src/job_manager.rs`
- Modify: `crates/argus-job/src/lib.rs`
- Modify: `crates/argus-wing/src/lib.rs`
- Test: `crates/argus-job/src/job_manager.rs`

**Step 1: Write the failing job-manager test**

Add a test that verifies a persisted workflow job is marked `running`, then `succeeded`, and stores its result after execution completes.

```rust
#[tokio::test]
async fn persisted_workflow_job_updates_status_and_result() {
    let (manager, repo, thread_id) = test_job_manager_with_repo().await;
    let job_id = JobId::new("job-1");

    manager.spawn_persisted_job_executor(thread_id, job_id.clone()).await.unwrap();

    let job = repo.get(&job_id).await.unwrap().unwrap();
    assert!(matches!(job.status, WorkflowStatus::Succeeded | WorkflowStatus::Running));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-job persisted_workflow_job_updates_status_and_result -- --exact`
Expected: FAIL because `JobManager` has no repository-backed execution path.

**Step 3: Write the minimal persistence wiring**

Inject `Arc<dyn JobRepository>` into `JobManager` and add a repository-backed spawn path that:

- marks the job `running`
- executes the existing lightweight turn
- writes `result`
- marks the job `succeeded` or `failed`

Keep the current ad hoc tracked-job behavior for compatibility, but route workflow jobs through the persisted path.

```rust
pub struct JobManager {
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
    job_repo: Arc<dyn JobRepository>,
    tracked_jobs: Arc<StdMutex<HashMap<String, TrackedJob>>>,
}
```

**Step 4: Run job-manager tests**

Run: `cargo test -p argus-job job_manager -- --nocapture`
Expected: PASS with persisted status and result updates.

**Step 5: Commit**

```bash
git add crates/argus-job/src/job_manager.rs \
        crates/argus-job/src/lib.rs \
        crates/argus-wing/src/lib.rs
git commit -m "feat(job): persist workflow job lifecycle"
```

### Task 4: Implement WorkflowManager Instantiation and Dispatch

**Files:**
- Create: `crates/argus-job/src/workflow_manager.rs`
- Modify: `crates/argus-job/src/lib.rs`
- Modify: `crates/argus-wing/src/lib.rs`
- Test: `crates/argus-job/src/workflow_manager.rs`

**Step 1: Write the failing workflow-manager tests**

Add tests for:

- successful append-only instantiation
- cycle rejection
- ready-node dispatch after upstream success

```rust
#[tokio::test]
async fn instantiate_workflow_materializes_template_and_extra_nodes() {
    let manager = test_workflow_manager().await;

    let execution = manager.instantiate_workflow(InstantiateWorkflowInput {
        template_id: WorkflowTemplateId::new("tpl-1"),
        extra_nodes: vec![AppendWorkflowNode {
            node_key: "extra-review".to_string(),
            name: "Extra Review".to_string(),
            agent_id: AgentId::new(9),
            prompt: "Review final output".to_string(),
            depends_on_keys: vec!["publish".to_string()],
        }],
    }).await.unwrap();

    assert_eq!(execution.total_nodes, 4);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-job instantiate_workflow_materializes_template_and_extra_nodes -- --exact`
Expected: FAIL because `WorkflowManager` and instantiation types do not exist.

**Step 3: Write the minimal manager**

Create `workflow_manager.rs` with:

- template loading
- append-only patch validation
- DAG validation
- execution-row creation
- job-row materialization with resolved `depends_on`
- `dispatch_ready_jobs(workflow_execution_id)`
- aggregate status recomputation after job completion

Prefer a small public API:

```rust
pub async fn instantiate_workflow(
    &self,
    input: InstantiateWorkflowInput,
) -> Result<WorkflowExecutionProgress, JobError>;

pub async fn get_workflow_progress(
    &self,
    execution_id: &WorkflowId,
) -> Result<Option<WorkflowExecutionProgress>, JobError>;
```

**Step 4: Run workflow-manager tests**

Run: `cargo test -p argus-job workflow_manager -- --nocapture`
Expected: PASS for instantiation, validation, and dispatch progression tests.

**Step 5: Commit**

```bash
git add crates/argus-job/src/workflow_manager.rs \
        crates/argus-job/src/lib.rs \
        crates/argus-wing/src/lib.rs
git commit -m "feat(workflow): materialize persistent workflow executions"
```

### Task 5: Expose Workflow Tools to the Main Agent

**Files:**
- Create: `crates/argus-job/src/start_workflow_tool.rs`
- Create: `crates/argus-job/src/get_workflow_progress_tool.rs`
- Modify: `crates/argus-job/src/types.rs`
- Modify: `crates/argus-job/src/lib.rs`
- Modify: `crates/argus-session/src/manager.rs`
- Test: `crates/argus-job/src/start_workflow_tool.rs`
- Test: `crates/argus-job/src/get_workflow_progress_tool.rs`

**Step 1: Write the failing tool tests**

Add a tool test that verifies `start_workflow` returns a workflow execution ID and `get_workflow_progress` returns grouped counts.

```rust
#[tokio::test]
async fn start_workflow_returns_execution_id() {
    let tool = test_start_workflow_tool().await;
    let response = tool.execute(
        serde_json::json!({
            "template_id": "tpl-1",
            "extra_nodes": [],
        }),
        test_ctx(),
    ).await.unwrap();

    assert!(response.get("workflow_execution_id").is_some());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-job start_workflow_returns_execution_id -- --exact`
Expected: FAIL because the workflow tools are not registered or implemented.

**Step 3: Write the minimal tool layer**

Add new tool args/results in `types.rs` and implement:

- `start_workflow`
- `get_workflow_progress`

Register both tools in `SessionManager::new(...)` next to the existing job tools.

```rust
pub struct StartWorkflowArgs {
    pub template_id: String,
    pub extra_nodes: Vec<AppendWorkflowNode>,
}
```

**Step 4: Run tool tests**

Run: `cargo test -p argus-job workflow_tool -- --nocapture`
Expected: PASS for both workflow-tool entry points.

**Step 5: Commit**

```bash
git add crates/argus-job/src/start_workflow_tool.rs \
        crates/argus-job/src/get_workflow_progress_tool.rs \
        crates/argus-job/src/types.rs \
        crates/argus-job/src/lib.rs \
        crates/argus-session/src/manager.rs
git commit -m "feat(workflow): add start and progress tools"
```

### Task 6: Add Wing-Level Workflow API and End-to-End Verification

**Files:**
- Modify: `crates/argus-wing/src/lib.rs`
- Modify: `crates/argus-job/src/workflow_manager.rs`
- Test: `crates/argus-job/src/workflow_manager.rs`
- Test: `crates/argus-job/src/job_manager.rs`

**Step 1: Write the failing end-to-end test**

Add an integration-style test that:

- creates a template
- instantiates a workflow with one appended node
- marks upstream jobs complete
- asserts the workflow status becomes `succeeded`

```rust
#[tokio::test]
async fn workflow_execution_reaches_succeeded_after_all_jobs_finish() {
    let harness = workflow_harness().await;
    let execution_id = harness.instantiate_demo_workflow().await.unwrap();

    harness.complete_all_jobs(&execution_id).await.unwrap();

    let progress = harness.get_progress(&execution_id).await.unwrap().unwrap();
    assert_eq!(progress.status, WorkflowStatus::Succeeded);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-job workflow_execution_reaches_succeeded_after_all_jobs_finish -- --exact`
Expected: FAIL because the full instantiate-dispatch-aggregate loop is not wired end to end.

**Step 3: Write the final glue code**

Expose thin `ArgusWing` methods that forward to `WorkflowManager`, and make sure workflow job completion triggers:

- downstream ready-job dispatch
- workflow aggregate status recomputation

Do not add desktop workflow UI in this task. Keep the public API backend-first and tool-first.

**Step 4: Run the focused verification suite**

Run:

```bash
cargo test -p argus-repository workflow -- --nocapture
cargo test -p argus-job workflow -- --nocapture
prek
```

Expected:

- repository workflow tests PASS
- job/workflow tests PASS
- `prek` passes or auto-fixes formatting and then passes on rerun

**Step 5: Commit**

```bash
git add crates/argus-wing/src/lib.rs \
        crates/argus-job/src/workflow_manager.rs \
        crates/argus-job/src/job_manager.rs
git commit -m "feat(workflow): wire persistent workflow execution through wing"
```

## Scope Guardrails

- Do not build a workflow editor UI in this plan.
- Do not add runtime graph mutation after execution start.
- Do not add retries or compensation flows.
- Do not add workflow cancellation beyond preserving the enum and stored status shape.
- Keep the main-agent surface centered on `start_workflow` and `get_workflow_progress`.
