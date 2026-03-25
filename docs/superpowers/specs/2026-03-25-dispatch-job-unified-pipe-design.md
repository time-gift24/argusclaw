# Dispatch Job Unified Pipe Design

**Date:** 2026-03-25
**Status:** Approved

## Overview

Replace the fragmented communication model (separate SSE broadcaster for jobs, `get_job_result` polling, direct `send_message()` calls) with a single `broadcast::Channel<ThreadEvent>` per Thread. All inputs — user messages, job dispatches, job results, user interrupts — flow through the same pipe.

## Architecture

Each Thread owns a `broadcast::Sender<ThreadEvent>` (the pipe). Pipes are scoped per Thread — events never leak across Threads.

- **Idle Thread**: owns the pipe. Waits on it. When `UserMessage` arrives, spawns a Turn. Queues additional incoming messages if a turn is already running.
- **Running Turn**: at each `execute_loop` iteration start, non-blocking poll of the pipe. Processes `JobResult` and `UserInterrupt` by injecting into the turn's message context. Never blocks the LLM loop.
- **dispatch_job tool**: fire-and-forget. Generates `job_id`, sends `JobDispatched` into the pipe, spawns background job executor, returns immediately.
- **get_job_result**: removed entirely. Replaced by `ThreadEvent::JobResult` channel events.

## ThreadEvent Extensions

New variants added to `ThreadEvent` in `argus-protocol/src/events.rs`:

```rust
JobDispatched {
    job_id: String,
    agent_id: AgentId,
    prompt: String,
    context: Option<serde_json::Value>,
}

JobResult {
    job_id: String,
    success: bool,
    message: String,
    token_usage: Option<TokenUsage>,
}

UserInterrupt {
    content: String,
}

UserMessage {
    content: String,
    msg_override: Option<MessageOverride>,
}
```

The existing `JobCompleted` variant is removed (replaced by `JobResult` with richer data).

## NamedTool Signature Change

`NamedTool::execute` signature changes from:

```rust
async fn execute(&self, input: serde_json::Value) -> Result<serde_json::Value, ToolError>
```

To:

```rust
async fn execute(
    &self,
    input: serde_json::Value,
    ctx: Arc<ToolExecutionContext>,
) -> Result<serde_json::Value, ToolError>
```

Where `ToolExecutionContext` (new struct in `argus-protocol/src/tool.rs`):

```rust
pub struct ToolExecutionContext {
    pub thread_id: ThreadId,
    pub pipe_tx: broadcast::Sender<ThreadEvent>,
}
```

> **Naming note:** The existing `event_sender` in Thread (`broadcast::Sender<ThreadEvent>`) and `thread_event_tx` in Turn are the same concept renamed. The spec uses `pipe_tx` / "pipe" consistently. In implementation, Thread's `event_sender` becomes `pipe_tx` and Turn's `thread_event_tx` remains as the pipe transmitter passed in via context.

All existing tool implementations are updated to accept the new signature. Tools that do not need the context simply ignore it.

## dispatch_job Tool

Simplified to fire-and-forget:

- **Input**: `{ prompt, agent_id, context }`
- **Output**: `{ job_id, status: "dispatched" }`

Flow:
1. Generate `job_id = Uuid::new_v4()`
2. Send `ThreadEvent::JobDispatched { job_id, agent_id, prompt, context }` into the pipe
3. Spawn background task: run Turn via `TurnBuilder`, on completion send `ThreadEvent::JobResult`
4. Return immediately

Retry logic (current 3-attempt exponential backoff) is removed. If dispatch fails, the job executor handles failures internally and reports via `JobResult { success: false }`.

## get_job_result — Removed

Replaced entirely by `ThreadEvent::JobResult` channel events. No polling needed. The `GetJobResultTool` and `JobManager::get_result()` are deleted.

## User Input Entry Point

External callers (CLI, Tauri) call:

```rust
thread.send_user_message(content: String, msg_override: Option<MessageOverride>)
```

This writes `UserMessage` into the pipe. The existing `send_message()` method is removed from `Thread`'s public API.

