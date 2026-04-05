# Turn/Thread Collapse Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Collapse duplicated Turn ownership back into Thread so `Vec<TurnRecord>` remains the single history authority and `turn.rs` keeps the turn execution logic without a second full `Turn` object model.

**Architecture:** Keep `Thread` as the only runtime/lifecycle owner, and move turn execution to `turn.rs` module-level helpers or a minimal private seam instead of `TurnBuilder` / `TurnExecution` / `Thread::build_turn()`. Preserve turn semantics: one successful turn yields one `TurnRecord`, turn-level compact yields `TurnCheckpoint`, and failed or cancelled turns persist nothing.

**Tech Stack:** Rust workspace, Tokio async runtime, `derive_builder` cleanup, `cargo test`

---

### Task 1: Lock the current turn semantics with regression tests

**Files:**
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: `crates/argus-agent/src/turn.rs`
- Modify: `crates/argus-agent/tests/integration_test.rs`

**Step 1: Add or rewrite thread-facing regression tests**

Add tests that assert outcomes in terms of committed `Vec<TurnRecord>`, not in terms of the current builder API. At minimum cover:

```rust
assert!(matches!(record.kind, TurnRecordKind::UserTurn));
assert!(matches!(record.kind, TurnRecordKind::TurnCheckpoint));
assert_eq!(thread.turn_count(), 1);
assert_eq!(thread.token_count(), expected_total_tokens);
```

**Step 2: Add a cancellation regression**

Add or adjust a test so cancellation proves no committed record is appended:

```rust
let result = thread.finish_turn(Err(ThreadError::TurnFailed(TurnError::Cancelled)));
assert!(result.is_ok());
assert!(thread.history_iter().next().is_none());
assert_eq!(thread.turn_count(), 0);
```

**Step 3: Run targeted tests**

Run:

```bash
cargo test -p argus-agent thread::tests
cargo test -p argus-agent turn::tests
```

Expected: the current behavior is locked before structural cleanup starts.

**Step 4: Commit the regression baseline**

```bash
git add crates/argus-agent/src/thread.rs crates/argus-agent/src/turn.rs crates/argus-agent/tests/integration_test.rs
git commit -m "Lock turn/thread persistence semantics before collapsing wrappers"
```

### Task 2: Extract turn execution out of the owning `Turn` object shape

**Files:**
- Modify: `crates/argus-agent/src/turn.rs`
- Modify: `crates/argus-agent/src/error.rs`

**Step 1: Write the failing compile change**

Start by moving pure execution helpers off `impl Turn` and into module-private functions in `turn.rs`. Keep `TurnCancellation`, but stop assuming a full owning `Turn` struct is required for:

- prompt materialization
- request construction
- finish-reason processing
- tool execution
- turn-level compaction

**Step 2: Preserve the existing logic with explicit inputs**

Refactor helpers to take the real inputs they need instead of `self`, for example:

```rust
fn materialize_messages(
    system_prompt: Option<&str>,
    history: &[ChatMessage],
    turn_messages: &[ChatMessage],
) -> Vec<ChatMessage>
```

and

```rust
async fn execute_loop(
    thread_id: &str,
    turn_number: u32,
    /* explicit thread-owned dependencies */
) -> Result<TurnRecord, TurnError>
```

Do not add new domain structs like `TurnFrame` or `TurnRunner`.

**Step 3: Remove now-obsolete builder-specific error paths if possible**

If `TurnBuilder` is no longer needed, collapse or delete `Turn build failed`-only branches that exist solely for builder construction errors.

**Step 4: Run targeted tests**

Run:

```bash
cargo test -p argus-agent turn::tests
```

Expected: the turn loop still passes while no longer depending on a heavyweight owning `Turn` object shape.

**Step 5: Commit the extraction**

```bash
git add crates/argus-agent/src/turn.rs crates/argus-agent/src/error.rs
git commit -m "Extract turn execution logic away from the owning Turn wrapper"
```

### Task 3: Make the thread reactor execute turns directly

**Files:**
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: `crates/argus-agent/src/turn.rs`

**Step 1: Remove `TurnBuilder` / `TurnExecution` from the reactor path**

Delete usage of:

- `Thread::build_turn()`
- `TurnBuilder`
- `TurnExecution`
- `start_turn_execution()` returning a wrapper handle

