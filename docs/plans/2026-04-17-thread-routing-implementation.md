# Thread Routing Redesign Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove `ThreadMailbox` and make `Thread::send_message(ThreadMessage)` the only external ingress for thread routing.

**Architecture:** Replace the shared mailbox/control model with a single thread-owned message ingress plus a private pending queue inside `Thread`. Move all job-result consume semantics back to `JobManager`, and delete inbox-related APIs from session/job/scheduler layers.

**Tech Stack:** Rust, `argus-protocol`, `argus-agent`, `argus-session`, `argus-job`, `argus-tool`, Tokio `mpsc`

---

### Task 1: Replace mailbox protocol types with `ThreadMessage`

**Files:**
- Modify: `crates/argus-protocol/src/events.rs`
- Test: `crates/argus-protocol/src/events.rs`

**Step 1: Write the failing protocol tests**

Add tests in `crates/argus-protocol/src/events.rs` that assert:

- `ThreadMessage::Interrupt` is not part of normal FIFO payload flow
- `ThreadMessage::UserInput`, `PeerMessage`, and `JobResult` can be ordered by arrival
- `ThreadMailbox`-specific tests are removed or replaced

Use a test skeleton like:

```rust
#[test]
fn thread_message_routes_fifo_payloads() {
    let messages = vec![
        ThreadMessage::UserInput { content: "a".into(), msg_override: None },
        ThreadMessage::PeerMessage { message: plain_mailbox_message("b") },
        ThreadMessage::JobResult { message: job_result_message("job-1") },
    ];

    assert_eq!(messages.len(), 3);
}
```

**Step 2: Run the targeted test to verify failure**

Run: `cargo test -p argus-protocol thread_message --lib`
Expected: FAIL because `ThreadMessage` does not exist yet

**Step 3: Replace the protocol types**

In `crates/argus-protocol/src/events.rs`:

- delete `ThreadCommand`
- delete `ThreadMailboxItem`
- delete `ThreadMailbox`
- add `ThreadControlMessage`
- add `ThreadMessage`

Keep `MailboxMessage` as the payload record used by `PeerMessage` and `JobResult`.

**Step 4: Run the protocol test again**

Run: `cargo test -p argus-protocol thread_message --lib`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-protocol/src/events.rs
git commit -m "Refactor thread ingress around ThreadMessage"
```

---

### Task 2: Refactor `Thread` to own routing and private pending state

**Files:**
- Modify: `crates/argus-agent/src/thread.rs`
- Test: `crates/argus-agent/src/thread.rs`

**Step 1: Write the failing runtime tests**

Add or rewrite tests in `crates/argus-agent/src/thread.rs` for:

- multiple `send_message(ThreadMessage::UserInput)` calls preserve FIFO
- `Interrupt` cancels only the active turn
- queued payload messages continue after a turn settles

Use concrete tests similar to:

```rust
#[tokio::test]
async fn send_message_queues_next_payload_while_running() {
    let mut thread = build_test_thread_without_system_prompt();
    thread.send_message(ThreadMessage::UserInput {
        content: "first".into(),
        msg_override: None,
    }).await.unwrap();
    thread.send_message(ThreadMessage::UserInput {
        content: "second".into(),
        msg_override: None,
    }).await.unwrap();

    assert!(thread.is_turn_running());
}
```

**Step 2: Run the targeted tests to verify failure**

Run: `cargo test -p argus-agent send_message_queues --lib`
Expected: FAIL because `Thread::send_message` does not exist yet

**Step 3: Replace mailbox/control plumbing**

In `crates/argus-agent/src/thread.rs`:

- remove `mailbox: Arc<Mutex<ThreadMailbox>>`
- remove `control_tx/control_rx` mailbox wake model
- add a thread-owned Tokio `mpsc` ingress for `ThreadMessage`
- add private `pending_messages: VecDeque<ThreadMessage>`
- implement `Thread::send_message(ThreadMessage)`
- update the runtime loop to consume `ThreadMessage`
- route `Interrupt` outside the normal FIFO payload path

Do not reintroduce any external mailbox-like accessor.

**Step 4: Run targeted runtime tests**

Run: `cargo test -p argus-agent thread_runtime --lib`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-agent/src/thread.rs
git commit -m "Let Thread own message routing and pending state"
```

