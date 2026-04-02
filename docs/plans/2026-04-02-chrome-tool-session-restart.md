# Chrome Tool Session Restart Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make the `chrome` tool explicitly document shared-session `open` behavior and add `close` plus `restart` actions for lightweight shutdown and force-restart flows.

**Architecture:** Extend the existing chrome action schema instead of adding a second tool. Keep production single-session behavior in `ChromeManager`, add a restart helper that composes shutdown plus open, and update user-facing descriptions in the tool definition and agent prompt to reflect the shared-session model.

**Tech Stack:** Rust, serde, Tokio, thirtyfour, TOML agent descriptors

---

### Task 1: Add failing tests for action schema and copy

**Files:**
- Modify: `crates/argus-tool/src/chrome/mod.rs`
- Modify: `agents/chrome_explore.toml`

**Step 1: Write the failing tests**

- Add tests asserting `close` and `restart` validation behavior.
- Add tests asserting the tool definition enum exposes `close` and `restart`.
- Add assertions that the tool description mentions shared session behavior.

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-tool close_requires_session_id restart_requires_url chrome_tool_definition_lists_only_readonly_actions chrome_tool_definition_lists_interactive_actions`

Expected: FAIL because the new actions and updated description do not exist yet.

**Step 3: Write minimal implementation**

- Extend `ChromeAction`, argument validation, and tool definition copy to satisfy the new expectations.

**Step 4: Run test to verify it passes**

Run the same `cargo test` command.

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-tool/src/chrome/mod.rs crates/argus-tool/src/chrome/models.rs crates/argus-tool/src/chrome/tool.rs agents/chrome_explore.toml
git commit -m "feat: document chrome shared session actions"
```

### Task 2: Add failing tests for close and restart behavior

**Files:**
- Modify: `crates/argus-tool/src/chrome/mod.rs`
- Modify: `crates/argus-tool/src/chrome/manager.rs`
- Modify: `crates/argus-tool/src/chrome/tool.rs`

**Step 1: Write the failing test**

- Add a tool-level test for `close` shutting down the tracked session.
- Add a tool-level or manager-level test for `restart` returning a fresh session.
- Add a managed-host test proving restart clears the reusable driver process.

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-tool chrome_tool_close_shuts_down_session chrome_tool_restart_returns_fresh_session`

Expected: FAIL because the actions are not implemented.

**Step 3: Write minimal implementation**

- Add `ChromeManager::restart`.
- Add a host reset path that shuts down the shared process for managed production sessions.
- Dispatch the new actions from `ChromeTool::execute`.

**Step 4: Run test to verify it passes**

Run the same `cargo test` command.

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-tool/src/chrome/mod.rs crates/argus-tool/src/chrome/manager.rs crates/argus-tool/src/chrome/tool.rs
git commit -m "feat: add chrome close and restart actions"
```

### Task 3: Verify the end-to-end chrome surface

**Files:**
- Modify: `crates/argus-tool/src/chrome/mod.rs`
- Modify: `crates/argus-tool/src/chrome/models.rs`
- Modify: `crates/argus-tool/src/chrome/manager.rs`
- Modify: `crates/argus-tool/src/chrome/tool.rs`
- Modify: `agents/chrome_explore.toml`

**Step 1: Run focused tests**

Run: `cargo test -p argus-tool chrome`

Expected: PASS for the chrome-focused unit tests.

**Step 2: Run formatting and lint-friendly checks**

Run: `cargo fmt --all --check`

Expected: PASS, or apply formatting and re-run.

**Step 3: Summarize assumptions**

- Note that planning docs were created inside the worktree to keep implementation traceable, even though the repository has a separate convention for `docs/`.

**Step 4: Commit**

```bash
git add agents/chrome_explore.toml crates/argus-tool/src/chrome docs/plans/2026-04-02-chrome-tool-session-restart-design.md docs/plans/2026-04-02-chrome-tool-session-restart.md
git commit -m "feat: add chrome session restart controls"
```
