# Thread Single-Source TurnRecord Design

**Date:** 2026-04-03

**Goal:** Make `Vec<TurnRecord>` the only authoritative thread history state and remove duplicated/cached history structures from `Thread`.

## Summary

Current `Thread` history has three parallel structures:

- `messages: Arc<Vec<ChatMessage>>`
- `system_messages: Vec<ChatMessage>`
- `turns: Vec<TurnRecord>`

This creates conceptual duplication and extra synchronization logic. The new design makes `turns` the only source of truth.

Core decisions validated in discussion:

- Keep only `Vec<TurnRecord>` as authoritative history.
- No backward compatibility for old trace format.
- Remove `next_turn_number` field and derive from existing user turns.
- Remove `compaction_checkpoint` state/file; represent checkpoint as a `TurnRecord` kind.
- Checkpoint records do not consume user turn numbers.
- Replace per-turn metadata files with append-only single `meta.jsonl` log.

## Goals

- Eliminate duplicated history state in `Thread`.
- Make history semantics append-only and replayable from one log stream.
- Keep user turn numbering stable and independent of checkpoint records.
- Simplify recovery logic to one linear replay path.
- Preserve runtime event guarantees (`TurnSettled` before `Idle`).

## Non-Goals

- Compatibility with existing `thread.meta.json` / per-turn trace format.
- Migration tooling for old persisted traces.
- Introducing new external protocol fields unrelated to thread history.

## Data Model

### TurnRecord becomes the only durable state primitive

`Thread` history is represented exclusively as ordered `TurnRecord` entries.

Proposed shape (conceptual):

```rust
pub struct TurnRecord {
    pub seq: u64,
    pub kind: TurnRecordKind,
    pub turn_number: Option<u32>,
    pub state: TurnState,
    pub messages: Vec<ChatMessage>,
    pub token_usage: Option<TokenUsage>,
    pub context_token_count: Option<u32>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub model: Option<String>,
    pub error: Option<String>,
}

pub enum TurnRecordKind {
    SystemBootstrap,
    UserTurn,
    Checkpoint { through_turn: u32 },
}
```

Notes:

- `SystemBootstrap` uses `turn_number = Some(0)`.
- `UserTurn` uses `turn_number = Some(n)`, where `n >= 1` and strictly increasing.
- `Checkpoint` uses `turn_number = None` and does not consume user turn numbers.
- `seq` is global append order for all record kinds.

## Thread State Changes

### Remove fields

- `messages: Arc<Vec<ChatMessage>>`
- `system_messages: Vec<ChatMessage>`
- `cached_committed_messages: Option<Arc<Vec<ChatMessage>>>`
- `next_turn_number: u32`
- `compaction_checkpoint: Option<CompactionCheckpoint>`

### Keep fields

- `turns: Vec<TurnRecord>`
- `current_turn: Option<InFlightTurn>`
- existing runtime/orchestration fields (`runtime_snapshot`, mailbox, control channels, etc.)

## API Changes

### History read API

Replace:

```rust
pub fn history(&self) -> &[ChatMessage]
```

With:

```rust
pub fn history_iter(&self) -> impl Iterator<Item = &ChatMessage> + '_
```

Behavior:

- When no checkpoint exists: iterate `SystemBootstrap` + all `UserTurn` messages in append order.
- When checkpoints exist: use only the latest checkpoint baseline plus subsequent user turns.

Rationale:

- `&[ChatMessage]` requires contiguous backing storage.
- Without cached flattened vectors, iterator API is the only zero-duplication representation.

## Context Construction

`build_turn_context()` is derived entirely from `turns`.

Algorithm:

1. Find latest `Checkpoint { through_turn }` by highest `seq`.
2. If found, seed context with that checkpoint record `messages`.
3. Append messages from `UserTurn` records where:
   - record `seq` is after checkpoint `seq`, and
   - `turn_number > through_turn` (guard against inconsistent logs).
4. If no checkpoint exists, use full message stream from `history_iter()`.

## Turn Number Allocation

No stored `next_turn_number`.

New user turn number is always derived at start:

- `max(turn_number for kind=UserTurn).unwrap_or(0) + 1`

This keeps numbering correct even if recovery truncates or runtime restarts.

## Compaction Representation

Compaction no longer mutates a dedicated `compaction_checkpoint` field.

When compaction succeeds:

- append one synthetic `TurnRecord`:
  - `kind = Checkpoint { through_turn: <latest user turn> }`
  - `messages = summary_messages`
  - `state = Completed`
  - `turn_number = None`

No existing turn records are rewritten.

## Persistence Format

### New format

- single append-only file: `turns/meta.jsonl`
- each line is one complete serialized `TurnRecord` JSON

### Removed format

- `thread.meta.json`
- `checkpoints/latest.json`
- `turns/<n>.messages.jsonl`
- `turns/<n>.meta.json`

## Recovery Rules

Recovery is strict and fail-fast.

Process:

1. Read `turns/meta.jsonl` linearly.
2. Deserialize each line as `TurnRecord`.
3. Validate invariants while replaying:
   - first record is `SystemBootstrap` with `turn_number=0`
   - `seq` strictly increasing
   - user turn numbers strictly increasing from 1
   - checkpoint `through_turn` is not greater than known max user turn
4. Materialize `Vec<TurnRecord>` directly into `Thread`.

If any invariant fails, recovery returns an error and stops.

No backward compatibility is provided.

## Error Handling

- Malformed JSON line: hard recovery failure.
- Missing `SystemBootstrap`: hard recovery failure.
- Duplicate/out-of-order `seq`: hard recovery failure.
- Invalid user turn numbering: hard recovery failure.
- Invalid checkpoint bounds: hard recovery failure.

Runtime write failure on append:

- turn settlement is still in-memory authoritative.
- persistence error is surfaced via existing warning/error path.
- no background repair in this scope.

## Testing Strategy

### Unit tests

- `history_iter` returns correct sequence with no checkpoint.
- `history_iter`/context uses latest checkpoint baseline when multiple checkpoints exist.
- derived next user turn number ignores checkpoint/system records.
- checkpoint append does not alter existing user turn numbers.

### Persistence tests

- append + recover roundtrip for mixed record kinds.
- recovery fails on malformed lines.
- recovery fails when first record is not `SystemBootstrap`.
- recovery fails on invalid `seq` or user turn ordering.

### Runtime tests

- completion path appends `UserTurn` record and maintains event order.
- compaction path appends `Checkpoint` record and context trims correctly.
- cancellation/failure still settle into `UserTurn` records with expected `TurnState`.

## Impacted Files (Expected)

- `crates/argus-agent/src/history.rs`
- `crates/argus-agent/src/thread.rs`
- `crates/argus-agent/src/turn_log_store.rs`
- `crates/argus-job/src/thread_pool.rs` (recovery integration changes)
- `crates/argus-session/src/manager.rs` (if relying on `history()` slice API)
- tests under `crates/argus-agent/src/thread.rs` and `crates/argus-agent/src/turn_log_store.rs`

## Rollout

This is a breaking internal trace format change and should be landed as one atomic refactor in a dedicated branch, with strict tests proving:

- deterministic replay,
- stable turn numbering,
- unchanged runtime event ordering,
- no behavior regression in queue/cancel/approval flow.