Replace them with a reactor-owned in-flight turn future driven directly from `thread.rs`.

**Step 2: Keep the state source inside `Thread`**

Before starting a turn, have `Thread` directly:

- derive `turn_number` from `self.turns`
- build context from `self.turns`
- run thread-level compact
- set `self.active_turn_cancellation`

Then call the `turn.rs` execution entrypoint and wait for `Result<TurnRecord, TurnError>`.

**Step 3: Preserve cancellation and settle semantics**

Keep stop requests flowing through `TurnCancellation`, and keep settlement rules unchanged:

```rust
Ok(record) => self.turns.push(record),
Err(ThreadError::TurnFailed(TurnError::Cancelled)) => Ok(()),
Err(error) => Err(error),
```

Do not introduce new result-shell types.

**Step 4: Run targeted tests**

Run:

```bash
cargo test -p argus-agent thread::tests
cargo test -p argus-agent --test integration_test
```

Expected: reactor scheduling, cancellation, and committed history still behave the same after the wrapper chain is removed.

**Step 5: Commit the reactor collapse**

```bash
git add crates/argus-agent/src/thread.rs crates/argus-agent/src/turn.rs
git commit -m "Let Thread drive turn execution directly"
```

### Task 4: Remove the public low-level Turn builder surface and update callers

**Files:**
- Modify: `crates/argus-agent/src/lib.rs`
- Modify: `crates/argus-agent/src/bin/turn.rs`
- Modify: `crates/argus-job/src/bin/smoke-chat.rs`
- Modify: `crates/argus-agent/src/tool_context.rs`

**Step 1: Remove stale public exports and docs**

Update `crates/argus-agent/src/lib.rs` so it no longer documents or re-exports `Turn` / `TurnBuilder` as the public low-level API if that API is gone.

**Step 2: Rewrite internal binaries around thread-owned execution**

For `crates/argus-agent/src/bin/turn.rs` and `crates/argus-job/src/bin/smoke-chat.rs`, replace direct builder usage with a one-thread / one-message execution path using `ThreadBuilder`.

Example target shape:

```rust
let mut thread = ThreadBuilder::new()
    .provider(provider)
    .compactor(compactor)
    .agent_record(agent_record)
    .session_id(session_id)
    .build()?;
```

Then send one user message and inspect the resulting committed record.

**Step 3: Clean up comments that point at deleted methods**

Update comments such as `Turn::execute_single_tool` references in `tool_context.rs` so they reference the surviving helper or generic turn execution path.

**Step 4: Run caller-facing verification**

Run:

```bash
cargo test -p argus-agent
cargo test -p argus-job --bin smoke-chat
```

Expected: no downstream code still assumes `TurnBuilder` exists.

**Step 5: Commit the public-surface cleanup**

```bash
git add crates/argus-agent/src/lib.rs crates/argus-agent/src/bin/turn.rs crates/argus-agent/src/tool_context.rs crates/argus-job/src/bin/smoke-chat.rs
git commit -m "Remove the public Turn builder surface after thread collapse"
```

### Task 5: Final cleanup and end-to-end verification

**Files:**
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: `crates/argus-agent/src/turn.rs`
- Modify: any remaining files reported by `rg` / compiler diagnostics

**Step 1: Delete dead code and imports**

Run:

```bash
rg -n "TurnBuilder|TurnExecution|build_turn\\(|begin_turn_with_number|execute_progress\\(" crates/argus-agent crates/argus-job
```

Delete any remaining dead imports, comments, tests, and helper branches that only existed to support the removed wrapper layer.

**Step 2: Run formatting and full verification**

Run:

```bash
cargo fmt --all
cargo test -p argus-agent
cargo test -p argus-job
```

If the workspace impact grows beyond these crates, run:

```bash
cargo test
```

**Step 3: Review the final diff for accidental abstraction creep**

Check that the implementation did not sneak in replacement shells such as:

- `TurnFrame`
- `TurnRunner`
- `TurnSettled`
- cached turn metadata derivable from `Vec<TurnRecord>`

**Step 4: Commit the cleanup pass**

```bash
git add crates/argus-agent crates/argus-job
git commit -m "Finish collapsing turn ownership into thread"
```