## Thread.run() — Main Orchestration Loop

`Thread.run()` is invoked by the session layer. The session holds `Arc<Mutex<Thread>>` and spawns `run()` as a background task:

```rust
// In session:
let thread = Arc::clone(&self.thread);
tokio::spawn(async move {
    thread.run().await;
});
```

Since `run()` takes `&mut self`, it must be called within a mutable reference context (e.g., `MutexGuard<Thread>`).

The broadcast channel capacity is **256** (existing constant `DEFAULT_CHANNEL_CAPACITY`). If the pipe fills (unlikely with 256 slots), `send` returns `Err` — logged as warning but does not block execution.

```rust
async fn run(&mut self) {
    let mut rx = self.subscribe();
    let mut pending_user_message: Option<UserMessage> = None;

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event? {
                    ThreadEvent::UserMessage(msg) => {
                        if self.is_turn_running() {
                            if pending_user_message.is_none() {
                                pending_user_message = Some(msg);
                            }
                        } else {
                            self.spawn_turn(msg).await;
                        }
                    }
                    ThreadEvent::Idle { .. } => {
                        // Turn always sends Idle even on failure (handled in Turn::execute)
                        if let Some(msg) = pending_user_message.take() {
                            self.spawn_turn(msg).await;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
```

> **Guarantee on crash:** `Turn::execute()` always sends `Idle` (or `TurnFailed`) before returning, even on panic. The pending message is never lost because `Idle` is emitted in a `finally`-style block after the main loop completes or fails.

Multiple concurrent user messages are queued (FIFO, one pending max).

## Turn Polling — Iteration Start

At the top of each `execute_loop` iteration, Turn drains the pipe non-blocking:

```rust
while let Ok(event) = rx.try_recv() {
    match event {
        ThreadEvent::JobResult { job_id, success, message, token_usage } => {
            let status = if success { "completed" } else { "failed" };
            let msg = format!("Job {} {}: {}", job_id, status, message);
            messages.push(ChatMessage::user(&msg));
        }
        ThreadEvent::UserInterrupt { content } => {
            messages.push(ChatMessage::user(&content));
        }
        _ => {}
    }
}
```

The receiver `rx` is created at `Turn::execute()` start via `thread_event_tx.subscribe()`. Turn no longer creates its own internal `broadcast::channel`. The pipe channel is owned by Thread; Turn subscribes to it for reading. The event forwarder task (which previously converted `TurnStreamEvent` → `ThreadEvent`) still runs internally, but now writes to the Thread-owned pipe via `thread_event_tx`.

## Job Executor

Lightweight — uses `TurnBuilder` directly (no separate Thread per job):

The background task receives `JobDispatched { job_id, agent_id, prompt, context }` from the pipe. Variables are resolved as follows:

- **provider**: resolved from `agent_id` via `provider_resolver` (same logic as current `execute_job`)
- **agent_record**: resolved from `agent_id` via `template_manager`
- **tools**: filtered from `tool_manager` by `agent_record.tool_names`
- **pipe_tx**: passed as `thread_event_tx` to `TurnBuilder`
- **stream_tx**: created fresh as `broadcast::channel(256)` (Turn internal use)

