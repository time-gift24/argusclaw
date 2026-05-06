# Turn Event Trace Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Persist UI-replayable in-progress turn events so the desktop chat can restore pending assistant content, reasoning, and tool artifacts after switching sessions or threads.

**Architecture:** Keep `turns.jsonl` as the committed transcript source and add a separate per-thread `turn_events.jsonl` process trace. The process trace is a narrow durable append log: the store owns monotonic cursors, call sites append typed payloads, and snapshot recovery folds uncommitted events into a `pending_assistant` payload.

**Tech Stack:** Rust workspace crates (`argus-agent`, `argus-session`, `argus-wing`, Tauri bridge), React/TypeScript desktop store, JSONL trace files, Tokio async file IO.

---

### Task 1: Add Turn Event Trace Store

**Files:**
- Create: `crates/argus-agent/src/turn_event_store.rs`
- Modify: `crates/argus-agent/src/lib.rs`
- Test: `crates/argus-agent/src/turn_event_store.rs`

**Step 1: Write failing tests**

Add tests for append/replay and pending reconstruction:

```rust
#[tokio::test]
async fn recovers_pending_assistant_from_turn_events() {
    let dir = tempfile::tempdir().expect("temp dir");
    let writer = TurnEventTraceWriter::open(dir.path()).await.unwrap();
    let first = writer
        .append(1, TurnTraceEventPayload::content_delta("hello"))
        .await
        .unwrap();
    let second = writer
        .append(1, TurnTraceEventPayload::reasoning_delta("thinking"))
        .await
        .unwrap();
    writer
        .append(
            1,
            TurnTraceEventPayload::tool_call_delta(
                0,
                Some("call-1"),
                Some("search"),
                Some("{\"q\""),
            ),
        )
        .await
        .unwrap();
    writer
        .append(
            1,
            TurnTraceEventPayload::tool_call_delta(0, None, None, Some(":\"rust\"}")),
        )
        .await
        .unwrap();
    writer
        .append(
            1,
            TurnTraceEventPayload::tool_started(
                "call-1",
                "search",
                serde_json::json!({"q":"rust"}),
            ),
        )
        .await
        .unwrap();
    writer
        .append(
            1,
            TurnTraceEventPayload::tool_completed(
                "call-1",
                "search",
                serde_json::json!({"ok":true}),
                false,
            ),
        )
        .await
        .unwrap();

    assert_eq!(first.cursor, TurnEventCursor::new(1));
    assert_eq!(second.cursor, TurnEventCursor::new(2));

    let pending = recover_pending_assistant(dir.path(), 0).await.unwrap().unwrap();
    assert_eq!(pending.turn_number, 1);
    assert_eq!(pending.content, "hello");
    assert_eq!(pending.reasoning, "thinking");
    assert_eq!(pending.tool_calls[0].arguments_text, "{\"q\":\"rust\"}");
    assert_eq!(pending.tool_calls[0].status, PendingToolStatus::Completed);
}

#[tokio::test]
async fn settled_turn_does_not_recover_pending_assistant() {
    let dir = tempfile::tempdir().expect("temp dir");
    let writer = TurnEventTraceWriter::open(dir.path()).await.unwrap();
    writer
        .append(1, TurnTraceEventPayload::content_delta("done"))
        .await
        .unwrap();
    writer
        .append(1, TurnTraceEventPayload::turn_settled())
        .await
        .unwrap();

    let pending = recover_pending_assistant(dir.path(), 0).await.unwrap();
    assert!(pending.is_none());
}

#[tokio::test]
async fn writer_assigns_unique_cursors_for_concurrent_appends() {
    let dir = tempfile::tempdir().expect("temp dir");
    let writer = TurnEventTraceWriter::open(dir.path()).await.unwrap();

    let left = writer.clone();
    let right = writer.clone();
    let (left, right) = tokio::join!(
        left.append(1, TurnTraceEventPayload::content_delta("a")),
        right.append(1, TurnTraceEventPayload::content_delta("b")),
    );

    let mut cursors = vec![left.unwrap().cursor, right.unwrap().cursor];
    cursors.sort();
    assert_eq!(cursors, vec![TurnEventCursor::new(1), TurnEventCursor::new(2)]);
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p argus-agent turn_event_store -- --nocapture
```

Expected: FAIL because `turn_event_store` does not exist.

**Step 3: Implement minimal store**

Create serializable types:

- `TurnEventCursor(u64)`
- `TurnTraceEvent { turn_number, cursor, created_at, payload }`
- `TurnTraceEventPayload`
- `TurnEventTraceWriter`
- `PendingAssistantTrace`
- `PendingToolCallTrace`
- `PendingToolStatus`

Implement:

- `turn_events_jsonl_path(base_dir)`
- `TurnEventTraceWriter::open(base_dir) -> TurnEventTraceWriter`
- `TurnEventTraceWriter::append(turn_number, payload) -> TurnTraceEvent`
- `recover_pending_assistant(base_dir, committed_turn_count)`

Use `tokio::fs::OpenOptions` append mode. The store owns ordering: the writer derives the initial next cursor from the current non-empty line count, guards cursor increments plus file appends with an async mutex, appends exactly one JSON line, and returns the event with its assigned cursor. Call sites must never provide cursor values.

The writer must be cloneable so the same writer can be shared by the LLM stream path, parallel tool tasks, and terminal settlement path. This avoids duplicate cursor assignment when tool calls complete concurrently.

Skip malformed JSONL lines with `tracing::warn!` for snapshot recovery. Keep the cursor type explicit so a future `read_after(cursor)` API can return a replay-gap error instead of silently losing events after retention.

**Step 4: Export module**

In `crates/argus-agent/src/lib.rs`, add:

```rust
pub mod turn_event_store;
```

**Step 5: Run tests**

Run:

```bash
cargo test -p argus-agent turn_event_store -- --nocapture
```

Expected: PASS.

**Step 6: Commit**

```bash
git add crates/argus-agent/src/lib.rs crates/argus-agent/src/turn_event_store.rs
git commit -m "feat(agent): add turn event trace store"
```

### Task 2: Persist Process Events During Turns

**Files:**
- Modify: `crates/argus-agent/src/turn.rs`
- Modify: `crates/argus-agent/src/thread.rs`
- Test: `crates/argus-agent/tests/trace_integration_test.rs`

**Step 1: Write failing integration test**

Add a test that creates a traced thread, runs a turn that emits a streamed content delta and a tool call, and asserts `turn_events.jsonl` exists with content/tool events.

Use existing trace integration helpers where possible. Keep the test focused on file creation and replay, not frontend behavior.

**Step 2: Run failing test**

Run:

```bash
cargo test -p argus-agent trace_integration_test turn_events -- --nocapture
```

Expected: FAIL because no process events are persisted.

**Step 3: Add trace config to turn execution**

Extend `execute_thread_turn` and `TurnContext` to receive an optional `TurnEventTraceWriter`.

Create the writer in the thread layer when trace config is enabled:

- before spawning `execute_thread_turn` in `start_turn_execution`
- before the direct `execute_turn` path

Keep a clone available in `Thread` settlement so terminal events use the same cursor sequence as the turn body.

Concretely, add an `active_turn_event_writer: Option<TurnEventTraceWriter>` runtime field to `Thread`. Set it when starting a turn, pass a clone into `execute_thread_turn`, use the stored clone in `settle_active_turn` for terminal markers, and clear it with `active_turn_cancellation`.

Keep persistence best-effort:

```rust
async fn append_process_event(ctx: &TurnContext<'_>, payload: TurnTraceEventPayload) {
    let Some(writer) = ctx.turn_event_writer.as_ref() else { return; };
    if let Err(error) = writer.append(ctx.turn_number, payload).await {
        tracing::warn!(error = %error, "failed to append turn process event");
    }
}
```

Do not add a per-turn sequence counter in `execute_loop` or `Thread`. The durable append store owns ordering. This matters because LLM stream events, tool events, and terminal events are emitted from different functions; centralized cursor assignment avoids ordering conflicts.

**Step 4: Persist live event equivalents**

Append events when existing frontend events are emitted:

- In streaming event loop: `ReasoningDelta`, `ContentDelta`, `ToolCallDelta`
- In `execute_single_tool`: `tool_started`, `tool_completed`
- In `settle_active_turn`: `turn_completed`, `turn_failed`, `turn_settled`

Do not persist `LlmStreamEvent::Usage` for this first version.

For terminal markers, append `turn_completed` before `turn_settled` on success, and `turn_failed` before `turn_settled` on failure. Cancelled turns should append `turn_settled` with no pending payload recovery, matching the UI need to clear stale in-flight state.

**Step 5: Run tests**

Run:

```bash
cargo test -p argus-agent trace_integration_test turn_events -- --nocapture
```

Expected: PASS.

**Step 6: Commit**

```bash
git add crates/argus-agent/src/turn.rs crates/argus-agent/src/thread.rs crates/argus-agent/tests/trace_integration_test.rs
git commit -m "feat(agent): persist turn process events"
```

### Task 3: Add Pending Assistant to Snapshot Contract

**Files:**
- Modify: `crates/argus-session/src/manager.rs`
- Modify: `crates/argus-wing/src/lib.rs`
- Modify: `crates/desktop/src-tauri/src/commands.rs`
- Test: `crates/argus-session/src/manager.rs`
- Test: `crates/desktop/tests/chat-tauri-bindings.test.mjs`

**Step 1: Write failing backend test**

