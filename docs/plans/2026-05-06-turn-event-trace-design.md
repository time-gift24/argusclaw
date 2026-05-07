# Turn Event Trace Design

## Goal

Preserve enough in-progress turn state for the desktop chat UI to recover pending assistant display after switching sessions or threads. The recovered state covers assistant content, reasoning, tool call deltas, tool start, and tool completion results.

## Non-Goals

- Do not change `turns.jsonl` semantics.
- Do not persist failed or cancelled turns as `TurnRecord`.
- Do not make process events a runtime recovery source.
- Do not recover plan, retry, compaction, job, or mailbox side state in this change.

## Current Behavior

`turns.jsonl` stores only committed `TurnRecord` entries. It is the append-only source for settled history.

The desktop UI receives live `ThreadEvent` events and builds `pendingAssistant` in memory. When the user switches sessions or threads, `get_thread_snapshot` returns committed messages only. Any uncommitted assistant content, reasoning, or tool artifacts that were only held in the frontend store can disappear.

Turn compaction already runs automatically:

- Thread-level compaction runs before a new turn starts and commits a `Checkpoint(0)` if it succeeds.
- Turn-level compaction runs inside the turn loop against the current request messages. If the turn later succeeds, the turn settles as a single `TurnCheckpoint`.

## Recommended Approach

Add one process trace file per thread node:

```text
{thread_base_dir}/turn_events.jsonl
```

This file is separate from `turns.jsonl`. It records UI-replayable process events, not committed conversation history. A single thread trace node should have at most one `turn_events.jsonl`; events for all turns in that thread are appended to it with `turn_number` and a monotonic cursor owned by the trace store.

## Durable Append Log Semantics

`turn_events.jsonl` should behave like a narrow durable append log:

- Appends are ordered by the store, not by call sites.
- Each record receives a monotonic per-file `cursor`.
- Readers replay records in cursor order.
- A future replay API can support `read_after(cursor)` cleanly.
- If a consumer asks for a cursor that predates retained data, the correct response is a snapshot/rebase path, not silent partial replay.

The first implementation only needs snapshot-time replay to build `pending_assistant`. It should still model cursor ownership in the store so the file format can grow into replay-after semantics without changing every turn call site.

## Trace Event Model

Each line is one JSON event:

```json
{
  "turn_number": 3,
  "cursor": 42,
  "created_at": "2026-05-06T10:15:30Z",
  "payload": {
    "type": "content_delta",
    "delta": "hello"
  }
}
```

Payloads needed for the first version:

- `reasoning_delta { delta }`
- `content_delta { delta }`
- `tool_call_delta { index, id, name, arguments_delta }`
- `tool_started { tool_call_id, tool_name, arguments }`
- `tool_completed { tool_call_id, tool_name, result, is_error }`
- `turn_completed`
- `turn_failed`
- `turn_settled`

Terminal markers let the snapshot reader avoid returning stale pending state for already settled, failed, or cancelled turns.

## Write Path

`argus-agent` should append turn events where the corresponding live `ThreadEvent` is already produced:

- LLM stream events in `call_llm_streaming`
- tool start and completion in `execute_single_tool`
- terminal turn events when the thread settles the active turn

The write path must be best-effort. If writing `turn_events.jsonl` fails, the turn continues and logs a warning. This matches the current trace behavior and keeps UI trace persistence from affecting runtime correctness.

Call sites must not calculate cursor values. They pass `(turn_number, payload)` to a trace writer, and the writer appends the next ordered record.

Because tool calls can run in parallel, cursor assignment must be serialized. The initial implementation should open one `TurnEventTraceWriter` per active turn/thread execution, initialize its next cursor from the current non-empty line count, and guard cursor increments plus file appends with an async mutex. The writer can be cloned into the LLM stream path, parallel tool tasks, and terminal settlement path while keeping ordering centralized.

## Snapshot Recovery

`get_thread_snapshot` should continue returning committed messages from the live runtime or `turns.jsonl`.

It should additionally return:

```ts
pending_assistant: null | {
  turn_number: number;
  content: string;
  reasoning: string;
  tool_calls: Array<{
    tool_call_id: string;
    tool_name: string;
    arguments_text: string;
    result?: unknown;
    is_error: boolean;
    status: "streaming" | "running" | "completed";
  }>;
}
```

Recovery logic:

1. Read committed `turn_count`.
2. Replay `turn_events.jsonl` in cursor order for events with `turn_number > turn_count`.
3. Pick the latest such turn.
4. If it has `turn_failed` or `turn_settled`, return `pending_assistant: null`.
5. Otherwise fold deltas into the same shape the frontend already uses for `pendingAssistant`.

This keeps committed history and pending display state separate while giving the UI a stable way to restore missed events.

## Frontend Integration

Update the desktop snapshot contract and chat store:

- `ThreadSnapshotPayload` gains `pending_assistant`.
- `switchToThread` initializes `pendingAssistant` from the snapshot instead of always clearing it.
- `refreshSnapshot` clears `pendingAssistant` only when the snapshot has no recovered pending assistant.
- Existing live `_handleThreadEvent` logic continues to append new deltas after restoration.

`chat-runtime.ts` should not need a new aggregation model. It already renders `pendingAssistant`; the restored state should reuse that shape.

## Error Handling

- Malformed process trace lines should be skipped with a warning rather than failing snapshot recovery.
- Missing `turn_events.jsonl` means there is no recoverable pending assistant.
- Process trace persistence failure should never fail an LLM call or tool execution.

## Tests

Backend tests:

- Appending and replaying `turn_events.jsonl` restores content, reasoning, streamed tool call arguments, tool started state, and tool completed state.
- A settled or failed turn does not produce `pending_assistant`.
- Missing or malformed process trace does not block committed snapshot recovery.

Frontend tests:

- `switchToThread` restores `pendingAssistant` from snapshot.
- `refreshSnapshot` preserves recovered pending assistant while a turn is still running.
- Existing live deltas append to restored pending state.

## Open Boundary

This design intentionally does not recover plan, retry, compaction, job, or mailbox state. Those can use the same process trace pattern later if the UI needs them, but they should not be included in the first change.
