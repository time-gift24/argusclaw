# Turn Compaction Redesign Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Split thread-level and turn-level compaction into separate implementations, then add transactional mid-turn compaction that settles ordered checkpoints before the final `UserTurn` only when a turn succeeds.

**Architecture:** Keep the current thread checkpoint flow but rename it into an explicit thread compactor path under `compact/thread.rs`. Add a new turn compactor under `compact/turn.rs`, return a `TurnSettlement` instead of a bare `TurnRecord`, and teach `Thread` to atomically append `Checkpoint(0)... + UserTurn(n)` after successful turn execution. Turn compaction must recompute directly from `self.history + turn_messages` before each LLM request and must never persist `system_prompt`.

**Tech Stack:** Rust, Tokio, derive_builder, existing `argus-agent` turn/thread runtime, `argus-job` thread pool wiring, existing unit/integration tests.

---

### Task 1: Split `compact.rs` into explicit thread and turn modules

**Files:**
- Create: `crates/argus-agent/src/compact/mod.rs`
- Create: `crates/argus-agent/src/compact/thread.rs`
- Create: `crates/argus-agent/src/compact/turn.rs`
- Delete: `crates/argus-agent/src/compact.rs`
- Modify: `crates/argus-agent/src/lib.rs`
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: `crates/argus-agent/src/turn.rs`
- Modify: `crates/argus-job/src/thread_pool.rs`

**Step 1: Move the existing thread compactor code behind renamed types**

Move the current `compact.rs` behavior into `compact/thread.rs` and rename the public API:

```rust
pub struct ThreadCompactResult {
    pub summary_messages: Vec<ChatMessage>,
    pub token_usage: TokenUsage,
}

#[async_trait]
pub trait ThreadCompactor: Send + Sync {
    async fn compact(
        &self,
        messages: &[ChatMessage],
        token_count: u32,
    ) -> Result<Option<ThreadCompactResult>, CompactError>;

    fn name(&self) -> &'static str;
}

pub struct LlmThreadCompactor {
    provider: Arc<dyn LlmProvider>,
    threshold_ratio: f32,
}
```

**Step 2: Define the empty turn-side API shell**

Create `compact/turn.rs` with the new type names only. Do not wire behavior yet.

```rust
pub struct TurnCompactResult {
    pub checkpoint_messages: Vec<ChatMessage>,
}

#[async_trait]
pub trait TurnCompactor: Send + Sync {
    async fn compact(
        &self,
        system_prompt: &str,
        history: &[ChatMessage],
        turn_messages: &[ChatMessage],
    ) -> Result<Option<TurnCompactResult>, CompactError>;

    fn name(&self) -> &'static str;
}
```

**Step 3: Update exports and imports**

Update `lib.rs`, `thread.rs`, `turn.rs`, and `argus-job/src/thread_pool.rs` to import the renamed thread compactor types from `compact::thread`.

Run: `cargo test -p argus-agent compact --quiet`
Expected: compile or test failures only from unfinished turn-compactor wiring, not from missing module paths or unresolved thread compactor symbols.

**Step 4: Make the tree compile again**

Finish import fixes until module boundaries compile cleanly.

