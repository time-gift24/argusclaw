# argus-repl: Interactive Test CLI Design

**Date**: 2026-03-21
**Status**: Approved

## 1. Overview

`argus-repl` is an interactive REPL binary for end-to-end testing of the ArgusClaw system. It creates a session/thread, registers tools, and uses a mock LLM provider to simulate multi-turn conversations. All tool calls are auto-approved.

## 2. Goals

- End-to-end testing of the full conversation loop (session → thread → turn → LLM → tools → events)
- No real LLM API required — mock provider with fixed responses
- Interactive exploration of system behavior in real time
- Structured output (Reasoning / Summary / ToolCall) for clarity

## 3. Project Location

- **Binary**: `crates/argus-test-support/src/bin/argus-repl/main.rs`
- **Workspace**: Listed in `crates/argus-test-support/Cargo.toml` as `[[bin]]`
- **Dependencies**: `argus-wing` (public API), `argus-test-support` (mock providers), `argus-protocol`

## 4. Command-Line Interface

```bash
# Default (concise mode)
cargo run -p argus-test-support --bin argus-repl

# Verbose mode (shows token stats, event details)
cargo run -p argus-test-support --bin argus-repl -- --verbose

# Custom database path
cargo run -p argus-test-support --bin argus-repl -- --db /tmp/test.db
```

## 5. Mock Provider

Implements `LlmProvider` trait from `argus-protocol`. Returns fixed text responses in round-robin:

```rust
static REPL_MOCK_RESPONSES: &[&str] = &[
    "收到。这是第 1 轮对话。",
    "这是第 2 轮。上下文持续累积。",
    "这是第 3 轮。你的消息历史在增长。",
    "这是第 4 轮。我们可以继续测试。",
    "这是第 5 轮。上下文应该已经相当长了。",
];

// complete() returns the next response in sequence
// No tool calls — pure text responses only
```

## 6. Tool Registration

Registered tools (no ShellTool for safety):
- `ReadTool` — read file contents
- `GrepTool` — search file contents
- `GlobTool` — file pattern matching
- `HttpTool` — HTTP requests

**Approval**: All tools auto-approved via `ApprovalPolicy::auto_approve(true)`.

## 7. Session Management

Single-session mode:
- Auto-created on startup: `create_session("repl-session")` + `create_thread()`
- All user input accumulates in the same thread's message history
- Exit via `Ctrl+C` or `exit` command

## 8. Output Format

### Default (concise) mode

```
[Argus REPL] Session #1 created, thread: abc123
> 你好
[Summary] 收到。这是第 1 轮对话。
> 继续
[Summary] 这是第 2 轮。上下文持续累积。
> exit
[Argus REPL] Goodbye.
```

### Verbose mode

```
[Argus REPL] Session #1 created, thread: abc123
> 你好
[Event: BeforeCallLLM] firing 0 hooks
[Event: LLM call] model=mock
[Reasoning] 模型正在思考...
[Summary] 收到。这是第 1 轮对话。
[Event: TurnCompleted] tokens=42 input, 38 output, duration=1.2s
>
```

**Event types displayed**:
- `BeforeCallLLM` — hook firing (verbose)
- `LLM call` — model invocation (verbose)
- `Reasoning` — from `LlmStreamEvent::ReasoningDelta`
- `Summary` — from `LlmStreamEvent::SummaryDelta`
- `ToolCall` — from `LlmStreamEvent::ToolCall`
- `ToolCompleted` — tool execution result
- `TurnCompleted` — turn stats (verbose only)

## 9. Core Execution Flow

```
1. ArgusWing::init()           # Initialize DB, managers
2. Register tools              # Read, Grep, Glob, Http (no Shell)
3. Create session + thread     # Single auto-created session
4. REPL loop:
   ├── Read user input
   ├── send_message(session_id, thread_id, input)
   ├── subscribe() → consume ThreadEvent
   │     ├── Processing{ReasoningDelta} → print "[Reasoning] ..."
   │     ├── Processing{SummaryDelta}  → print "[Summary] ..."
   │     ├── Processing{ToolCall}       → print "[ToolCall] ..."
   │     ├── Processing{ToolCompleted}  → print "[ToolCompleted] ..."
   │     └── TurnCompleted              → print stats (verbose)
   └── auto-approve all tools
```

## 10. Event Mapping

`ThreadEvent` variants map to REPL output:

| ThreadEvent | REPL Output Tag | Verbose Only |
|---|---|---|
| `Processing{ReasoningDelta}` | `[Reasoning]` | No |
| `Processing{SummaryDelta}` | `[Summary]` | No |
| `Processing{ToolCall}` | `[ToolCall]` | No |
| `Processing{ToolCompleted}` | `[ToolCompleted]` | No |
| `Processing{ContentDelta}` | (ignored) | - |
| `TurnCompleted` | (stats line) | Yes |
| `TurnFailed` | `[Error]` | No |
| `WaitingForApproval` | `[Approval]` | No |
| `ApprovalResolved` | (logged) | Yes |

## 11. Implementation Notes

- Use `tracing` for internal logging, separate from REPL output
- The mock provider must implement `Send + Sync` for `Arc<dyn LlmProvider>`
- Thread event subscription uses `broadcast::channel(256)` with a dedicated task consuming events
- Database defaults to `~/.arguswing/sqlite.db`, overridable via `--db`
