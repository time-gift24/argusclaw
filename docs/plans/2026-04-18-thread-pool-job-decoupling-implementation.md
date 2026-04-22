# ThreadPool / Job 解耦实现计划

**Goal:** Make `ThreadPool` chat/runtime-only, move job orchestration and job runtime monitoring into `JobManager`, split the public monitor surface into `ThreadPoolState + JobRuntimeState`, and keep the naming cleanup separate.

**Architecture:** `ThreadPool` remains the runtime residency / lifecycle / delivery layer for chat/runtime threads. `JobManager` owns job binding, recovery, execution, persistence, result shadow, and the full job runtime state/event stream. `SessionScheduler` consumes explicit job APIs from `JobManager`. Desktop merges chat/runtime and job runtime sources in the store layer.

**Tech Stack:** Rust, TypeScript, `argus-job`, `argus-session`, `argus-wing`, `argus-protocol`, desktop Tauri/Zustand

---

### Task 1: Update the design docs to reflect the split monitor model

**Files:**
- Modify: `docs/plans/2026-04-18-thread-pool-job-decoupling-design.md`
- Modify: `docs/plans/2026-04-18-thread-pool-job-decoupling-implementation.md`

**Deliverables:**

- The design doc states the new ownership boundary and removes the old “unified thread-pool compatibility view” assumption.
- The implementation doc records the execution order, dual-source monitor strategy, and deferred naming follow-up.

---

### Task 2: Lock current job behavior before removing job semantics from `ThreadPool`

**Files:**
- Modify: `crates/argus-job/src/job_manager.rs`
- Modify: `crates/argus-job/src/thread_pool.rs`
- Modify: `crates/argus-session/src/manager.rs`

**Behavior to keep stable:**

- job dispatch creates or recovers an execution thread binding
- child job discovery / parent recovery still work after restart
- job result is delivered back to the originating thread and can be consumed exactly once
- scheduler `job:` / `parent` / `*` target resolution remains intact
- thread monitor still shows both chat and job runtimes after the split
- stop-job still targets only job runtimes

Add or preserve targeted tests before moving logic across module boundaries.

---

### Task 3: Move job runtime ownership into `JobManager`

**Files:**
- Modify: `crates/argus-job/src/job_manager.rs`
- Modify: `crates/argus-job/src/types.rs`
- Modify: `crates/argus-job/src/lib.rs`

**Changes:**

- Rename `ThreadPoolJobRequest` to `JobExecutionRequest`.
- Add a private `JobRuntimeStore` inside `JobManager` for:
  - binding cache
  - parent/child job-thread relationships
  - delivered job-result shadow
  - authoritative `job_runtimes`
- Move job-specific helpers from `ThreadPool` into `JobManager`:
  - binding persistence / recovery
  - job runtime construction and loading
  - job status/result persistence
  - task-assignment execution
  - turn-result waiting
  - delivered job-result claim

`dispatch_job` becomes the single entry point that owns the full job execution pipeline.

---

### Task 4: Shrink `ThreadPool` to chat/runtime-pool responsibilities

**Files:**
- Modify: `crates/argus-job/src/thread_pool.rs`

**Changes:**

- Remove job-binding caches and job-result shadow from `ThreadPoolStore`.
- Remove job-facing public methods from `ThreadPool`.
- Keep only runtime-pool capabilities:
  - registration / summary state
  - slot management
  - runtime attach / load helpers
  - runtime lifecycle transitions
  - chat runtime support
  - subscriptions / metrics / generic mailbox delivery

Expose only the minimal crate-private hooks that `JobManager` needs to drive a non-chat runtime lifecycle.

---

### Task 5: Split the external monitor/state protocol

**Files:**
- Modify: `crates/argus-protocol/src/events.rs`
- Modify: `crates/argus-protocol/src/lib.rs`
- Modify: `crates/argus-wing/src/lib.rs`
- Modify: `crates/desktop/src-tauri/src/commands.rs`
- Modify: `crates/desktop/src-tauri/src/events/thread.rs`
- Modify: `crates/desktop/src-tauri/src/lib.rs`
- Modify: `crates/desktop/lib/types/chat.ts`
- Modify: `crates/desktop/lib/tauri.ts`
- Modify: `crates/desktop/lib/chat-store.ts`

**Changes:**

- Make `ThreadPoolState` / `ThreadPoolSnapshot` / `ThreadPoolRuntimeSummary` chat-only.
- Remove `job_id` and job kind from thread-pool lifecycle events.
- Add `JobRuntimeState` / `JobRuntimeSnapshot` / `JobRuntimeSummary`.
- Add `JobRuntimeQueued/Started/Cooling/Evicted/MetricsUpdated`.
- Expose `job_runtime_state()` / `get_job_runtime_state` through wing + Tauri.
- Merge chat/runtime and job runtime data inside desktop store for monitor UI consumption.

---

### Task 6: Switch session/scheduler to explicit job APIs

**Files:**
- Modify: `crates/argus-session/src/manager.rs`

**Changes:**

- Replace all scheduler job lookups that currently call `ThreadPool` job methods with `JobManager` methods.
- Keep `ThreadPool` usage only for:
  - chat runtime loading
  - runtime summary checks
  - generic mailbox delivery
  - event subscription

The scheduler should no longer depend on `ThreadPool` for job graph semantics.

---

### Task 7: Move and update tests with the new source of truth

**Files:**
- Modify: `crates/argus-job/src/job_manager.rs`
- Modify: `crates/argus-job/src/thread_pool.rs`
- Modify: `crates/argus-session/src/manager.rs`
- Modify: desktop bindings/store tests consuming runtime monitor state

**Changes:**

- Job behavior assertions move with `JobManager`.
- `ThreadPoolState` assertions become chat-only.
- Job runtime assertions move to `JobRuntimeState` / `JobRuntime*` events.
- Desktop tests assert the store merges dual sources rather than reading job identity from `ThreadPoolState`.

Record any remaining naming debt in comments or the final report rather than widening the refactor.

---

### Task 8: Verify and report remaining naming follow-up

**Required verification:**

- `cargo test -p argus-protocol -- --nocapture`
- `cargo fmt --all`
- `cargo test -p argus-job -- --nocapture`
- `cargo test -p argus-session -- --nocapture`
- `cargo test -p argus-wing -- --nocapture`
- `node --test tests/chat-tauri-bindings.test.mjs tests/chat-store-session-model.test.mjs`
- `pnpm exec tsx --test tests/chat-store-subagent-job-details.behavior.test.tsx`
- `pnpm exec tsc --noEmit`

**Report must include:**

- changed files
- which responsibilities moved out of `ThreadPool`
- which protocol surfaces split into chat/runtime vs job runtime
- remaining naming follow-up, especially the deferred `ThreadPool` -> `RuntimePool` evaluation
