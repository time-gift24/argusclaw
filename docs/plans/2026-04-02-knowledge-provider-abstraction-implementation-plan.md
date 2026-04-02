# Knowledge Provider Abstraction Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Introduce operation-level provider traits for the `knowledge` tool so GitHub REST, `git`, and `gh` details are isolated behind default adapters while the current tool behavior stays unchanged.

**Architecture:** Add a new `ops.rs` module that defines `KnowledgeRepoReadOps` and `KnowledgePrOps`, then refactor `GitHubKnowledgeBackend` and `KnowledgePrService` to depend on those traits instead of transport- or CLI-specific abstractions. Keep `GitHubKnowledgeClient`, `GitHubTransport`, `ReqwestGitHubTransport`, and `CliRunner` as implementation details of the default adapters so the public tool contract and default runtime wiring remain stable.

**Tech Stack:** Rust, Tokio, async_trait, reqwest, serde_json, cargo test.

---

### Task 1: Add the provider seam module and generic constructors

**Files:**
- Create: `crates/argus-tool/src/knowledge/ops.rs`
- Modify: `crates/argus-tool/src/knowledge/mod.rs`
- Modify: `crates/argus-tool/src/knowledge/github.rs`
- Modify: `crates/argus-tool/src/knowledge/pr.rs`
- Test: `crates/argus-tool/src/knowledge/mod.rs`

**Step 1: Write the failing constructor tests**

Add unit tests in `crates/argus-tool/src/knowledge/mod.rs` that try to build:

```rust
let backend = GitHubKnowledgeBackend::new_with_ops(vec![repo], FakeRepoReadOps::default());
let service = KnowledgePrService::new_with_ops(FakeKnowledgePrOps::default());
```

Also add tiny fake implementations:

```rust
#[derive(Default)]
struct FakeRepoReadOps;

#[derive(Default)]
struct FakeKnowledgePrOps;
```

The first goal is to make the desired constructor shape compile.

**Step 2: Run the focused constructor tests**

Run: `cargo test -p argus-tool knowledge_backend_constructor_ --lib`
Expected: FAIL because `ops.rs`, `KnowledgeRepoReadOps`, `KnowledgePrOps`, and the new constructors do not exist yet.

**Step 3: Add the new seam module and constructors**

Create `crates/argus-tool/src/knowledge/ops.rs` with the initial traits:

```rust
#[async_trait]
pub trait KnowledgeRepoReadOps: Send + Sync {
    async fn resolve_snapshot(
        &self,
        repo: &KnowledgeRepoDescriptor,
        ref_name: &str,
    ) -> Result<GitHubSnapshot, KnowledgeToolError>;

    async fn read_tree(
        &self,
        repo: &KnowledgeRepoDescriptor,
        rev: &str,
    ) -> Result<GitHubTree, KnowledgeToolError>;

    async fn read_blob(
        &self,
        repo: &KnowledgeRepoDescriptor,
        blob_sha: &str,
    ) -> Result<GitHubBlob, KnowledgeToolError>;
}

#[async_trait]
pub trait KnowledgePrOps: Send + Sync {
    async fn ensure_ready(&self) -> Result<(), KnowledgeToolError>;
    async fn prepare_workspace(
        &self,
        args: &KnowledgeCreatePrArgs,
    ) -> Result<KnowledgePrWorkspace, KnowledgeToolError>;
    async fn commit_and_push(
        &self,
        workspace: &mut KnowledgePrWorkspace,
        commit_message: &str,
    ) -> Result<String, KnowledgeToolError>;
    async fn create_or_reuse_pr(
        &self,
        workspace: &KnowledgePrWorkspace,
        title: &str,
        body: &str,
        draft: bool,
    ) -> Result<GitPrOutcome, KnowledgeToolError>;
}
```

Then:

- export the traits from `crates/argus-tool/src/knowledge/mod.rs`
- add `GitHubKnowledgeBackend::new_with_ops(...)`
- add `KnowledgePrService::new_with_ops(...)`

Do not change the default constructors yet.

**Step 4: Re-run the focused constructor tests**

