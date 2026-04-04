# Thread Single-Source TurnRecord Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Refactor thread history so `Vec<TurnRecord>` is the only source of truth, with append-only `meta.jsonl` persistence and no legacy compatibility path.

**Architecture:** Replace flattened/cached history state with typed append-only turn records (`SystemBootstrap`, `UserTurn`, `Checkpoint`) keyed by global `seq`. Build runtime context and history views directly from `turns`, and recover by replaying one `meta.jsonl` log with strict invariants. Keep reactor orchestration/event ordering unchanged.

**Tech Stack:** Rust, tokio, serde/serde_json, derive_builder, existing argus-agent reactor/turn pipeline.

---

### Task 1: Extend TurnRecord Model For Typed Append-Only History

**Files:**
- Modify: `crates/argus-agent/src/history.rs`
- Test: `crates/argus-agent/src/history.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn user_turn_number_derivation_ignores_checkpoint_records() {
    let records = vec![
        TurnRecord::system_bootstrap(0, vec![ChatMessage::system("sys")]),
        TurnRecord::user_completed(1, 1, vec![ChatMessage::user("u1")]),
        TurnRecord::checkpoint(2, 1, vec![ChatMessage::assistant("summary")]),
    ];

    assert_eq!(derive_next_user_turn_number(&records), 2);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-agent history::tests::user_turn_number_derivation_ignores_checkpoint_records -- --exact`
Expected: FAIL because new record kinds/helpers do not exist yet.

**Step 3: Write minimal implementation**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TurnRecordKind {
    SystemBootstrap,
    UserTurn,
    Checkpoint { through_turn: u32 },
}

