# ThreadRuntime Refactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Move thread runtime ownership from `argus-job` into `argus-agent` by introducing `ThreadRuntime`, while keeping job dispatch and job lifecycle in `argus-job`.

**Architecture:** Add a thread-centric runtime service in `argus-agent`, migrate `argus-session` to use it for chat-thread lifecycle and subscriptions, then make `argus-job` compose that service instead of owning the runtime implementation. Keep the migration test-first and preserve the current trace-layout and recovery semantics.

**Tech Stack:** Rust, Tokio, broadcast channels, workspace crates `argus-agent`, `argus-job`, `argus-session`, `argus-wing`

---

### Task 1: Add `ThreadRuntime` to `argus-agent`

**Files:**
- Create: `crates/argus-agent/src/thread_runtime.rs`
- Modify: `crates/argus-agent/src/lib.rs`
- Modify: `crates/argus-agent/CLAUDE.md`
- Test: `crates/argus-agent/src/thread_runtime.rs`

**Step 1: Write the failing test**

Add unit tests in `crates/argus-agent/src/thread_runtime.rs` for:

```rust
#[tokio::test]
async fn register_thread_exposes_subscription_and_summary() {
    let runtime = ThreadRuntime::new(/* deps */);
    let thread_id = ThreadId::new();

    runtime.register_thread(ThreadRegistration {
        thread_id,
        kind: ThreadTraceKind::ChatRoot,
        parent_thread_id: None,
        job_id: None,
        root_session_id: Some(SessionId::new()),
        /* ... */
    });

    assert!(runtime.subscribe(&thread_id).is_some());
    assert!(runtime.runtime_summary(&thread_id).is_some());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-agent register_thread_exposes_subscription_and_summary -- --exact`
Expected: FAIL with errors such as `cannot find type 'ThreadRuntime'`.

**Step 3: Write minimal implementation**

Create `ThreadRuntime` with the smallest public surface needed by the test:

```rust
pub struct ThreadRuntime {
    store: Arc<Mutex<ThreadRuntimeStore>>,
}

impl ThreadRuntime {
    pub fn new(/* existing deps from ThreadPool runtime side */) -> Self { /* ... */ }

    pub fn register_thread(&self, registration: ThreadRegistration) {
        /* move runtime registration logic here */
    }

    pub fn subscribe(&self, thread_id: &ThreadId) -> Option<broadcast::Receiver<ThreadEvent>> {
        /* ... */
    }
}
```

Also re-export `ThreadRuntime` from `crates/argus-agent/src/lib.rs` and document that parent/child maps are runtime caches only.

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-agent register_thread_exposes_subscription_and_summary -- --exact`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-agent/src/thread_runtime.rs crates/argus-agent/src/lib.rs crates/argus-agent/CLAUDE.md
git commit -m "refactor(agent): add ThreadRuntime"
```

### Task 2: Move chat-thread runtime usage in `argus-session`

**Files:**
- Modify: `crates/argus-session/src/manager.rs`
- Modify: `crates/argus-session/src/lib.rs`
- Modify: `crates/argus-session/CLAUDE.md`
- Test: `crates/argus-session/src/manager.rs`

**Step 1: Write the failing test**

Add or update a `SessionManager` test that proves chat-thread subscription and cleanup use `ThreadRuntime`:

```rust
#[tokio::test]
async fn subscribe_registers_chat_thread_via_thread_runtime() {
    let manager = test_session_manager();
    let thread_id = manager.create_thread_for_test().await;

    let receiver = manager.subscribe(session_id, &thread_id).await;

    assert!(receiver.is_some());
    assert!(manager.thread_runtime().runtime_summary(&thread_id).is_some());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-session subscribe_registers_chat_thread_via_thread_runtime -- --exact`
Expected: FAIL because `SessionManager` still depends on `ThreadPool`.

**Step 3: Write minimal implementation**

Replace direct `ThreadPool` ownership with `Arc<ThreadRuntime>` for chat-thread paths:

```rust
pub struct SessionManager {
    thread_runtime: Arc<ThreadRuntime>,
    job_manager: Arc<JobManager>,
    // ...
}

fn thread_runtime(&self) -> Arc<ThreadRuntime> {
    Arc::clone(&self.thread_runtime)
}
```

Update `register_chat_thread`, `loaded_chat_thread`, `remove_runtime`, and `subscribe` call sites to use `ThreadRuntime`.

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-session subscribe_registers_chat_thread_via_thread_runtime -- --exact`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-session/src/manager.rs crates/argus-session/src/lib.rs crates/argus-session/CLAUDE.md
git commit -m "refactor(session): use ThreadRuntime for chat threads"
```

### Task 3: Make `argus-job` compose `ThreadRuntime`

