# Session-Owned Thread Mailbox Routing Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Move thread inbound routing under `Session`, simplify `Thread` to consume its own mailbox directly, and remove `Turn` access to mailbox/control senders while preserving FIFO queueing and pure-stop interrupt behavior.

**Architecture:** Keep outbound event broadcasting unchanged, but make inbound work flow through a session-owned mailbox map keyed by `ThreadId`. Inline the current runtime state machine into `Thread` itself, remove the legacy `ThreadReactor` layer, and update scheduler/job flows to route via session-owned APIs rather than thread-internal `control_tx`.

**Tech Stack:** Rust, Tokio, `broadcast`, `RwLock`, `DashMap`, `argus-agent`, `argus-session`, `argus-job`, `argus-tool`, `argus-protocol`

---

### Task 1: Redefine Mailbox Semantics and Lock Them Down with Tests

**Files:**
- Modify: `crates/argus-protocol/src/events.rs`
- Modify: `crates/argus-agent/tests/integration_test.rs`

**Step 1: Write the failing mailbox tests**

Add tests that describe the target semantics:

```rust
#[test]
fn thread_mailbox_take_next_turn_message_preserves_global_fifo() {
    let mut mailbox = ThreadMailbox::default();
    mailbox.enqueue_user_message("first".to_string(), None);
    mailbox.enqueue_mailbox_message(sample_mailbox_message("job-1"));

    let first = mailbox.take_next_turn_message().unwrap();
    let second = mailbox.take_next_turn_message().unwrap();

    assert_eq!(first.content, "first");
    assert!(second.content.contains("Job: job-1"));
}

#[test]
fn thread_mailbox_interrupt_stop_is_not_enqueued() {
    let mut mailbox = ThreadMailbox::default();
    mailbox.interrupt_stop();

    assert!(mailbox.take_next_turn_message().is_none());
    assert!(mailbox.take_stop_signal());
    assert!(!mailbox.take_stop_signal());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-agent thread_mailbox_ -- --nocapture`

Expected: FAIL because the mailbox API still depends on `ThreadControlEvent` push/drain compatibility behavior.

**Step 3: Write the minimal mailbox implementation**

Refactor `ThreadMailbox` so it directly exposes mailbox operations instead of wrapping legacy control events:

```rust
pub struct ThreadMailbox {
    queued_inputs: VecDeque<QueuedThreadInput>,
    stop_requested: bool,
}

pub enum QueuedThreadInput {
    UserMessage(QueuedUserMessage),
    MailboxMessage(MailboxMessage),
}
```

Add direct methods such as:

- `enqueue_user_message(...)`
- `enqueue_mailbox_message(...)`
- `interrupt_stop()`
- `take_stop_signal()`
- `take_next_turn_message()`
- `claim_job_result(...)`

Remove the compatibility-only `push(ThreadControlEvent)` and `drain_for_turn()` behavior from the primary path.

**Step 4: Run tests to verify the mailbox behavior passes**

Run: `cargo test -p argus-agent thread_mailbox_ -- --nocapture`

Expected: PASS for the new mailbox ordering and stop-signal tests.

**Step 5: Commit**

```bash
git add crates/argus-protocol/src/events.rs crates/argus-agent/tests/integration_test.rs
git commit -m "refactor(protocol): simplify thread mailbox semantics"
```

### Task 2: Move Thread Routing Entry Points into Session-Owned Mailboxes

**Files:**
- Modify: `crates/argus-session/src/session.rs`
- Modify: `crates/argus-session/src/manager.rs`
- Modify: `crates/argus-job/src/thread_pool.rs`
- Modify: `crates/argus-job/src/job_manager.rs`

**Step 1: Write the failing session-routing tests**

Add tests that assert:

- cancelling a thread routes a pure stop signal without sending a queued user message
- sending a mailbox message uses the target thread mailbox owned by the session/runtime layer

Sketch:

```rust
#[tokio::test]
async fn cancel_thread_sets_stop_signal_without_queueing_new_turn_input() {
    // load a session + thread
    // call SessionManager::cancel_thread(...)
    // assert the target mailbox exposes a stop signal
    // assert no next-turn queued message was created
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-session cancel_thread_sets_stop_signal_without_queueing_new_turn_input -- --nocapture`

Expected: FAIL because `cancel_thread` still routes through `send_control_event(ThreadControlEvent::UserInterrupt { ... })`.

**Step 3: Implement session-owned mailbox routing**

Add mailbox ownership directly to the session/session-manager path instead of introducing a new registry type:

```rust
threads: DashMap<ThreadId, Weak<RwLock<Thread>>>,
mailboxes: DashMap<ThreadId, Arc<Mutex<ThreadMailbox>>>,
```

Expose direct routing helpers on `SessionManager`, for example:

- `enqueue_thread_user_message(thread_id, content, msg_override)`
- `enqueue_thread_mailbox_message(thread_id, message)`
- `interrupt_thread(thread_id)`
- `claim_thread_job_result(thread_id, job_id)`

Update:

- `Session::broadcast(...)`
- `SessionManager::send_message(...)`
- `SessionManager::cancel_thread(...)`
- `ThreadPool::deliver_mailbox_message(...)`
- `JobManager::forward_job_result_to_runtime(...)`

so they call session-owned mailbox methods instead of thread-internal `control_tx`.

**Step 4: Run the routing tests**

Run: `cargo test -p argus-session -- --nocapture`

Expected: PASS for session-level routing behavior.

**Step 5: Commit**

```bash
git add crates/argus-session/src/session.rs crates/argus-session/src/manager.rs crates/argus-job/src/thread_pool.rs crates/argus-job/src/job_manager.rs
git commit -m "refactor(session): route thread inputs through session-owned mailboxes"
```

### Task 3: Inline Runtime State Machine into Thread and Remove ThreadReactor

**Files:**
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: `crates/argus-agent/src/thread_handle.rs`
- Modify: `crates/argus-agent/src/lib.rs`
- Modify: `crates/argus-agent/tests/integration_test.rs`

**Step 1: Write the failing thread runtime tests**

Add tests around direct thread runtime behavior:

```rust
#[tokio::test]
async fn queued_follow_up_runs_after_cancelled_turn_settles() {
    // start a turn
    // enqueue a follow-up while running
    // issue interrupt_stop
    // assert the running turn is cancelled
    // assert the follow-up starts next in FIFO order
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-agent queued_follow_up_runs_after_cancelled_turn_settles -- --nocapture`

Expected: FAIL because the runtime loop still depends on `control_rx` plus `ThreadReactor`.

**Step 3: Inline the reactor state into `Thread`**

Move the current fields and decisions into `Thread` directly:

```rust
runtime_state: ThreadRuntimeState,
next_turn_number: u32,
queue_depth: usize,
active_turn_cancellation: Option<TurnCancellation>,
```

Replace `ThreadReactor` / `ThreadReactorAction` with direct `Thread::run_loop()` decisions:

- if idle and mailbox has queued work, start next turn
- if running and mailbox has stop signal, cancel active turn
- after settlement, dequeue next FIFO item if present

Delete or reduce `thread_handle.rs` so it no longer wraps a separate runtime state machine.

**Step 4: Run thread runtime tests**

Run: `cargo test -p argus-agent -- --nocapture`

Expected: PASS for queueing, cancellation, and turn-settlement behavior.

**Step 5: Commit**

```bash
git add crates/argus-agent/src/thread.rs crates/argus-agent/src/thread_handle.rs crates/argus-agent/src/lib.rs crates/argus-agent/tests/integration_test.rs
git commit -m "refactor(agent): inline thread mailbox runtime into thread"
```

### Task 4: Remove Mailbox and Control Sender from Turn and Tool Context

