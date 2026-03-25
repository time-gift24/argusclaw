# Dispatch Job Unified Pipe Design

**Date:** 2026-03-25
**Status:** Draft

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

The receiver `rx` is created at `Turn::execute()` start via `thread_event_tx.subscribe()`.

## Job Executor

Lightweight — uses `TurnBuilder` directly (no separate Thread per job):

```rust
tokio::spawn(async move {
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
                message: summarize(&o),
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
```

## Error Handling

- **Pipe send fails**: log warning, do not block execution
- **Job execution fails**: send `JobResult { success: false }` into pipe
- **Turn poll misses event**: events are broadcast; Turn processes what it drains at iteration start

## Files to Change

| Crate | File | Change |
|-------|------|--------|
| `argus-protocol` | `src/events.rs` | Add `JobDispatched`, `JobResult`, `UserInterrupt`, `UserMessage`. Remove `JobCompleted`. |
| `argus-protocol` | `src/tool.rs` | Add `ToolExecutionContext`. Update `NamedTool` trait signature. |
| `argus-protocol` | `src/lib.rs` | Export new types |
| `argus-job` | `src/dispatch_tool.rs` | Fire-and-forget, send into pipe, remove retry and wait logic |
| `argus-job` | `src/get_job_result_tool.rs` | Delete entire file |
| `argus-job` | `src/job_manager.rs` | Remove `get_result()`, update dispatch to use pipe |
| `argus-job` | `src/lib.rs` | Remove `GetJobResultTool` export |
| `argus-job` | `src/types.rs` | Remove `JobDispatchResult.wait_for_result`, update as needed |
| `argus-thread` | `src/thread.rs` | Add `run()` loop, `send_user_message()`, `spawn_turn()`, `is_turn_running()` |
| `argus-thread` | `src/lib.rs` | Export `run()` |
| `argus-turn` | `src/turn.rs` | Add pipe receiver, poll at iteration start |
| `argus-session` | `src/manager.rs` | Update tool registration, remove `get_job_result` |
| All tool implementations | Various | Update `execute` signature to accept `ctx` |
| `desktop` | Various | Update API calls to use `send_user_message()` |
| `cli` | Various | Update CLI calls to use `send_user_message()` |

## Constraints

- **Turn atomicity**: when a Turn is running, Thread does not process pipe messages except for turn lifecycle events (Idle). The running Turn drains the pipe at iteration boundaries only.
- **Single pending user message**: if a second user message arrives while a Turn is running, it is queued (max 1 pending). Further messages are dropped.
- **No parent turn**: a Thread has at most one Turn running at any time.
- **Pipe scoping**: each Thread has its own pipe. No cross-Thread event leakage.