**Files:**
- Modify: `crates/argus-job/src/job_manager.rs`
- Modify: `crates/argus-job/src/thread_pool.rs`
- Modify: `crates/argus-job/src/lib.rs`
- Modify: `crates/argus-job/CLAUDE.md`
- Test: `crates/argus-job/src/job_manager.rs`

**Step 1: Write the failing test**

Add a regression test proving dispatch still binds a job to a thread after the runtime split:

```rust
#[tokio::test]
async fn dispatch_job_registers_thread_via_thread_runtime_and_persists_binding() {
    let manager = test_job_manager();

    let thread_id = manager.dispatch_for_test(/* ... */).await.unwrap();

    assert_eq!(manager.thread_binding("job-1"), Some(thread_id));
    assert!(manager.thread_runtime().runtime_summary(&thread_id).is_some());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-job dispatch_job_registers_thread_via_thread_runtime_and_persists_binding -- --exact`
Expected: FAIL because `JobManager` still builds and exposes `ThreadPool`.

**Step 3: Write minimal implementation**

Keep job dispatch in `JobManager`, but inject and use `ThreadRuntime` for runtime creation:

```rust
pub struct JobManager {
    thread_runtime: Arc<ThreadRuntime>,
    thread_pool: Arc<ThreadPool>,
    // tracked jobs...
}

pub fn thread_runtime(&self) -> Arc<ThreadRuntime> {
    Arc::clone(&self.thread_runtime)
}
```

Reduce `ThreadPool` so it contains only job-specific logic and delegates runtime registration/recovery to `ThreadRuntime`.

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-job dispatch_job_registers_thread_via_thread_runtime_and_persists_binding -- --exact`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-job/src/job_manager.rs crates/argus-job/src/thread_pool.rs crates/argus-job/src/lib.rs crates/argus-job/CLAUDE.md
git commit -m "refactor(job): split ThreadRuntime from dispatch"
```

### Task 4: Update `argus-wing` wiring and observer APIs

**Files:**
- Modify: `crates/argus-wing/src/lib.rs`
- Modify: `crates/argus-protocol/src/events.rs`
- Test: `crates/argus-wing/src/lib.rs`

**Step 1: Write the failing test**

Add or update an observer-facing test that verifies thread state still includes loaded chat runtimes after the wiring change:

```rust
#[tokio::test]
async fn delete_thread_removes_chat_runtime_from_thread_runtime_state() {
    let wing = test_wing().await;
    let thread_id = wing.create_thread(/* ... */).await.unwrap();

    let before_delete = wing.thread_runtime_state();
    assert!(before_delete.runtimes.iter().any(|runtime| runtime.thread_id == thread_id));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-wing delete_thread_removes_chat_runtime_from_thread_runtime_state -- --exact`
Expected: FAIL because `ArgusWing` still exposes thread-pool-oriented state only.

**Step 3: Write minimal implementation**

Wire `SessionManager::new` with `job_manager.thread_runtime()` and rename observer APIs only if the rename helps clarity:

```rust
let session_manager = Arc::new(SessionManager::new(
    /* ... */
    job_manager.thread_runtime(),
    job_manager.clone(),
));
```

If external naming must stay stable for now, keep `thread_pool_state()` as a compatibility wrapper and mark it for follow-up cleanup.

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-wing delete_thread_removes_chat_runtime_from_thread_runtime_state -- --exact`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-wing/src/lib.rs crates/argus-protocol/src/events.rs
git commit -m "refactor(wing): wire ThreadRuntime through app"
```

### Task 5: Full verification and cleanup

**Files:**
- Modify: `crates/argus-agent/src/thread_runtime.rs`
- Modify: `crates/argus-job/src/thread_pool.rs`
- Modify: `crates/argus-session/src/manager.rs`
- Modify: `crates/argus-wing/src/lib.rs`
- Test: workspace integration tests touched above

**Step 1: Write the final regression assertions**

Add or tighten tests around:

```rust
assert_eq!(runtime.recover_parent_thread_id(&child_id).await?, Some(parent_id));
assert!(session_manager.subscribe(session_id, &thread_id).await.is_some());
assert_eq!(job_manager.thread_binding("job-1"), Some(child_id));
```

**Step 2: Run targeted verification**

Run:

```bash
cargo test -p argus-agent
cargo test -p argus-session
cargo test -p argus-job
cargo test -p argus-wing
```

Expected: PASS in all four crates.

**Step 3: Run formatting**

Run:

```bash
cargo fmt --all
```

Expected: no diff afterward, or only formatting changes that are staged intentionally.

**Step 4: Re-run the most relevant workspace tests**

Run:

```bash
cargo test -p argus-agent -p argus-session -p argus-job -p argus-wing
```

Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-agent crates/argus-job crates/argus-session crates/argus-wing
git commit -m "refactor: move thread runtime ownership into argus-agent"
```