Run: `cargo test -p argus-agent compact --quiet`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-agent/src/compact crates/argus-agent/src/lib.rs crates/argus-agent/src/thread.rs crates/argus-agent/src/turn.rs crates/argus-job/src/thread_pool.rs
git commit -m "Separate thread and turn compaction modules"
```

### Task 2: Introduce `TurnSettlement` and thread-side settlement plumbing

**Files:**
- Modify: `crates/argus-agent/src/turn.rs`
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: `crates/argus-agent/tests/integration_test.rs`

**Step 1: Write failing tests for settlement shape**

Add unit coverage in `crates/argus-agent/src/turn.rs` and thread runtime coverage in `crates/argus-agent/src/thread.rs` for:

- a successful turn with no turn compaction returns zero checkpoints plus one final user turn
- `Thread::finish_turn` appends all settlement records in order

Test skeleton:

```rust
assert!(settlement.checkpoints.is_empty());
assert_eq!(settlement.user_turn.kind, TurnRecordKind::UserTurn);
assert_eq!(thread.turns().len(), 2);
assert!(matches!(thread.turns()[0].kind, TurnRecordKind::Checkpoint));
assert!(matches!(thread.turns()[1].kind, TurnRecordKind::UserTurn));
```

Run: `cargo test -p argus-agent turn::tests:: --quiet`
Expected: FAIL because `TurnExecution` and `Thread::finish_turn` still return `TurnRecord`

**Step 2: Replace the bare `TurnRecord` return type**

Add a new settlement container in `crates/argus-agent/src/turn.rs`:

```rust
pub struct TurnSettlement {
    pub checkpoints: Vec<TurnRecord>,
    pub user_turn: TurnRecord,
}
```

Update:

- `TurnExecution.result_rx`
- `CollectedTurnExecution.result`
- `TurnExecution::finish`
- `TurnExecution::collect`
- `Turn::execute_internal`
- `Turn::execute`
- `Thread::settle_active_turn`
- `Thread::finish_turn`

**Step 3: Append settlements atomically in `Thread`**

`Thread::finish_turn` should change from `push(record)` to:

```rust
for checkpoint in settlement.checkpoints {
    self.turns.push(checkpoint);
}
self.turns.push(settlement.user_turn);
```

`ThreadEvent::TurnCompleted` should still broadcast `settlement.user_turn.token_usage`.

**Step 4: Run the new and nearby thread tests**

Run: `cargo test -p argus-agent thread::tests::finish_turn --quiet`
Expected: PASS

Run: `cargo test -p argus-agent turn::tests:: --quiet`
Expected: PASS for the new settlement-shape tests

**Step 5: Commit**

```bash
git add crates/argus-agent/src/turn.rs crates/argus-agent/src/thread.rs crates/argus-agent/tests/integration_test.rs
git commit -m "Return settled turn records instead of a bare user turn"
```

### Task 3: Implement the new turn compactor prompt and message-shape rules

**Files:**
- Modify: `crates/argus-agent/src/compact/turn.rs`
- Modify: `crates/argus-agent/src/error.rs`

**Step 1: Write failing tests for turn compactor behavior**

Add tests for:

- user-history selection walks newest-to-oldest and stops around the `bytes / 4 <= 20000` budget
- output role is synthetic `user`
- `system_prompt` is included in the request but not in the returned checkpoint messages
- prompt text is user-perspective and forbids tool calls

Test skeleton:

```rust
assert_eq!(result.checkpoint_messages.last().unwrap().role, Role::User);
assert!(request.messages[0].role == Role::System);
assert!(result.checkpoint_messages.iter().all(|m| m.role != Role::System));
assert!(request.messages.iter().any(|m| m.content.contains("Do not call any tools")));
```

Run: `cargo test -p argus-agent compact::turn --quiet`
Expected: FAIL because `compact/turn.rs` is still a shell

**Step 2: Implement `LlmTurnCompactor`**

Create the provider-backed turn compactor with direct inputs:

```rust
pub struct LlmTurnCompactor {
    provider: Arc<dyn LlmProvider>,
}

impl LlmTurnCompactor {
    fn select_recent_user_inputs(messages: &[ChatMessage]) -> Vec<ChatMessage> { /* bytes / 4 budget */ }
    fn build_request_messages(system_prompt: &str, selected_history: &[ChatMessage], turn_messages: &[ChatMessage]) -> Vec<ChatMessage> { /* user-perspective prompt */ }
}
```

The returned `TurnCompactResult.checkpoint_messages` should contain:

- selected recent user inputs
- one synthetic `user` summary message

**Step 3: Keep errors isolated**

Reuse `CompactError` if it stays expressive enough. Only add a new variant in `crates/argus-agent/src/error.rs` if the turn path needs a clearer message than the existing generic compaction failure.

**Step 4: Run targeted tests**

Run: `cargo test -p argus-agent compact::turn --quiet`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-agent/src/compact/turn.rs crates/argus-agent/src/error.rs
git commit -m "Implement the user-perspective turn compactor"
```

### Task 4: Wire mid-turn compaction into `Turn::execute_loop`

**Files:**
- Modify: `crates/argus-agent/src/turn.rs`
- Modify: `crates/argus-agent/src/thread.rs`

**Step 1: Write failing execution-loop tests**

Add coverage for:

- repeated execute-loop compaction using the latest real request context, not a stale clone
- multiple successful turn-level compactions before a final `UserTurn`
- final `UserTurn` omitting messages already folded into earlier checkpoints
- compaction failure logging and continuing with the uncompressed context

Test shape:

```rust
assert_eq!(captured_requests.len(), 2);
assert!(captured_requests[1].messages.iter().any(|m| m.content == "fresh tool output"));
assert_eq!(settlement.checkpoints.len(), 2);
assert!(!settlement.user_turn.messages.iter().any(|m| m.content == "folded-away message"));
```

Run: `cargo test -p argus-agent execute_loop --quiet`
Expected: FAIL because `Turn::execute_loop` still only builds requests from `self.history + turn_messages`

**Step 2: Add turn-compactor dependencies to `Turn`**

Extend the builder and struct with:

```rust
#[builder(default, setter(strip_option))]
turn_compactor: Option<Arc<dyn TurnCompactor>>,
```

Do not add extra runtime mirrors like cached history windows or summary state.

**Step 3: Compact directly from current execution state**

Before each LLM request:

- inspect the current `self.history` and `turn_messages`
- ask the turn compactor whether to compact
- if it returns `Some(...)`, immediately:
  - convert the result into a `Checkpoint(0)` for the settlement payload
  - replace `self.history` with the compacted checkpoint messages
  - clear the folded-away `turn_messages`

Implementation sketch:

```rust
if let Some(compactor) = &self.turn_compactor {
    if let Some(result) = compactor
        .compact(&self.agent_record.system_prompt, self.history.as_ref(), &turn_messages)
        .await?
    {
        checkpoints.push(TurnRecord::checkpoint(result.checkpoint_messages.clone(), token_usage.clone()));
        self.history = Arc::new(result.checkpoint_messages);
        turn_messages.clear();
    }
}
```

Use the actual compaction return usage if the turn compactor exposes it; do not reuse stale provider usage from a previous iteration.

**Step 4: Build the final `TurnSettlement`**

At the successful `NextAction::Return` branch:

```rust
Ok(TurnSettlement {
    checkpoints,
    user_turn: self.build_turn_record(std::mem::take(&mut turn_messages), token_usage.clone()),
})
```

**Step 5: Run targeted loop tests**

Run: `cargo test -p argus-agent execute_loop --quiet`
Expected: PASS

**Step 6: Commit**

```bash
git add crates/argus-agent/src/turn.rs crates/argus-agent/src/thread.rs
git commit -m "Compact long-running turns transactionally"
```

### Task 5: Keep persistence and recovery behavior correct

**Files:**
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: `crates/argus-agent/src/turn_log_store.rs`
- Modify: `crates/argus-agent/tests/trace_integration_test.rs`

**Step 1: Write failing persistence tests**

Add tests for:

- successful turn-level compaction persists `Checkpoint(0)... + UserTurn(n)` in order
- cancelled turn-level compaction persists nothing
- failed turn-level compaction persists nothing
- trace recovery still treats the last checkpoint as the active recovery base

Test shape:

```rust
assert_eq!(recovered.turns.len(), 3);
assert!(matches!(recovered.turns[0].kind, TurnRecordKind::Checkpoint));
assert!(matches!(recovered.turns[1].kind, TurnRecordKind::Checkpoint));
assert!(matches!(recovered.turns[2].kind, TurnRecordKind::UserTurn));
```

Run: `cargo test -p argus-agent trace_integration_test --quiet`
Expected: FAIL until the new settlement append order is persisted end-to-end

**Step 2: Verify persistence code handles multi-record settlements**

`persist_trace_turns` already writes the in-memory `turns` snapshot; adjust only the assumptions or tests that still expect one new record per successful turn.

If needed, add a helper assertion in `turn_log_store.rs` tests that `RecoveredThreadLogState::turn_count()` still ignores checkpoint tails.

**Step 3: Run recovery and trace tests**

Run: `cargo test -p argus-agent turn_log_store --quiet`
Expected: PASS

Run: `cargo test -p argus-agent trace_integration_test --quiet`
Expected: PASS

**Step 4: Commit**

```bash
git add crates/argus-agent/src/thread.rs crates/argus-agent/src/turn_log_store.rs crates/argus-agent/tests/trace_integration_test.rs
git commit -m "Preserve recovery semantics for turn compaction checkpoints"
```

### Task 6: Clean up callers, docs, and full verification

**Files:**
- Modify: `crates/argus-agent/src/lib.rs`
- Modify: `crates/argus-agent/src/turn.rs`
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: `crates/argus-job/src/thread_pool.rs`
- Modify: `docs/plans/2026-04-05-turn-compaction-redesign-design.md` only if implementation reality requires a tiny correction

**Step 1: Fix remaining downstream references**

Update doctext, examples, imports, and type names so the public API reflects:

- renamed thread compactor exports
- turn compactor availability
- `TurnSettlement`-based execution path

**Step 2: Run focused verification**

Run: `cargo test -p argus-agent --quiet`
Expected: PASS

Run: `cargo test -p argus-job --quiet`
Expected: PASS

**Step 3: Run workspace formatting if needed**

Run: `cargo fmt --all`
Expected: no diff or only formatting changes

**Step 4: Re-run the highest-signal tests after formatting**

Run: `cargo test -p argus-agent --quiet`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-agent/src/lib.rs crates/argus-agent/src/turn.rs crates/argus-agent/src/thread.rs crates/argus-job/src/thread_pool.rs docs/plans/2026-04-05-turn-compaction-redesign-design.md
git commit -m "Finalize the turn compaction redesign wiring"
```

### Task 7: Final review before implementation handoff

**Files:**
- Review only: `docs/plans/2026-04-05-turn-compaction-redesign.md`
- Review only: `docs/plans/2026-04-05-turn-compaction-redesign-design.md`

**Step 1: Check the plan against the approved design**

Verify the plan still satisfies:

- thread compaction only renamed, not redesigned
- turn compaction uses user-perspective summaries
- no extra runtime mirror state
- successful settlement order is `Checkpoint... + UserTurn`
- failed/cancelled turns persist nothing from turn compaction

**Step 2: Leave the worktree ready for execution**

Run: `git status --short`
Expected: clean working tree

**Step 3: Commit only if this review changes the plan**

```bash
git add docs/plans/2026-04-05-turn-compaction-redesign.md docs/plans/2026-04-05-turn-compaction-redesign-design.md
git commit -m "Refine the turn compaction implementation plan"
```
