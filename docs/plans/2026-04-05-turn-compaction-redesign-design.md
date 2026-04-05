# Turn Compaction Redesign

## Goal

Redesign compaction in `argus-agent` so thread-level compaction keeps its current checkpoint semantics, while turn-level compaction becomes a distinct execution-time mechanism that can compact long-running turns without polluting thread state on failure or cancellation.

## Decisions

- Discard the current in-flight turn compaction patch and redesign from first principles.
- Split compaction into two explicit implementations under a new `compact/` module directory.
- Keep thread-level compaction semantics unchanged while sharing a single `Compactor` abstraction with turn-level compaction.
- Add a new turn-level compaction path that runs inside `Turn::execute_loop`.
- Turn-level compaction is transactional: if a turn fails or is cancelled, no `TurnCheckpoint` is persisted.
- If a turn succeeds after internal compaction, settle it as a single counted `TurnCheckpoint` instead of `Checkpoint(0)... + UserTurn(n)`.

## Module Layout

Replace the single-file `crates/argus-agent/src/compact.rs` with:

- `crates/argus-agent/src/compact/mod.rs`
- `crates/argus-agent/src/compact/thread.rs`
- `crates/argus-agent/src/compact/turn.rs`

Shared compaction API:

- `Compactor`
- `CompactResult`

Concrete implementations:

- `LlmThreadCompactor`
- `LlmTurnCompactor`

The trait is shared, but the two implementations still have different semantics: thread compaction persists `Checkpoint(0)` records, while turn compaction settles as a counted `TurnCheckpoint`.

## Thread-Level Compaction

Thread-level compaction keeps the current behavior:

- It runs before a new turn starts.
- It produces a `Checkpoint(0)` persisted immediately in thread history.
- Recovery still uses "latest checkpoint + following user turns".
- The current implementation is mostly moved and renamed rather than redesigned.

This path remains responsible for normal thread checkpoint maintenance and recovery-oriented history trimming.

## Turn-Level Compaction

Turn-level compaction is a new execution-time mechanism for long turns that would otherwise overflow the provider context window during `execute_loop`.

### Trigger point

- Check for turn-level compaction only immediately before the next LLM request is built.
- Do not compact during streaming.
- Do not compact during tool execution.
- Recompute directly from the current execution state every time; do not introduce extra cached runtime mirrors.

### Input model

Each compaction attempt is built directly from the current state:

- `system_prompt`
- user history inputs selected from newest to oldest
- the current turn messages that are about to be folded away

User history selection rules:

- keep only user-authored inputs
- walk from newest to oldest
- estimate size with `bytes / 4`
- cap the retained history budget at roughly `20000` tokens

### Summary semantics

Turn-level compaction produces a synthetic `user` message, not an assistant summary.

The prompt must be rewritten around the user's perspective:

- what I asked you to do
- what you already discovered or completed
- what context you must keep in mind
- what you should continue doing next

The prompt must explicitly forbid tool calls and assistant-perspective narration.

## TurnRecord Commit Model

Turn-level compaction must not persist directly during execution.

Instead:

- immediately replace the active execution context with the compacted context
- keep only the latest compacted history plus any tail messages produced after the last compaction

If the turn later succeeds, persist:

- `UserTurn(n)` if no turn-level compaction happened
- `TurnCheckpoint(n)` if turn-level compaction happened

If the turn later fails or is cancelled:

- persist no `TurnCheckpoint`
- keep the invariant that failed/cancelled turns do not create `TurnRecord`s

## Persistence Boundaries

For turn-level checkpoints:

- persist a single `TurnCheckpoint(n)` message snapshot
- do not persist the `system_prompt`
- store the compacted history plus any post-compaction tail messages

For actual model requests:

- prepend the `system_prompt` only at request construction time

For counted turn settlement:

- `TurnCheckpoint` consumes a real turn number and participates in recovery, next-turn numbering, and context reconstruction

## Error Handling

- If turn-level compaction fails, log a warning and continue with the uncompressed context.
- Compaction failure must not fail the turn by itself.
- Thread-level and turn-level compaction failures should remain separately identifiable in logs and tests.

## Testing

Add focused tests for:

- repeated `execute_loop` compaction always using the latest real request context
- turn-level compaction settling as a single `TurnCheckpoint`
- no `TurnCheckpoint` being persisted when a turn is cancelled
- no `TurnCheckpoint` being persisted when a turn fails
- `TurnCheckpoint` never storing `system_prompt`
- turn-level synthetic summaries using the `user` role
- `TurnCheckpoint` participating in turn numbering and recovery
- transcript/session/job readers including `TurnCheckpoint` consistently
- thread-level compaction continuing to preserve current recovery semantics

## Non-Goals

- No reintroduction of per-turn settlement wrappers such as `TurnSettlement`.
- No extra runtime cache objects for turn compaction bookkeeping when direct recomputation is sufficient.
- No change to agent snapshot authority or prompt persistence rules.

## Migration Notes

- Update public exports in `crates/argus-agent/src/lib.rs`.
- Keep thread and turn compactor implementations separate even though they share one trait.
- Ensure transcript/session/job readers make an explicit decision about whether `TurnCheckpoint` belongs in their view.
