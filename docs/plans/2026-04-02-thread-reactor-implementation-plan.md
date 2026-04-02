# Thread Reactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Refactor thread execution so `Thread` becomes the single reactor that owns queueing, turn progression, settlement, persistence, and event ordering, while `Turn` becomes a progress-yielding execution state object.

**Architecture:** Fold the logic from `crates/argus-agent/src/runtime.rs` back into `crates/argus-agent/src/thread.rs`, then convert `Turn` from a detached task model into a progress-driven state machine that reads shared thread behavior. Remove the extra task boundary only after sequencing tests prove that `TurnSettled` always precedes `Idle` and that queueing, approval, cancellation, and persistence still behave correctly.

**Tech Stack:** Rust, Tokio, derive_builder, broadcast/mpsc channels, committed turn logs (`messages.jsonl` + `meta.json`), cargo test.

---

### Task 1: Freeze current sequencing with failing tests

**Files:**
- Modify: `crates/argus-agent/src/runtime.rs`
- Modify: `crates/argus-agent/src/thread.rs`
- Test: `crates/argus-agent/src/runtime.rs`
- Test: `crates/argus-agent/src/thread.rs`
- Test: `crates/argus-job/src/thread_pool.rs`

**Step 1: Write the failing sequencing tests**

Add or expand tests that assert:

- completed turns emit `TurnSettled` before `Idle`
- failed turns emit `TurnSettled` before `Idle`
- queued follow-up work does not start before prior settlement
- the job-capacity regression remains covered

**Step 2: Run the focused tests to verify current behavior**

Run: `cargo test -p argus-agent runtime_ --lib`
Expected: Existing runtime tests pass; any new sequencing assertions that expose hidden ordering gaps fail first.

Run: `cargo test -p argus-job execute_job_respects_thread_pool_capacity --lib`
Expected: PASS before the refactor, confirming the regression test is stable.

**Step 3: Commit the test scaffold**

```bash
git add crates/argus-agent/src/runtime.rs crates/argus-agent/src/thread.rs crates/argus-job/src/thread_pool.rs
git commit -m "test(agent): freeze thread reactor sequencing"
```

### Task 2: Introduce a thread-owned reactor loop inside `thread.rs`

**Files:**
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: `crates/argus-agent/src/types.rs`
- Test: `crates/argus-agent/src/thread.rs`

**Step 1: Add the minimal new thread runtime state**

Define or extend thread-local runtime state in `thread.rs` so the thread directly owns:

- inbox / queue state
- current execution phase
- active cancellation handle

Keep existing public methods stable while introducing private helpers that replace `ThreadRuntimeAction`.

**Step 2: Write a focused unit test for idle-to-running-to-idle flow**

Run: `cargo test -p argus-agent thread_reactor_ --lib`
Expected: FAIL first until the new loop is wired.

**Step 3: Implement the thread-owned reactor helpers**

Implement private helpers in `thread.rs` for:

- enqueueing control events
- deciding when the next turn can start
- handling terminal turn outcomes

Do not remove `runtime.rs` yet.

**Step 4: Re-run the focused unit tests**

Run: `cargo test -p argus-agent thread_reactor_ --lib`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-agent/src/thread.rs crates/argus-agent/src/types.rs
git commit -m "refactor(agent): add thread-owned reactor state"
```

### Task 3: Convert `Turn` into a progress-yielding state machine

**Files:**
- Modify: `crates/argus-agent/src/turn.rs`
- Modify: `crates/argus-agent/src/error.rs`
- Test: `crates/argus-agent/src/turn.rs`
- Test: `crates/argus-agent/tests/integration_test.rs`

**Step 1: Define the new progress contract**

Add explicit progress output types, for example terminal and non-terminal progress variants, in `turn.rs`.

**Step 2: Write the failing turn progression tests**

Cover:

- normal completion
- tool progress emission
- approval pause and resume
- cancellation

Run: `cargo test -p argus-agent turn_progress_ --lib`
Expected: FAIL until `Turn` stops assuming detached full execution.

**Step 3: Implement minimal turn progression**

Refactor `Turn::execute()` into a progress-driven API and remove direct settlement responsibilities from `Turn`.

**Step 4: Re-run focused turn tests**

Run: `cargo test -p argus-agent turn_progress_ --lib`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-agent/src/turn.rs crates/argus-agent/src/error.rs crates/argus-agent/tests/integration_test.rs
git commit -m "refactor(agent): make turn execution progress-driven"
```

### Task 4: Replace detached turn spawning with in-thread progression

**Files:**
- Modify: `crates/argus-agent/src/runtime.rs`
- Modify: `crates/argus-agent/src/thread.rs`
- Test: `crates/argus-agent/src/runtime.rs`
- Test: `crates/argus-agent/tests/trace_integration_test.rs`

