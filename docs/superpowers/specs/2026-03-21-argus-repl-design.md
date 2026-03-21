# argus-repl: Interactive Test CLI Design

**Date**: 2026-03-21
**Status**: Approved (revised)

## 1. Overview

`argus-repl` is an interactive REPL binary for end-to-end testing of the ArgusClaw system. It creates a session/thread, registers tools, and uses a mock LLM provider to simulate multi-turn conversations. All tool calls are auto-approved.

## 2. Goals

- End-to-end testing of the full conversation loop (session → thread → turn → LLM → tools → events)
- No real LLM API required — mock provider with fixed responses
- Interactive exploration of system behavior in real time
- Structured output (Reasoning / Content / ToolCall) for clarity

## 3. Project Location

- **Crate**: `crates/argus-repl/` (new standalone binary crate)
- **Binary entry**: `crates/argus-repl/src/main.rs`
- **Workspace**: Added to root `Cargo.toml` members
- **Dependencies**: `argus-wing`, `argus-protocol`, `argus-session`, `argus-thread`, `argus-tool`, `argus-repository`, `argus-approval`, `argus-template`, `argus-llm`

Note: `argus-wing` cannot be added to `argus-test-support` because it would create a circular dependency (`argus-wing` depends on `argus-test-support`). The REPL is therefore a standalone crate.

## 4. Command-Line Interface

```bash
# Default (concise mode, shows structured output only)
cargo run -p argus-repl

# Verbose mode (shows detailed events, token stats)
cargo run -p argus-repl -- --verbose

# Custom database path
cargo run -p argus-repl -- --db /tmp/test.db
```

## 5. Mock Provider

Implements `LlmProvider` trait from `argus-protocol`. Returns fixed text responses in round-robin using the streaming API:

```rust
static REPL_MOCK_RESPONSES: &[&str] = &[
    "收到。这是第 1 轮对话。",
    "这是第 2 轮。上下文持续累积。",
    "这是第 3 轮。你的消息历史在增长。",
    "这是第 4 轮。我们可以继续测试。",
    "这是第 5 轮。上下文应该已经相当长了。",
];
```

**Streaming implementation**: `stream_complete_with_tools()` yields `LlmStreamEvent::ContentDelta` events with text chunks. The mock accumulates and emits all text in a single `ContentDelta` (no streaming), wrapped in a `Finished` event.

**No tool calls**: `ToolCallDelta` is never emitted. The mock is pure text only.

**Round-robin behavior**: After the last response, wraps back to the first.

**Requirement**: Must implement `Send + Sync` to satisfy `Arc<dyn LlmProvider>`.

### Wiring Mock Provider into ProviderResolver

The session system resolves providers through `ProviderResolver` → `ProviderManager` → database. To inject the mock:

1. Create a `LlmProviderRecord` in the database via `wing.upsert_provider()` during REPL init
2. Wrap the mock provider in a struct that implements `LlmProvider` and stores the mock internally
3. The session will resolve the provider by ID from the DB, then the mock's `complete`/`stream_complete_with_tools` methods are called

Alternatively (simpler): create the thread manually with `ThreadBuilder`, passing the mock provider directly, bypassing `ProviderResolver` entirely. This avoids DB complexity for a REPL.

**Chosen approach**: Use `ThreadBuilder` directly — create session from DB, but build thread manually with mock provider injected. This avoids modifying `ArgusWing` or `ProviderManager`.

## 6. Tool Registration

Registered tools (no ShellTool for safety):
- `ReadTool` — read file contents
- `GrepTool` — search file contents
- `GlobTool` — file pattern matching
- `HttpTool` — HTTP requests

**Approval**: All tools auto-approved by setting `ApprovalPolicy`:

```rust
let mut policy = ApprovalPolicy::default();
policy.auto_approve = true;
policy.apply_shorthands();
```

The `auto_approve` shorthand clears the `require_approval` list when `apply_shorthands()` is called, so no tool will ever trigger `WaitingForApproval`.

## 7. Session Management

Single-session mode:
- Create session via `wing.create_session("repl-session")` — persists to DB
- Build thread via `ThreadBuilder` directly, injecting mock provider
- Thread is NOT persisted to DB in this approach (in-memory only) — acceptable for a REPL
- All user input accumulates in the thread's message history
- Exit via `Ctrl+C` (tokio signal) or `exit` command

## 8. Output Format

### Default (concise) mode

```
[Argus REPL] Session #1 created, thread: abc123
> 你好
[Content] 收到。这是第 1 轮对话。
> 继续
[Content] 这是第 2 轮。上下文持续累积。
> exit
[Argus REPL] Goodbye.
```

### Verbose mode

```
[Argus REPL] Session #1 created, thread: abc123
> 你好
[Event: LLM call] model=mock
[Reasoning] 模型正在思考...
[Content] 收到。这是第 1 轮对话。
[Event: TurnCompleted] tokens=42 input, 38 output
>
```

