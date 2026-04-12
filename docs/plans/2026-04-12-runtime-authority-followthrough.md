# Runtime Authority Followthrough Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Finish the ThreadRuntime migration by fixing scheduler mailbox readiness, restoring chat lifecycle observability, and wiring the desktop monitor to the new runtime authority.

**Architecture:** Keep job execution and job-pool telemetry in `argus-job::ThreadPool`, but treat `argus-agent::ThreadRuntime` as the authority for chat runtime readiness and lifecycle state. Update `argus-session` to route chat mailbox validation through `ThreadRuntime`, then switch the desktop monitor to consume `thread_runtime_state()` while still applying job-pool delta events.

**Tech Stack:** Rust (`argus-agent`, `argus-session`, `argus-job`, `argus-protocol`), TypeScript (`crates/desktop/lib/chat-store.ts`), Tauri bindings.

---

### Task 1: Scheduler mailbox readiness

**Files:**
- Modify: `crates/argus-session/src/manager.rs`

**Step 1: Write the failing test**

Add coverage for `parent` or `thread:<chat-id>` mailbox targets when the target is a chat runtime tracked in `ThreadRuntime` but not in `ThreadPool`.

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-session mailbox_`
Expected: FAIL because readiness still treats `ThreadPool` as the target authority.

**Step 3: Write minimal implementation**

Update mailbox target validation so chat targets resolve through `ThreadRuntime`, while job targets keep the existing `ThreadPool` readiness rules.

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-session mailbox_`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-session/src/manager.rs
git commit -m "fix(session): validate chat mailbox targets via thread runtime"
```

### Task 2: ThreadRuntime lifecycle summaries

**Files:**
- Modify: `crates/argus-agent/src/thread_runtime.rs`
- Test: `crates/argus-agent/src/thread_runtime.rs`

**Step 1: Write the failing test**

Add a unit test showing that a loaded chat runtime remains `Inactive` even after thread events indicate it is running or idle.

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-agent thread_runtime`
Expected: FAIL because summaries are not updated from forwarded thread events.

**Step 3: Write minimal implementation**

Translate forwarded thread events into summary state updates for chat runtimes so `thread_runtime_state()` reflects loading, running, cooling/evicted, and last activity consistently.

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-agent thread_runtime`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-agent/src/thread_runtime.rs
git commit -m "fix(agent): reflect chat lifecycle in thread runtime state"
```

### Task 3: Desktop monitor authority

**Files:**
- Modify: `crates/desktop/lib/chat-store.ts`
- Test: `crates/desktop/tests/chat-store-session-model.test.mjs`

**Step 1: Write the failing test**

Update the store test to expect polling via `threadRuntime.getState()` instead of `threadPool.getState()`.

**Step 2: Run test to verify it fails**

Run: `node --test crates/desktop/tests/chat-store-session-model.test.mjs`
Expected: FAIL because the store still polls the old API.

**Step 3: Write minimal implementation**

Switch snapshot polling to `threadRuntime.getState()` and keep the existing delta-event handling for job-pool telemetry.

**Step 4: Run test to verify it passes**

Run: `node --test crates/desktop/tests/chat-store-session-model.test.mjs`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/desktop/lib/chat-store.ts crates/desktop/tests/chat-store-session-model.test.mjs
git commit -m "fix(desktop): observe thread runtime authority"
```

### Task 4: Final verification

**Files:**
- Verify only

**Step 1: Run focused checks**

Run:
- `cargo test -p argus-session`
- `cargo test -p argus-agent thread_runtime`
- `node --test crates/desktop/tests/chat-store-session-model.test.mjs crates/desktop/tests/chat-tauri-bindings.test.mjs`

**Step 2: Run cross-crate verification**

Run: `cargo test -p argus-agent -p argus-session -p argus-job -p argus-wing`
Expected: PASS

**Step 3: Commit**

```bash
git add docs/plans/2026-04-12-runtime-authority-followthrough.md
git commit -m "docs: add runtime authority followthrough plan"
```
