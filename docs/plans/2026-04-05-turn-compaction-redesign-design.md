# Turn Compaction Redesign

## Goal

Redesign compaction in `argus-agent` so thread-level compaction keeps its current checkpoint semantics, while turn-level compaction becomes a distinct execution-time mechanism that can compact long-running turns without polluting thread state on failure or cancellation.

## Decisions

- Discard the current in-flight turn compaction patch and redesign from first principles.
- Split compaction into two explicit implementations under a new `compact/` module directory.
- Keep thread-level compaction semantics unchanged; only rename its types to make the intent obvious.
- Add a new turn-level compaction path that runs inside `Turn::execute_loop`.
- Turn-level compaction is transactional: if a turn fails or is cancelled, none of its turn-level checkpoints are persisted.
- If a turn succeeds after multiple turn-level compactions, persist every successful checkpoint in order, then persist the final `UserTurn`.

## Module Layout

Replace the single-file `crates/argus-agent/src/compact.rs` with:

- `crates/argus-agent/src/compact/mod.rs`
- `crates/argus-agent/src/compact/thread.rs`
- `crates/argus-agent/src/compact/turn.rs`

Proposed naming split:

- Thread side:
  - `ThreadCompactor`
  - `ThreadCompactResult`
  - `LlmThreadCompactor`
- Turn side:
  - `TurnCompactor`
  - `TurnCompactResult`
  - `LlmTurnCompactor`

`Thread` depends only on the thread compactor. `Turn` depends only on the turn compactor. The two paths must not share a generic trait because they intentionally have different semantics.

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

- on each successful turn-level compaction, create a checkpoint record for the eventual settlement payload
- immediately replace the active execution context with the compacted context
- drop the folded-away tail from the future `UserTurn` body so the final record does not duplicate content already absorbed by checkpoints

If the turn later succeeds, persist:

`Checkpoint(0) -> Checkpoint(0) -> ... -> UserTurn(n)`

If the turn later fails or is cancelled:

- persist nothing from turn-level compaction
- keep the invariant that failed/cancelled turns do not create `TurnRecord`s

## Persistence Boundaries

For turn-level checkpoints:

- persist only the compacted turn context
- do not persist the `system_prompt`
- store the retained user history inputs and the synthetic `user` summary

For actual model requests:

- prepend the `system_prompt` only at request construction time

For the final `UserTurn`:

- store only the remaining tail after the last successful turn-level compaction

## Error Handling

- If turn-level compaction fails, log a warning and continue with the uncompressed context.
- Compaction failure must not fail the turn by itself.
- Thread-level and turn-level compaction failures should remain separately identifiable in logs and tests.

## Testing

Add focused tests for:

- repeated `execute_loop` compaction always using the latest real request context
- multiple turn-level compactions persisting as ordered checkpoints before the final `UserTurn`
- no turn-level checkpoints being persisted when a turn is cancelled
- no turn-level checkpoints being persisted when a turn fails
- turn-level checkpoints never storing `system_prompt`
- turn-level synthetic summaries using the `user` role
- final `UserTurn` omitting content already absorbed by earlier turn-level checkpoints
- thread-level compaction continuing to preserve current recovery semantics after the rename

## Non-Goals

- No change to the fundamental `ThreadRecordKind` model beyond adding turn-level checkpoint settlement into the existing `Checkpoint(0)` representation.
- No extra runtime cache objects for turn compaction bookkeeping when direct recomputation is sufficient.
- No change to agent snapshot authority or prompt persistence rules.

## Migration Notes

- Update public exports in `crates/argus-agent/src/lib.rs`.
- Update `argus-job` wiring to use the renamed thread compactor types.
- Rename existing tests to make it obvious whether they target thread compaction or turn compaction.