Run: `cargo test -p argus-tool knowledge_backend_constructor_ --lib`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-tool/src/knowledge/ops.rs crates/argus-tool/src/knowledge/mod.rs crates/argus-tool/src/knowledge/github.rs crates/argus-tool/src/knowledge/pr.rs
git commit -m "refactor(tool): add knowledge provider seam types"
```

### Task 2: Refactor the read path to use `KnowledgeRepoReadOps`

**Files:**
- Modify: `crates/argus-tool/src/knowledge/github.rs`
- Modify: `crates/argus-tool/src/knowledge/tool.rs`
- Modify: `crates/argus-tool/src/knowledge/mod.rs`
- Test: `crates/argus-tool/src/knowledge/mod.rs`
- Test: `crates/argus-tool/tests/knowledge_flow.rs`

**Step 1: Write the failing backend behavior tests**

Add backend-focused tests that use a fake `KnowledgeRepoReadOps` and assert:

- `resolve_snapshot()` receives the expected `KnowledgeRepoDescriptor`
- `read_tree()` is driven by the cached snapshot repo and revision
- `read_blob()` maps `NotFound` to the requested path

Use test names with the `knowledge_backend_` prefix so they can be run in isolation.

**Step 2: Run the focused backend tests**

Run: `cargo test -p argus-tool knowledge_backend_ --lib`
Expected: FAIL because `GitHubKnowledgeBackend` still depends on `GitHubTransport`-shaped wiring.

**Step 3: Rewire `GitHubKnowledgeBackend` and add the default read adapter**

In `crates/argus-tool/src/knowledge/github.rs`:

- introduce `GitHubRestKnowledgeRepoOps<T: GitHubTransport>`
- move GitHub URL construction and client calls behind that adapter
- change `GitHubKnowledgeBackend<T: GitHubTransport>` to `GitHubKnowledgeBackend<O: KnowledgeRepoReadOps>`
- keep `GitHubKnowledgeClient` and `GitHubTransport` intact as adapter internals

The target shape should look like:

```rust
pub struct GitHubRestKnowledgeRepoOps<T: GitHubTransport> {
    client: GitHubKnowledgeClient<T>,
}

pub struct GitHubKnowledgeBackend<O: KnowledgeRepoReadOps> {
    ops: O,
    repos: DashMap<String, KnowledgeRepoDescriptor>,
    snapshots: DashMap<String, SnapshotRecord>,
}
```

Update any constructor call sites in `crates/argus-tool/src/knowledge/tool.rs` to use the new backend constructor.

**Step 4: Re-run the read-path tests**

Run: `cargo test -p argus-tool knowledge_backend_ --lib`
Expected: PASS.

Run: `cargo test -p argus-tool knowledge_github_ --lib`
Expected: PASS, confirming the default GitHub adapter still maps transport responses correctly.

**Step 5: Commit**

```bash
git add crates/argus-tool/src/knowledge/github.rs crates/argus-tool/src/knowledge/tool.rs crates/argus-tool/src/knowledge/mod.rs crates/argus-tool/tests/knowledge_flow.rs
git commit -m "refactor(tool): abstract knowledge read operations"
```

### Task 3: Refactor the PR path to use `KnowledgePrOps`

**Files:**
- Modify: `crates/argus-tool/src/knowledge/pr.rs`
- Modify: `crates/argus-tool/src/knowledge/mod.rs`
- Test: `crates/argus-tool/tests/knowledge_create_pr.rs`

**Step 1: Write the failing PR service tests**

Update `crates/argus-tool/tests/knowledge_create_pr.rs` so the primary fake implements `KnowledgePrOps` instead of `GitPrExecutor`, and add assertions that:

- `ensure_ready()` is called before workspace preparation
- `prepare_workspace()` receives the original request unchanged
- `create_or_reuse_pr()` still determines the returned summary correctly

Keep the existing manifest merge and path validation assertions intact.

**Step 2: Run the focused PR tests**

Run: `cargo test -p argus-tool --test knowledge_create_pr`
Expected: FAIL because `KnowledgePrService` still expects `GitPrExecutor` and the default CLI implementation is not exposed through `KnowledgePrOps`.

**Step 3: Rewire the service and add the default CLI adapter**

In `crates/argus-tool/src/knowledge/pr.rs`:

- add `CliKnowledgePrOps<R: CliRunner>` as the default adapter
- move the current `CliPrExecutor` logic into that adapter, or convert `CliPrExecutor` into a compatibility wrapper around it
- change `KnowledgePrService<E: GitPrExecutor>` to `KnowledgePrService<O: KnowledgePrOps>`
- rename `ensure_auth()` to `ensure_ready()` at the service seam

Preserve the current behavior for:

- `gh auth status`
- `git --version`
- clone and branch setup
- commit and push
- PR reuse or creation

**Step 4: Re-run the focused PR tests**

Run: `cargo test -p argus-tool --test knowledge_create_pr`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-tool/src/knowledge/pr.rs crates/argus-tool/src/knowledge/mod.rs crates/argus-tool/tests/knowledge_create_pr.rs
git commit -m "refactor(tool): abstract knowledge PR operations"
```