```rust
tokio::spawn(async move {
    let thread_id = format!("job-{}", job_id);

    // Resolve agent_record and provider (same as current execute_job)
    let agent_record = template_manager.get(agent_id).await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("agent {} not found", agent_id))?;

    let provider = provider_resolver.resolve(agent_record.provider_id).await
        .map_err(|e| e.to_string())?;

    let tools: Vec<Arc<dyn NamedTool>> = tool_manager
        .list_ids()
        .iter()
        .filter(|name| agent_record.tool_names.contains(name))
        .filter_map(|name| tool_manager.get(name))
        .collect();

    let (stream_tx, _stream_rx) = broadcast::channel(256);

    let turn = TurnBuilder::default()
        .turn_number(1)
        .thread_id(thread_id.clone())
        .messages(vec![ChatMessage::user(&prompt)])
        .provider(provider)
        .tools(tools)
        .hooks(Vec::new())
        .config(TurnConfig::new())
        .agent_record(agent_record)
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .build()
        .map_err(|e| e.to_string())?;

    let output = turn.execute().await;

    match output {
        Ok(o) => {
            pipe_tx.send(ThreadEvent::JobResult {
                job_id,
                success: true,
                message: summarize_job_output(&o),
                token_usage: Some(o.token_usage),
            }).ok();
        }
        Err(e) => {
            pipe_tx.send(ThreadEvent::JobResult {
                job_id,
                success: false,
                message: e.to_string(),
                token_usage: None,
            }).ok();
        }
    }
});

/// Summarize TurnOutput into a brief result string.
fn summarize_job_output(output: &TurnOutput) -> String {
    for msg in output.messages.iter().rev() {
        if let ChatMessage { role: Role::Assistant, content, .. } = msg
            && !content.is_empty()
        {
            if content.len() > 500 {
                return format!("{}...", &content[..500]);
            }
            return content.clone();
        }
    }
    format!("job completed, {} messages in turn", output.messages.len())
}
```

## Error Handling

- **Pipe send fails**: log warning, do not block execution
- **Job execution fails**: send `JobResult { success: false }` into pipe
- **Turn poll misses event**: events are broadcast; Turn processes what it drains at iteration start

## Files to Change

| Crate | File | Change |
|-------|------|--------|
| `argus-protocol` | `src/events.rs` | Add `JobDispatched`, `JobResult`, `UserInterrupt`, `UserMessage`. Remove `JobCompleted`. Add imports: `AgentId` from `ids.rs`, `TokenUsage` from `token_usage.rs`, `MessageOverride` from `message_override.rs`. |
| `argus-protocol` | `src/tool.rs` | Add `ToolExecutionContext`. Update `NamedTool` trait signature. |
| `argus-protocol` | `src/lib.rs` | Export `ToolExecutionContext` |
| `argus-job` | `src/dispatch_tool.rs` | Fire-and-forget, send into pipe, remove retry and wait logic. Accept `ToolExecutionContext` |
| `argus-job` | `src/get_job_result_tool.rs` | Delete entire file |
| `argus-job` | `src/job_manager.rs` | Remove `get_result()`, update dispatch to use pipe. Accept `ToolExecutionContext` |
| `argus-job` | `src/lib.rs` | Remove `GetJobResultTool` export |
| `argus-job` | `src/types.rs` | Remove `wait_for_result` from `JobDispatchArgs`. Remove `JobDispatchResult` (replaced by `JobResult`). |
| `argus-thread` | `src/thread.rs` | Rename `event_sender` → `pipe_tx`. Add `run()`, `send_user_message()`, `spawn_turn()`, `is_turn_running()`. Remove `send_message()` from public API. |
| `argus-thread` | `src/lib.rs` | Export `run()` |
| `argus-turn` | `src/turn.rs` | Add pipe receiver (`rx`) from `thread_event_tx.subscribe()` at `execute()` start. Poll at iteration start. Remove internal channel creation. |
| `argus-session` | `src/manager.rs` | Update tool registration with `ToolExecutionContext`. Remove `get_job_result` tool registration. Spawn `thread.run()` as background task. |
| All tool implementations | Various | Update `execute` signature to accept `ctx: Arc<ToolExecutionContext>` |
| `desktop` | Various | Update API calls to use `send_user_message()` |
| `cli` | Various | Update CLI calls to use `send_user_message()` |

## Constraints

- **Turn atomicity**: when a Turn is running, Thread does not process pipe messages except for turn lifecycle events (Idle). The running Turn drains the pipe at iteration boundaries only.
- **Single pending user message**: if a second user message arrives while a Turn is running, it is queued (max 1 pending). Further messages are dropped.
- **No parent turn**: a Thread has at most one Turn running at any time.
- **Pipe scoping**: each Thread has its own pipe. No cross-Thread event leakage.
