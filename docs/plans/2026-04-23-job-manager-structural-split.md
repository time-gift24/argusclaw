# Job Manager Structural Split Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Split `crates/argus-job/src/job_manager.rs` into focused internal modules while preserving `JobManager` as the public facade and keeping behavior stable.

**Architecture:** Keep `JobManager` in the existing public module, move related method groups and private stores into a `job_manager/` submodule tree, and preserve the current control flow by delegation rather than redesign. The refactor is structural only: no crate moves, no public API changes, and no intentional runtime behavior changes.

**Tech Stack:** Rust workspace, `argus-job`, `argus-thread-pool`, Tokio broadcast, repository traits, `argus-agent` thread runtime

---

### Task 1: Establish module skeleton and move state/storage definitions

**Files:**
- Modify: `crates/argus-job/src/job_manager.rs`
- Create: `crates/argus-job/src/job_manager/tracking.rs`
- Create: `crates/argus-job/src/job_manager/runtime_state.rs`
- Create: `crates/argus-job/src/job_manager/binding_recovery.rs`
- Create: `crates/argus-job/src/job_manager/persistence.rs`
- Create: `crates/argus-job/src/job_manager/mailbox_result.rs`
- Create: `crates/argus-job/src/job_manager/execution.rs`

**Step 1: Move private store/state types into the submodule tree**

- Keep `JobManager` and `JobLookup` in `job_manager.rs`
- Move `TrackedJobState`, `TrackedJob`, `TrackedJobsStore` into `tracking.rs`
- Move `JobRuntimeStore` and snapshot helpers into `runtime_state.rs`

**Step 2: Wire module declarations and shared visibility**

- Add `mod tracking;`, `mod runtime_state;`, `mod binding_recovery;`, `mod persistence;`, `mod mailbox_result;`, and `mod execution;`
- Use `pub(super)` for helpers shared across sibling modules

**Step 3: Run formatter**

Run: `cargo fmt`

**Step 4: Build the crate**

Run: `cargo test -p argus-job --no-run`
Expected: compile succeeds

### Task 2: Move lookup/runtime/recovery behavior without changing signatures

**Files:**
- Modify: `crates/argus-job/src/job_manager.rs`
- Modify: `crates/argus-job/src/job_manager/tracking.rs`
- Modify: `crates/argus-job/src/job_manager/runtime_state.rs`
- Modify: `crates/argus-job/src/job_manager/binding_recovery.rs`

**Step 1: Move tracking-related methods**

- Move dispatched/completed bookkeeping
- Move lookup / consume / pending helpers
- Keep public signatures unchanged

**Step 2: Move runtime-state helpers**

- Move runtime summary / snapshot / lifecycle bridge helpers
- Preserve event emission behavior

**Step 3: Move binding/recovery helpers**

- Move binding cache helpers
- Move recover parent/child/job-thread lookups
- Preserve metadata sync behavior

**Step 4: Run focused compile check**

Run: `cargo test -p argus-job --no-run`
Expected: compile succeeds

### Task 3: Move persistence and mailbox/result helpers

**Files:**
- Modify: `crates/argus-job/src/job_manager.rs`
- Modify: `crates/argus-job/src/job_manager/persistence.rs`
- Modify: `crates/argus-job/src/job_manager/mailbox_result.rs`

**Step 1: Move persistence helpers**

- Move thread/job persistence helpers and rollback logic
- Preserve repository trait usage and error mapping

**Step 2: Move mailbox/result helpers**

- Move delivered-result shadow helpers
- Move mailbox forward path and result broadcast helpers

**Step 3: Run focused compile check**

Run: `cargo test -p argus-job --no-run`
Expected: compile succeeds

### Task 4: Move execution flow and clean facade

**Files:**
- Modify: `crates/argus-job/src/job_manager.rs`
- Modify: `crates/argus-job/src/job_manager/execution.rs`

**Step 1: Move dispatch/execution methods**

- Move `dispatch_job`, `enqueue_job_runtime`, `execute_job_runtime`, `ensure_job_runtime`, `build_job_thread`, and related wait/result helpers
- Preserve the current event ordering and persistence timing

**Step 2: Reduce facade file to composition-oriented code**

- Keep imports, public types, constructor/setup, and any minimal shared helpers that truly belong at the facade layer

**Step 3: Run full target tests**

Run: `cargo test -p argus-job -- --nocapture`
Expected: existing `argus-job` tests pass

### Task 5: Final polish and verification

**Files:**
- Modify: `crates/argus-job/src/job_manager.rs`
- Modify: `crates/argus-job/src/job_manager/*.rs`

**Step 1: Remove dead imports / visibility leaks**

- Trim unused imports
- Prefer narrow visibility

**Step 2: Re-run formatting and tests**

Run: `cargo fmt`
Run: `cargo test -p argus-job -- --nocapture`
Expected: formatting clean, tests pass

**Step 3: Summarize outcomes**

- Record what moved where
- Note any follow-up cleanup intentionally deferred