### Task 4: Preserve default runtime wiring and public surface

**Files:**
- Modify: `crates/argus-tool/src/knowledge/tool.rs`
- Modify: `crates/argus-tool/src/knowledge/mod.rs`
- Modify: `crates/argus-tool/src/bin/knowledge_cli.rs`
- Test: `crates/argus-tool/src/knowledge/mod.rs`
- Test: `crates/argus-tool/tests/knowledge_flow.rs`

**Step 1: Write the failing default-wiring tests**

Add regression coverage that still compiles and runs:

```rust
let runtime = DefaultKnowledgeRuntime::new();
let tool = KnowledgeTool::new();
```

Also assert the tool definition and JSON actions are unchanged.

**Step 2: Run the focused wiring tests**

Run: `cargo test -p argus-tool knowledge_tool_ --lib`
Expected: FAIL if constructor defaults or re-exports are broken during the refactor.

Run: `cargo test -p argus-tool --test knowledge_flow`
Expected: FAIL if the end-to-end read flow no longer wires the default GitHub adapter correctly.

**Step 3: Finish the default composition and compatibility exports**

Update `crates/argus-tool/src/knowledge/tool.rs` and `crates/argus-tool/src/knowledge/mod.rs` so:

- `DefaultKnowledgeRuntime::new()` builds `GitHubRestKnowledgeRepoOps<ReqwestGitHubTransport>`
- `KnowledgePrService::new()` builds `CliKnowledgePrOps<RealCliRunner>`
- `KnowledgeTool::new()` keeps the same caller experience
- public re-exports remain stable enough for existing tests and downstream code

If keeping `GitPrExecutor` temporarily reduces churn, expose it as a compatibility alias or wrapper and mark it for future cleanup in a follow-up change.

**Step 4: Re-run the focused wiring tests**

Run: `cargo test -p argus-tool knowledge_tool_ --lib`
Expected: PASS.

Run: `cargo test -p argus-tool --test knowledge_flow`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-tool/src/knowledge/tool.rs crates/argus-tool/src/knowledge/mod.rs crates/argus-tool/src/bin/knowledge_cli.rs crates/argus-tool/tests/knowledge_flow.rs
git commit -m "refactor(tool): preserve default knowledge runtime wiring"
```

### Task 5: Run full verification and clean up compatibility edges

**Files:**
- Modify: `crates/argus-tool/src/knowledge/github.rs`
- Modify: `crates/argus-tool/src/knowledge/pr.rs`
- Modify: `crates/argus-tool/src/knowledge/tool.rs`
- Modify: `crates/argus-tool/src/knowledge/mod.rs`
- Test: `crates/argus-tool/src/knowledge/mod.rs`
- Test: `crates/argus-tool/tests/knowledge_flow.rs`
- Test: `crates/argus-tool/tests/knowledge_create_pr.rs`

**Step 1: Remove obvious naming debt that blocks the new seam**

Clean up any leftover references that still force callers through the old low-level abstraction names when the new provider seam is available. Keep this limited to naming and constructor cleanup. Do not generalize the `GitHub*` domain models in this change.

**Step 2: Run formatting**

Run: `cargo fmt --all`
Expected: PASS with no formatting diffs left behind.

**Step 3: Run the targeted verification suite**

Run: `cargo test -p argus-tool knowledge_github_ --lib`
Expected: PASS.

Run: `cargo test -p argus-tool --test knowledge_flow`
Expected: PASS.

Run: `cargo test -p argus-tool --test knowledge_create_pr`
Expected: PASS.

**Step 4: Run the full package test suite**

Run: `cargo test -p argus-tool`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-tool/src/knowledge/ops.rs crates/argus-tool/src/knowledge/github.rs crates/argus-tool/src/knowledge/pr.rs crates/argus-tool/src/knowledge/tool.rs crates/argus-tool/src/knowledge/mod.rs crates/argus-tool/tests/knowledge_flow.rs crates/argus-tool/tests/knowledge_create_pr.rs crates/argus-tool/src/bin/knowledge_cli.rs
git commit -m "refactor(tool): decouple knowledge tool from github operations"
```