---

### Task 3: Route session and thread-pool writes through `Thread::send_message`

**Files:**
- Modify: `crates/argus-session/src/session.rs`
- Modify: `crates/argus-session/src/manager.rs`
- Modify: `crates/argus-job/src/thread_pool.rs`
- Test: `crates/argus-session/src/manager.rs`

**Step 1: Write failing orchestration tests**

Update or add tests covering:

- sending a user message reaches the target thread via `send_message`
- delivering a mailbox message routes as `ThreadMessage::PeerMessage`
- interrupt uses `ThreadMessage::Interrupt`

**Step 2: Run a targeted test**

Run: `cargo test -p argus-session send_message_wakes_existing_runtime_loop --lib`
Expected: FAIL because mailbox accessors are still wired in

**Step 3: Remove mailbox access from orchestration**

In these files:

- delete `Session::mailbox()`
- replace `enqueue_user_message` internals with `thread.send_message(ThreadMessage::UserInput { ... })`
- replace `enqueue_mailbox_message` internals with `thread.send_message(ThreadMessage::PeerMessage { ... })`
- replace interrupt internals with `thread.send_message(ThreadMessage::Interrupt)`
- remove `Session::claim_job_result()`
- remove mailbox access from `ThreadPool`

If a caller still needs job-result consume behavior, keep it in `JobManager`; do not route it back through `Thread`.

**Step 4: Run orchestration tests**

Run: `cargo test -p argus-session send_message_wakes_existing_runtime_loop --lib`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-session/src/session.rs crates/argus-session/src/manager.rs crates/argus-job/src/thread_pool.rs
git commit -m "Route session and thread-pool writes through Thread::send_message"
```

---

### Task 4: Delete inbox-facing scheduler APIs

**Files:**
- Modify: `crates/argus-tool/src/scheduler.rs`
- Modify: `crates/argus-session/src/manager.rs`
- Test: `crates/argus-tool/src/scheduler.rs`

**Step 1: Write failing scheduler tests**

Update tests so the scheduler surface only expects messaging/job APIs, not inbox APIs.

Specifically remove assumptions around:

- `CheckInboxRequest`
- `MarkReadRequest`
- `SchedulerBackend::check_inbox`
- `SchedulerBackend::mark_read`

**Step 2: Run a targeted scheduler test**

Run: `cargo test -p argus-tool scheduler --lib`
Expected: FAIL until the inbox API surface is removed

**Step 3: Delete the inbox API surface**

In `crates/argus-tool/src/scheduler.rs`:

- delete `CheckInboxRequest`
- delete `MarkReadRequest`
- remove `check_inbox` / `mark_read` from `SchedulerBackend`
- remove `SchedulerInput::CheckInbox`
- remove `SchedulerInput::MarkRead`

In `crates/argus-session/src/manager.rs`:

- remove the corresponding backend implementations

**Step 4: Run scheduler tests**

Run: `cargo test -p argus-tool scheduler --lib`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-tool/src/scheduler.rs crates/argus-session/src/manager.rs
git commit -m "Remove inbox-oriented scheduler APIs from thread routing"
```

---

### Task 5: Verify the full refactor

**Files:**
- Modify: any touched files above if verification reveals issues

**Step 1: Run formatting**

Run: `cargo fmt`
Expected: succeeds with no remaining diff after re-run

**Step 2: Run targeted crate tests**

Run: `cargo test -p argus-protocol`
Expected: PASS

Run: `cargo test -p argus-agent`
Expected: PASS

Run: `cargo test -p argus-session`
Expected: PASS

Run: `cargo test -p argus-tool`
Expected: PASS

**Step 3: Run workspace checks if affordable**

Run: `prek`
Expected: PASS

**Step 4: Final commit**

```bash
git add -u
git commit -m "Finish thread routing simplification and mailbox removal"
```

---

Plan complete and saved to `docs/plans/2026-04-17-thread-routing-implementation.md`. Two execution options:

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

Which approach?