**Files:**
- Modify: `crates/argus-agent/src/turn.rs`
- Modify: `crates/argus-protocol/src/tool.rs`
- Modify: `crates/argus-tool/src/scheduler.rs`
- Modify: `crates/argus-tool/tests/tool_execution_context_compat.rs`
- Modify: `crates/argus-agent/tests/integration_test.rs`

**Step 1: Write the failing turn/tool-context tests**

Add tests that describe the new API surface:

```rust
#[test]
fn tool_execution_context_contains_thread_identity_but_not_control_sender() {
    let _ctx = ToolExecutionContext {
        thread_id: ThreadId::new(),
        agent_id: None,
        pipe_tx,
    };
}
```

Add/adjust the nested dispatch test so it still proves the originating thread ID survives tool execution, without constructing `Turn` with `control_tx` or `mailbox`.

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-tool tool_execution_context_compat -- --nocapture`

Expected: FAIL because `ToolExecutionContext` and `TurnBuilder` still require the old fields.

**Step 3: Implement the minimal API cleanup**

Remove from `Turn`:

- `control_tx`
- `mailbox`

Keep:

- `stream_tx`
- `thread_event_tx`
- `cancellation`

Reduce `ToolExecutionContext` to the data the tool actually needs:

```rust
pub struct ToolExecutionContext {
    pub thread_id: ThreadId,
    pub agent_id: Option<AgentId>,
    pub pipe_tx: broadcast::Sender<ThreadEvent>,
}
```

Update scheduler request types so backend routing relies on the backend/session layer instead of copying `control_tx` through tool execution.

**Step 4: Run the turn and tool tests**

Run:

- `cargo test -p argus-tool -- --nocapture`
- `cargo test -p argus-agent tool_execution_context_uses_originating_thread_id_for_nested_dispatch -- --nocapture`

Expected: PASS with the reduced `Turn` and `ToolExecutionContext` API.

**Step 5: Commit**

```bash
git add crates/argus-agent/src/turn.rs crates/argus-protocol/src/tool.rs crates/argus-tool/src/scheduler.rs crates/argus-tool/tests/tool_execution_context_compat.rs crates/argus-agent/tests/integration_test.rs
git commit -m "refactor(agent): remove turn mailbox and control sender"
```

### Task 5: Remove Legacy Compatibility Paths and Verify End-to-End Behavior

**Files:**
- Modify: `crates/argus-protocol/src/lib.rs`
- Modify: `crates/argus-protocol/src/events.rs`
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: `crates/argus-session/src/manager.rs`
- Modify: `crates/argus-job/src/job_manager.rs`

**Step 1: Write the final regression tests**

Add regression coverage for:

- stop while idle is a no-op
- queued mailbox messages remain unread until explicitly marked read
- consuming one job result does not reorder remaining queued items

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-agent -- --nocapture`

Expected: FAIL while dead compatibility paths still exist or behavior is still split.

**Step 3: Delete the old compatibility entry points**

Remove or shrink:

- `ThreadControlEvent` as a public routing surface
- leftover mailbox `push(...)` / legacy interrupt drain behavior
- `Thread.send_user_message(...)`
- `Thread.send_control_event(...)`

Keep only the pieces still required for outbound events and persisted runtime state.

**Step 4: Run the full verification suite**

Run:

- `cargo test -p argus-protocol -- --nocapture`
- `cargo test -p argus-agent -- --nocapture`
- `cargo test -p argus-session -- --nocapture`
- `cargo test -p argus-job -- --nocapture`
- `cargo test -p argus-tool scheduler -- --nocapture`

Expected: PASS across the touched crates, with mailbox routing and interrupt semantics verified end to end.

**Step 5: Commit**

```bash
git add crates/argus-protocol/src/lib.rs crates/argus-protocol/src/events.rs crates/argus-agent/src/thread.rs crates/argus-session/src/manager.rs crates/argus-job/src/job_manager.rs
git commit -m "refactor(session): finish unified thread mailbox routing"
```