pub fn derive_next_user_turn_number(turns: &[TurnRecord]) -> u32 {
    turns
        .iter()
        .filter(|t| matches!(t.kind, TurnRecordKind::UserTurn))
        .filter_map(|t| t.turn_number)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-agent history::tests::user_turn_number_derivation_ignores_checkpoint_records -- --exact`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-agent/src/history.rs
git commit -m "refactor(agent): add typed turn record model"
```

### Task 2: Switch Turn Log Persistence To Single Append-Only `meta.jsonl`

**Files:**
- Modify: `crates/argus-agent/src/turn_log_store.rs`
- Test: `crates/argus-agent/src/turn_log_store.rs`

**Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn append_and_recover_meta_jsonl_roundtrip() {
    // append system, user, checkpoint; recover and assert same sequence
}

#[tokio::test]
async fn recover_fails_when_first_record_is_not_system_bootstrap() {
    // write invalid log and assert strict error
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p argus-agent turn_log_store::tests::append_and_recover_meta_jsonl_roundtrip turn_log_store::tests::recover_fails_when_first_record_is_not_system_bootstrap -- --exact`
Expected: FAIL because append-only API and strict validation are not implemented.

**Step 3: Write minimal implementation**

```rust
pub async fn append_turn_record(base_dir: &Path, record: &TurnRecord) -> Result<(), TurnLogError> {
    // create turns dir, append one JSON line to turns/meta.jsonl
}

pub async fn recover_thread_log_state(base_dir: &Path) -> Result<RecoveredThreadLogState, TurnLogError> {
    // read turns/meta.jsonl line by line, deserialize + validate invariants
}
```

Also remove legacy `thread.meta.json`, checkpoint file, and per-turn `messages/meta` read-write paths.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p argus-agent turn_log_store::tests -- --nocapture`
Expected: PASS with new `meta.jsonl` roundtrip + strict error tests.

**Step 5: Commit**

```bash
git add crates/argus-agent/src/turn_log_store.rs
git commit -m "refactor(agent): replace turn log with append-only meta.jsonl"
```

### Task 3: Remove Duplicated Thread History Fields And Introduce `history_iter`

**Files:**
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: `crates/argus-agent/src/types.rs` (if message count derivation changes)
- Test: `crates/argus-agent/src/thread.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn history_iter_reads_from_turn_records_without_cached_flattened_messages() {
    let thread = seeded_thread_with_system_and_one_user_turn();
    let history: Vec<_> = thread.history_iter().map(|m| m.content.clone()).collect();
    assert_eq!(history, vec!["sys", "hi", "hello"]);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-agent thread::tests::history_iter_reads_from_turn_records_without_cached_flattened_messages -- --exact`
Expected: FAIL because `history_iter` does not exist and old fields are still authoritative.

**Step 3: Write minimal implementation**

```rust
pub fn history_iter(&self) -> impl Iterator<Item = &ChatMessage> + '_ {
    self.turns.iter().flat_map(|turn| turn.messages.iter())
}
```

Then remove:
- `messages`
- `system_messages`
- `cached_committed_messages`
- `next_turn_number`

Update `info().message_count` and all history call sites to use iterator counting/collecting.

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-agent thread::tests::history_iter_reads_from_turn_records_without_cached_flattened_messages -- --exact`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-agent/src/thread.rs crates/argus-agent/src/types.rs
git commit -m "refactor(agent): make turns the only thread history source"
```

### Task 4: Represent Compaction As Checkpoint TurnRecords

**Files:**
- Modify: `crates/argus-agent/src/thread.rs`
- Test: `crates/argus-agent/src/thread.rs`

**Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn compaction_appends_checkpoint_record_without_consuming_turn_number() {
    // ensure next user turn number stays contiguous after checkpoint append
}

#[tokio::test]
async fn build_turn_context_uses_latest_checkpoint_plus_following_user_turns() {
    // verify context assembly from checkpoint baseline
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p argus-agent thread::tests::compaction_appends_checkpoint_record_without_consuming_turn_number thread::tests::build_turn_context_uses_latest_checkpoint_plus_following_user_turns -- --exact`
Expected: FAIL because checkpoint still relies on dedicated field.

**Step 3: Write minimal implementation**

```rust
fn append_checkpoint_record(&mut self, through_turn: u32, summary_messages: Vec<ChatMessage>) {
    self.turns.push(TurnRecord::checkpoint(self.next_seq(), through_turn, summary_messages));
}
```

Update `build_turn_context()` to:
- locate latest checkpoint by `seq`
- seed from checkpoint `messages`
- append following `UserTurn` messages only

**Step 4: Run tests to verify they pass**

Run: `cargo test -p argus-agent thread::tests::compaction_appends_checkpoint_record_without_consuming_turn_number thread::tests::build_turn_context_uses_latest_checkpoint_plus_following_user_turns -- --exact`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-agent/src/thread.rs
git commit -m "refactor(agent): inline checkpoint state into turn records"
```

### Task 5: Update Recovery/Integration Paths To New Strict Log Format

**Files:**
- Modify: `crates/argus-agent/src/thread.rs`
- Modify: `crates/argus-job/src/thread_pool.rs`
- Modify: `crates/argus-session/src/manager.rs`
- Test: `crates/argus-session/src/manager.rs`

**Step 1: Write the failing integration test**

```rust
#[tokio::test]
async fn recover_messages_from_meta_jsonl_replays_typed_turn_records() {
    // seed meta.jsonl with system+turns+checkpoint and assert session recovery message flow
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-session manager::tests::recover_messages_from_meta_jsonl_replays_typed_turn_records -- --exact`
Expected: FAIL because integration still expects old recovered shape/APIs.

**Step 3: Write minimal implementation**

- Replace old recovery calls that rely on `system_messages` side channel.
- Consume recovered `turns` directly.
- Replace `thread.history()` usages with:

```rust
thread.history_iter().cloned().collect::<Vec<_>>()
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-session manager::tests::recover_messages_from_meta_jsonl_replays_typed_turn_records -- --exact`
Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-agent/src/thread.rs crates/argus-job/src/thread_pool.rs crates/argus-session/src/manager.rs
git commit -m "refactor(session): recover threads from typed turnrecord log"
```

### Task 6: Enforce Strict Log Validation + Failure Semantics

**Files:**
- Modify: `crates/argus-agent/src/turn_log_store.rs`
- Test: `crates/argus-agent/src/turn_log_store.rs`

**Step 1: Write failing tests for invalid logs**

```rust
#[tokio::test]
async fn recover_fails_on_out_of_order_seq() {}

#[tokio::test]
async fn recover_fails_on_non_monotonic_user_turn_numbers() {}

#[tokio::test]
async fn recover_fails_on_checkpoint_through_turn_ahead_of_history() {}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p argus-agent turn_log_store::tests::recover_fails_on_out_of_order_seq turn_log_store::tests::recover_fails_on_non_monotonic_user_turn_numbers turn_log_store::tests::recover_fails_on_checkpoint_through_turn_ahead_of_history -- --exact`
Expected: FAIL before validators are implemented.

**Step 3: Write minimal implementation**

- Add replay validator helpers.
- Return explicit `TurnLogError` variants for each invariant violation.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p argus-agent turn_log_store::tests -- --nocapture`
Expected: PASS and deterministic strict failures.

**Step 5: Commit**

```bash
git add crates/argus-agent/src/turn_log_store.rs
git commit -m "test(agent): enforce strict meta.jsonl replay invariants"
```

### Task 7: Full Regression Pass For Reactor Ordering And Thread Lifecycle

**Files:**
- Modify: `crates/argus-agent/src/thread.rs`
- Test: `crates/argus-agent/src/thread.rs`

**Step 1: Write failing regression tests (if missing) for event order and settle behavior**

```rust
#[tokio::test]
async fn completed_turn_emits_turn_settled_before_idle_after_refactor() {}

#[tokio::test]
async fn failed_turn_emits_turn_settled_before_idle_after_refactor() {}
```

**Step 2: Run tests to verify they fail or expose regressions**

Run: `cargo test -p argus-agent thread::tests::completed_turn_emits_turn_settled_before_idle_after_refactor thread::tests::failed_turn_emits_turn_settled_before_idle_after_refactor -- --exact`
Expected: FAIL if regressions exist, otherwise convert to guardrail tests and keep.

**Step 3: Write minimal implementation/fixes**

- Adjust settle/persist/event emission points only if tests reveal ordering regressions.
- Do not alter protocol semantics.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p argus-agent thread::tests -- --nocapture`
Expected: PASS for lifecycle and ordering tests.

**Step 5: Commit**

```bash
git add crates/argus-agent/src/thread.rs
git commit -m "test(agent): lock reactor event ordering after turnrecord refactor"
```

### Task 8: Final Verification And Documentation Sync

**Files:**
- Modify: `docs/plans/2026-04-03-thread-single-source-turnrecord-design.md` (if implementation reality diverges)
- Modify: `docs/plans/2026-04-03-thread-single-source-turnrecord-implementation-plan.md` (mark deltas)

**Step 1: Run focused test suites**

Run:

```bash
cargo test -p argus-agent
cargo test -p argus-session
cargo test -p argus-job
```

Expected: PASS.

**Step 2: Run quality gates**

Run:

```bash
prek
cargo deny check
```

Expected: PASS.

**Step 3: Record any intentional deviations**

- If code differs from design, update design doc with a short “Implemented Deviations” section.

**Step 4: Final commit**

```bash
git add docs/plans/2026-04-03-thread-single-source-turnrecord-design.md docs/plans/2026-04-03-thread-single-source-turnrecord-implementation-plan.md
git commit -m "docs: sync turnrecord single-source design and plan"
```

**Step 5: Prepare PR summary**

Include:
- removed state (`messages/system_messages/cached_committed_messages/next_turn_number/compaction_checkpoint`)
- new `meta.jsonl` persistence contract
- strict replay invariants
- no backward compatibility statement

---

## Execution Notes

- Follow TDD per task: red -> green -> commit.
- Keep each commit scoped to one task.
- Use `@superpowers/test-driven-development` for implementation.
- Use `@superpowers/verification-before-completion` before claiming completion.
- Use `@superpowers/requesting-code-review` before merge.