**Step 1: Write the failing test for no detached turn execution**

Cover the sequence where a queued message arrives while a turn is running and verify progression still completes without relying on `start_turn_task()`.

**Step 2: Run the focused tests**

Run: `cargo test -p argus-agent trace_integration_test --test trace_integration_test`
Expected: FAIL until detached turn spawning is removed.

**Step 3: Implement thread-driven advancement**

Change thread execution so:

- no separate turn task is spawned
- `Thread` advances the active turn in its own loop
- terminal turn outcomes settle and persist in the same loop

**Step 4: Re-run the focused tests**

Run: `cargo test -p argus-agent --test trace_integration_test`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-agent/src/runtime.rs crates/argus-agent/src/thread.rs crates/argus-agent/tests/trace_integration_test.rs
git commit -m "refactor(agent): inline turn progression into thread reactor"
```

### Task 5: Remove thread/turn duplication and obsolete locking

**Files:**
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: `crates/argus-agent/src/turn.rs`
- Modify: `crates/argus-agent/src/history.rs`
- Test: `crates/argus-agent/src/thread.rs`
- Test: `crates/argus-agent/src/turn.rs`

**Step 1: Remove duplicated turn-owned shared state**

Delete or fold back into `Thread`:

- turn-owned shared `messages`
- detached-event forwarding handles
- turn-owned tool and hook copies that now come from thread reads

**Step 2: Write a failing regression test for shared history reads**

Run: `cargo test -p argus-agent shared_history_ --lib`
Expected: FAIL until the duplicated read path is removed.

**Step 3: Implement the minimal cleanup**

Keep `Turn` focused on per-turn execution state and make shared reads come from thread-owned behavior.

**Step 4: Re-run focused tests**

Run: `cargo test -p argus-agent shared_history_ --lib`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-agent/src/thread.rs crates/argus-agent/src/turn.rs crates/argus-agent/src/history.rs
git commit -m "refactor(agent): remove duplicated turn shared state"
```

### Task 6: Delete the obsolete runtime layer

**Files:**
- Modify: `crates/argus-agent/src/lib.rs`
- Delete: `crates/argus-agent/src/runtime.rs`
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: `crates/argus-job/src/thread_pool.rs`
- Modify: `crates/argus-session/src/manager.rs`
- Test: `crates/argus-agent/src/thread.rs`
- Test: `crates/argus-job/src/thread_pool.rs`
- Test: `crates/argus-session/src/manager.rs`

**Step 1: Write failing integration tests around removed runtime entry points**

Cover callers that still rely on `Thread::spawn_runtime_actor()` or runtime-layer behavior.

**Step 2: Run the focused integration tests**

Run: `cargo test -p argus-agent --lib`
Expected: FAIL first where old runtime entry points are still referenced.

**Step 3: Remove runtime-layer APIs and update callers**

Delete `runtime.rs`, remove exports and helpers that only exist for the old actor model, and update session/job code to use the thread-owned reactor entry points.

**Step 4: Re-run package tests**

Run: `cargo test -p argus-agent --lib --test integration_test --test trace_integration_test`
Expected: PASS.

Run: `cargo test -p argus-session --lib`
Expected: PASS.

Run: `cargo test -p argus-job --lib`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-agent/src/lib.rs crates/argus-agent/src/thread.rs crates/argus-agent/src/turn.rs crates/argus-job/src/thread_pool.rs crates/argus-session/src/manager.rs
git rm crates/argus-agent/src/runtime.rs
git commit -m "refactor(agent): remove detached runtime actor layer"
```

### Task 7: Full regression verification

**Files:**
- Modify: `crates/desktop/tests/chat-store-session-model.test.mjs`
- Test: `crates/argus-agent/tests/integration_test.rs`
- Test: `crates/argus-agent/tests/trace_integration_test.rs`
- Test: `crates/argus-job/src/thread_pool.rs`
- Test: `crates/argus-session/src/manager.rs`
- Test: `crates/desktop/tests/chat-store-session-model.test.mjs`

**Step 1: Run the full relevant verification set**

Run: `cargo test -p argus-agent --lib --test integration_test --test trace_integration_test`
Expected: PASS.

Run: `cargo test -p argus-session --lib`
Expected: PASS.

Run: `cargo test -p argus-job --lib`
Expected: PASS.

Run: `node --test tests/chat-store-session-model.test.mjs`
Expected: PASS.

**Step 2: Fix any compatibility regressions minimally**

Only touch files needed to preserve committed-history snapshot behavior and cross-crate event expectations.

**Step 3: Re-run the same verification set**

Expected: all PASS.

**Step 4: Commit**

```bash
git add crates/argus-agent crates/argus-session crates/argus-job crates/desktop/tests/chat-store-session-model.test.mjs
git commit -m "test(agent): verify thread reactor refactor end-to-end"
```