Add a session manager test that writes committed turn count `0`, appends `turn_events.jsonl`, calls `get_thread_snapshot`, and expects a non-null pending assistant.

**Step 2: Run failing backend test**

Run:

```bash
cargo test -p argus-session pending_assistant_snapshot -- --nocapture
```

Expected: FAIL because snapshots return only messages/counts.

**Step 3: Introduce snapshot type**

Replace tuple return paths with a small struct, for example:

```rust
pub struct ThreadSnapshot {
    pub messages: Vec<ChatMessage>,
    pub turn_count: u32,
    pub token_count: u32,
    pub plan_item_count: u32,
    pub pending_assistant: Option<PendingAssistantTrace>,
}
```

Keep this in `argus-session` or a lower shared crate only if needed. Do not put desktop-only naming into `argus-agent`.

**Step 4: Recover pending assistant in session layer**

In `SessionManager::get_thread_snapshot`, after determining committed `turn_count`, call:

```rust
recover_pending_assistant(&thread_base_dir, turn_count).await
```

Use the same trace base directory resolution already used by `recover_thread_state_from_trace`.

**Step 5: Update facade and Tauri payload**

Propagate `ThreadSnapshot` through `argus-wing`.

In `crates/desktop/src-tauri/src/commands.rs`, add serializable payload structs matching the frontend shape:

- `PendingAssistantPayload`
- `PendingToolCallPayload`

Set `pending_assistant` in `ThreadSnapshotPayload`.

**Step 6: Run tests**

Run:

```bash
cargo test -p argus-session pending_assistant_snapshot -- --nocapture
node crates/desktop/tests/chat-tauri-bindings.test.mjs
```

Expected: PASS.

**Step 7: Commit**

```bash
git add crates/argus-session/src/manager.rs crates/argus-wing/src/lib.rs crates/desktop/src-tauri/src/commands.rs crates/desktop/tests/chat-tauri-bindings.test.mjs
git commit -m "feat(session): expose pending assistant snapshots"
```

### Task 4: Restore Pending Assistant in Desktop Store

**Files:**
- Modify: `crates/desktop/lib/types/chat.ts`
- Modify: `crates/desktop/lib/tauri.ts`
- Modify: `crates/desktop/lib/chat-store.ts`
- Test: `crates/desktop/tests/chat-store-session-model.test.mjs`

**Step 1: Write failing frontend tests**

Add assertions that:

- `ThreadSnapshotPayload` includes `pending_assistant`.
- `switchToThread` maps snapshot `pending_assistant` to `pendingAssistant`.
- `refreshSnapshot` uses recovered pending state instead of always clearing it.

**Step 2: Run failing tests**

Run:

```bash
node crates/desktop/tests/chat-store-session-model.test.mjs
```

Expected: FAIL because the contract and mapping do not exist.

**Step 3: Add frontend types**

Add:

```ts
export interface PendingAssistantSnapshotPayload {
  turn_number: number;
  content: string;
  reasoning: string;
  tool_calls: PendingToolCall[];
}
```

Extend both `crates/desktop/lib/types/chat.ts` and `crates/desktop/lib/tauri.ts` snapshot interfaces.

**Step 4: Map snapshot pending state**

Add a helper in `chat-store.ts`:

```ts
const mapPendingAssistantSnapshot = (
  pending: ThreadSnapshotPayload["pending_assistant"],
): ChatSessionState["pendingAssistant"] =>
  pending
    ? {
        content: pending.content,
        reasoning: pending.reasoning,
        toolCalls: pending.tool_calls,
        plan: null,
        retry: null,
      }
    : null;
```

Use it in `activateSession`, `switchToThread`, and `refreshSnapshot`.

**Step 5: Run frontend tests**

Run:

```bash
node crates/desktop/tests/chat-store-session-model.test.mjs
```

Expected: PASS.

**Step 6: Commit**

```bash
git add crates/desktop/lib/types/chat.ts crates/desktop/lib/tauri.ts crates/desktop/lib/chat-store.ts crates/desktop/tests/chat-store-session-model.test.mjs
git commit -m "feat(desktop): restore pending assistant from snapshots"
```

### Task 5: Full Verification

**Files:**
- No source edits expected.

**Step 1: Run Rust checks scoped to touched crates**

Run:

```bash
cargo test -p argus-agent -p argus-session -p argus-wing
```

Expected: PASS.

**Step 2: Run desktop tests**

Run:

```bash
cd crates/desktop
pnpm test
```

Expected: PASS or document the exact unavailable script if the project has no `test` command.

**Step 3: Run pre-commit manually**

Run:

```bash
prek
```

Expected: PASS. If `prek` hangs, capture the stuck hook/process and note it in the final handoff.

**Step 4: Final commit if verification required changes**

```bash
git status --short
git add <changed files>
git commit -m "test: cover turn event trace recovery"
```

Expected: clean worktree after commits.