**Event types displayed**:
- `Event: LLM call` — at start of each turn (verbose)
- `Reasoning` — from `LlmStreamEvent::ReasoningDelta`
- `Content` — from `LlmStreamEvent::ContentDelta` (accumulates into final text)
- `ToolCall` — from `LlmStreamEvent::ToolCallDelta` (mock never emits this)
- `ToolCompleted` — from `ThreadEvent::ToolCompleted`
- `Event: TurnCompleted` — from `ThreadEvent::TurnCompleted`, with token stats (verbose)
- `Error` — from `ThreadEvent::TurnFailed`

## 9. Core Execution Flow

```
1. ArgusWing::init()           # Initialize DB, managers
2. Register tools              # Read, Grep, Glob, Http (no Shell)
3. Create session              # wing.create_session("repl-session")
4. Build thread manually       # ThreadBuilder with mock provider injected
5. REPL loop:
   ├── Read user input (line-oriented, trim newline)
   │     └── If empty or "exit", break
   ├── send_message(session_id, thread_id, input)
   ├── subscribe() → consume ThreadEvent
   │     ├── ThreadEvent::Processing { event: LlmStreamEvent::ReasoningDelta { delta } }
   │     │     → print "[Reasoning] {delta}"
   │     ├── ThreadEvent::Processing { event: LlmStreamEvent::ContentDelta { delta } }
   │     │     → accumulate text, no immediate print (buffer until Finished)
   │     ├── ThreadEvent::Processing { event: LlmStreamEvent::ToolCallDelta { .. } }
   │     │     → print "[ToolCall] {name}"
   │     ├── ThreadEvent::ToolStarted { tool_name, arguments }
   │     │     → (verbose) print "[ToolStarted] {tool_name}"
   │     ├── ThreadEvent::ToolCompleted { tool_name, result }
   │     │     → print "[ToolCompleted] {tool_name}: {truncated_result}"
   │     ├── ThreadEvent::Processing { event: LlmStreamEvent::Finished { .. } }
   │     │     → print accumulated "[Content] {full_text}"
   │     └── ThreadEvent::TurnCompleted { token_usage, .. }
   │           → (verbose) print token stats
   └── Auto-approve all tools via ApprovalPolicy
```

## 10. Event Mapping

| ThreadEvent Variant | REPL Output Tag | Verbose Only |
|---|---|---|
| `ThreadEvent::Processing { event: LlmStreamEvent::ReasoningDelta { delta } }` | `[Reasoning] {delta}` | No |
| `ThreadEvent::Processing { event: LlmStreamEvent::ContentDelta { delta } }` | (buffered until Finished) | No |
| `ThreadEvent::Processing { event: LlmStreamEvent::ToolCallDelta { name, .. } }` | `[ToolCall] {name}` | No |
| `ThreadEvent::Processing { event: LlmStreamEvent::Usage { .. } }` | (ignored) | Yes |
| `ThreadEvent::Processing { event: LlmStreamEvent::Finished { finish_reason } }` | `[Content] {accumulated_text}` | No |
| `ThreadEvent::Processing { event: LlmStreamEvent::RetryAttempt { attempt, max_retries, error } }` | `[Retry] {attempt}/{max_retries}: {error}` | Yes |
| `ThreadEvent::ToolStarted { tool_name, arguments }` | `[ToolStarted] {tool_name}` | Yes |
| `ThreadEvent::ToolCompleted { tool_name, result }` | `[ToolCompleted] {tool_name}: {truncated}` | No |
| `ThreadEvent::TurnCompleted { token_usage, .. }` | `[Event: TurnCompleted] tokens={input} input, {output} output` | Yes |
| `ThreadEvent::TurnFailed { error }` | `[Error] {error}` | No |
| `ThreadEvent::WaitingForApproval { request }` | `[Approval] {request.tool_name} pending` | No |
| `ThreadEvent::ApprovalResolved { request_id, decision }` | (ignored) | Yes |
| `ThreadEvent::Idle { .. }` | (ignored) | - |
| `ThreadEvent::Compacted { .. }` | (ignored) | Yes |

**Content buffering**: `ContentDelta` events are accumulated in a `String` buffer. On `Finished`, print `[Content] {buffer}`. On `TurnFailed`, discard buffer.

## 11. Implementation Notes

- Use `tracing` for internal REPL logging (distinct from stdout output)
- Thread event subscription uses `broadcast::channel(256)` with a dedicated async task
- Database defaults to `~/.arguswing/sqlite.db`, overridable via `--db` CLI arg
- Mock responses wrap after 5 turns (round-robin)
- `Ctrl+C` handling via `tokio::signal::ctrl_c()` in the REPL loop
- Empty input (blank line) is skipped without sending a turn
