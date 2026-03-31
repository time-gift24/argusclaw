# Knowledge PR Capability Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add `knowledge.create_knowledge_pr` so the existing `knowledge` tool can validate LLM-prepared documentation and manifest updates, write them into a temporary repository checkout, and open a GitHub pull request with local `git` and `gh`.

**Architecture:** Extend the current `knowledge` action model with a payload-driven write action, implement a dedicated PR execution module around `tokio::process::Command`, and make approvals action-aware by deriving an approval key such as `knowledge_create_knowledge_pr` from `tool_input.action`. Keep content generation outside the tool; the tool only validates, writes, pushes, and opens the PR.

**Tech Stack:** Rust, tokio, serde/serde_json, tempfile, git CLI, gh CLI, argus approval hooks

---

### Task 1: Extend `knowledge` models and schema

**Files:**
- Modify: `crates/argus-tool/src/knowledge/models.rs`
- Modify: `crates/argus-tool/src/knowledge/tool.rs`
- Modify: `crates/argus-tool/src/knowledge/mod.rs`

**Step 1: Write the failing contract tests**

Add tests in `crates/argus-tool/src/knowledge/mod.rs` that assert:

- `KnowledgeAction` accepts `create_knowledge_pr`
- the tool definition exposes `create_knowledge_pr`
- parsing a valid PR payload succeeds
- parsing a malformed PR payload fails

**Step 2: Run the focused tests to verify they fail**

Run:

```bash
cargo test -p argus-tool knowledge_tool_definition_lists_expected_actions knowledge_tool_rejects_invalid_action_before_runtime -- --nocapture
```

Expected: FAIL because `create_knowledge_pr` is not yet part of the action enum or schema.

**Step 3: Add the new action and payload structs**

Implement:

- `KnowledgeAction::CreateKnowledgePr`
- `KnowledgeCreatePrArgs`
- `KnowledgeFileWrite`
- `KnowledgeManifestPatch`
- `KnowledgeCreatePrResult`

Keep all fields under `models.rs` so `tool.rs` only dispatches.

**Step 4: Update the tool definition**

Add the new action name and its parameters to the JSON schema in `crates/argus-tool/src/knowledge/tool.rs`.

**Step 5: Re-run the focused tests**

Run:

```bash
cargo test -p argus-tool knowledge_tool_definition_lists_expected_actions knowledge_tool_rejects_invalid_action_before_runtime -- --nocapture
```

Expected: PASS for the contract-level tests.

**Step 6: Commit**

```bash
git add crates/argus-tool/src/knowledge/models.rs crates/argus-tool/src/knowledge/tool.rs crates/argus-tool/src/knowledge/mod.rs
git commit -m "feat: add knowledge create PR action schema"
```

### Task 2: Build manifest merge and path validation helpers

**Files:**
- Create: `crates/argus-tool/src/knowledge/pr.rs`
- Modify: `crates/argus-tool/src/knowledge/mod.rs`
- Test: `crates/argus-tool/tests/knowledge_create_pr.rs`

**Step 1: Write the failing pure-logic tests**

Create tests covering:

- reject absolute paths
- reject `..`
- reject `.git/**`
- create manifest when absent
- upsert `files` by `path`
- upsert `nodes` by `id`
- deduplicate `include` / `exclude` / `entrypoints`

**Step 2: Run the focused tests to verify they fail**

Run:

```bash
cargo test -p argus-tool --test knowledge_create_pr -- --nocapture
```

Expected: FAIL because `pr.rs` and merge helpers do not exist yet.

**Step 3: Implement the pure helpers**

Add to `crates/argus-tool/src/knowledge/pr.rs`:

- `validate_repo_relative_path(path: &str) -> Result<(), KnowledgeToolError>`
- `merge_manifest(existing: Option<RepositoryManifest>, patch: &KnowledgeManifestPatch) -> Result<RepositoryManifest, KnowledgeToolError>`
- stable serialization helpers for manifest output

**Step 4: Export the new module**

Update `crates/argus-tool/src/knowledge/mod.rs` to wire in `pr.rs`.

**Step 5: Re-run the focused tests**

Run:

```bash
cargo test -p argus-tool --test knowledge_create_pr -- --nocapture
```

Expected: PASS for path validation and manifest merge tests.

**Step 6: Commit**

```bash
git add crates/argus-tool/src/knowledge/pr.rs crates/argus-tool/src/knowledge/mod.rs crates/argus-tool/tests/knowledge_create_pr.rs
git commit -m "feat: add knowledge manifest merge helpers"
```

### Task 3: Implement the git and gh executor

**Files:**
- Modify: `crates/argus-tool/Cargo.toml`
- Modify: `crates/argus-tool/src/knowledge/pr.rs`
- Test: `crates/argus-tool/tests/knowledge_create_pr.rs`

**Step 1: Write failing executor tests**

Add tests that model:

- successful clone -> branch -> write -> commit -> push -> PR
- auth failure from `gh auth status`
- PR creation failure after push
- existing PR reuse

Use a fake executor trait so tests do not shell out.

**Step 2: Run the focused tests to verify they fail**

Run:

```bash
cargo test -p argus-tool --test knowledge_create_pr executor -- --nocapture
```

Expected: FAIL because no executor abstraction exists.

**Step 3: Add the runtime dependency for temp directories**

Move `tempfile` into `[dependencies]` in `crates/argus-tool/Cargo.toml` if it is only available in dev-dependencies today.

**Step 4: Implement the executor abstraction**

Add to `crates/argus-tool/src/knowledge/pr.rs`:

- a `GitPrExecutor` trait
- a `CliGitPrExecutor` implementation using `tokio::process::Command`
- helper methods for:
  - `gh auth status`
  - clone
  - checkout base branch
  - create branch
  - add / commit / push
  - create or reuse PR

Do not use `sh -c`; pass command args explicitly.

**Step 5: Re-run the focused tests**

Run:

```bash
cargo test -p argus-tool --test knowledge_create_pr executor -- --nocapture
```

Expected: PASS for fake executor scenarios.

**Step 6: Commit**

```bash
git add crates/argus-tool/Cargo.toml crates/argus-tool/src/knowledge/pr.rs crates/argus-tool/tests/knowledge_create_pr.rs
git commit -m "feat: add knowledge git PR executor"
```

### Task 4: Wire `create_knowledge_pr` into the runtime

**Files:**
- Modify: `crates/argus-tool/src/knowledge/tool.rs`
- Modify: `crates/argus-tool/src/knowledge/mod.rs`
- Test: `crates/argus-tool/tests/knowledge_create_pr.rs`

**Step 1: Write the failing dispatch test**

Add a test that calls:

```json
{ "action": "create_knowledge_pr", ... }
```

and asserts the returned JSON contains:

- `branch`
- `commit_sha`
- `pr_url`
- `manifest_path`

**Step 2: Run the focused test to verify it fails**

Run:

```bash
cargo test -p argus-tool --test knowledge_create_pr dispatch -- --nocapture
```

Expected: FAIL because `tool.rs` does not dispatch the new action.

**Step 3: Implement the runtime dispatch**

In `crates/argus-tool/src/knowledge/tool.rs`:

- parse the new action payload
- invoke the PR module
- render a stable JSON result payload

Keep the write path isolated from read-only actions.

**Step 4: Re-run the focused test**

Run:

```bash
cargo test -p argus-tool --test knowledge_create_pr dispatch -- --nocapture
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-tool/src/knowledge/tool.rs crates/argus-tool/src/knowledge/mod.rs crates/argus-tool/tests/knowledge_create_pr.rs
git commit -m "feat: wire knowledge create PR runtime"
```

### Task 5: Add action-scoped approval support

**Files:**
- Modify: `crates/argus-approval/src/policy.rs`
- Modify: `crates/argus-approval/src/hook.rs`
- Test: `crates/argus-approval/src/hook.rs`
- Test: `crates/argus-approval/src/policy.rs`

**Step 1: Write the failing approval tests**

Add tests asserting:

- `knowledge_create_knowledge_pr` is recognized by policy
- `knowledge.search_nodes` still does not require approval by default
- `knowledge.create_knowledge_pr` does require approval

**Step 2: Run the focused tests to verify they fail**

Run:

```bash
cargo test -p argus-approval policy_requires_approval_method test_approval_hook_skips_tools_not_in_policy -- --nocapture
```

Expected: FAIL because the hook only checks raw `tool_name`.

**Step 3: Implement action-scoped approval resolution**

In `crates/argus-approval/src/hook.rs`:

- read `ctx.tool_input["action"]`
- derive `knowledge_<action>` for the `knowledge` tool
- check policy against the derived key before the raw tool name

In `crates/argus-approval/src/policy.rs`:

- add `knowledge_create_knowledge_pr` to `ApprovalPolicy::default()`

**Step 4: Re-run the focused tests**

Run:

```bash
cargo test -p argus-approval -- --nocapture
```

Expected: PASS for the new hook and policy tests.

**Step 5: Commit**

```bash
git add crates/argus-approval/src/policy.rs crates/argus-approval/src/hook.rs
git commit -m "feat: gate knowledge create PR with action approval"
```

### Task 6: Run end-to-end verification

**Files:**
- Test: `crates/argus-tool/tests/knowledge_create_pr.rs`
- Verify: workspace test commands

**Step 1: Run the targeted new tests**

```bash
cargo test -p argus-tool --test knowledge_create_pr -- --nocapture
cargo test -p argus-approval -- --nocapture
```

Expected: PASS.

**Step 2: Run the broader regression suite**

```bash
cargo test -q
```

Expected: PASS across the workspace.

**Step 3: Review the diff**

```bash
git status --short
git diff --stat
```

Expected: only the planned files are modified.

**Step 4: Commit the verification pass**

```bash
git add crates/argus-tool/Cargo.toml crates/argus-tool/src/knowledge/models.rs crates/argus-tool/src/knowledge/tool.rs crates/argus-tool/src/knowledge/mod.rs crates/argus-tool/src/knowledge/pr.rs crates/argus-tool/tests/knowledge_create_pr.rs crates/argus-approval/src/policy.rs crates/argus-approval/src/hook.rs
git commit -m "feat: add knowledge PR creation workflow"
```

### Task 7: Update documentation and handoff

**Files:**
- Modify: `docs/plans/2026-03-31-knowledge-pr-design.md`
- Modify: `docs/plans/2026-03-31-knowledge-pr-capability.md`

**Step 1: Refresh the design and plan docs if implementation diverged**

Update the docs so they match the final code, especially:

- action name
- manifest merge semantics
- approval key name

**Step 2: Run a final quick diff check**

```bash
git diff -- docs/plans/2026-03-31-knowledge-pr-design.md docs/plans/2026-03-31-knowledge-pr-capability.md
```

Expected: either no diff or only doc sync updates.

**Step 3: Commit doc sync if needed**

```bash
git add docs/plans/2026-03-31-knowledge-pr-design.md docs/plans/2026-03-31-knowledge-pr-capability.md
git commit -m "docs: sync knowledge PR design and plan"
```
