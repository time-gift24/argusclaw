# argus-repl Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build an interactive REPL binary that creates a session/thread, registers tools, and uses a mock LLM provider to simulate multi-turn conversations for end-to-end testing.

**Architecture:** Standalone `crates/argus-repl` binary crate. Uses `ArgusWing::init()` for DB and manager setup, `ThreadBuilder` directly (bypassing `ProviderResolver`) to inject a mock `LlmProvider`, registers safe tools (ReadTool, GrepTool, GlobTool, HttpTool) with auto-approve policy, and runs a tokio async REPL loop consuming `ThreadEvent` broadcasts.

**Tech Stack:** Rust, tokio async runtime, `clap` for CLI args, `futures-util` for stream creation, `tracing` for internal logging.

---

## Chunk 1: Project Scaffold

**Files:**
- Create: `crates/argus-repl/Cargo.toml`
- Modify: `Cargo.toml` (add workspace member)

- [ ] **Step 1: Create `crates/argus-repl/Cargo.toml`**

```toml
[package]
name = "argus-repl"
version = "0.1.0"
edition = "2021"

[dependencies]
argus-wing = { path = "../argus-wing" }
argus-protocol = { path = "../argus-protocol" }
argus-session = { path = "../argus-session" }
argus-thread = { path = "../argus-thread" }
argus-tool = { path = "../argus-tool" }
argus-template = { path = "../argus-template" }
argus-approval = { path = "../argus-approval" }
argus-repository = { path = "../argus-repository" }
argus-llm = { path = "../argus-llm" }

tokio = { version = "1", features = ["full"] }
clap = { version = "4", features = ["derive"] }
futures-util = "0.3"
rust_decimal = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

- [ ] **Step 2: Add `crates/argus-repl` to workspace members in `Cargo.toml`**

Edit `Cargo.toml` at root, add `"crates/argus-repl"` to the `members` list.

- [ ] **Step 3: Verify build with `cargo build -p argus-repl 2>&1 | head -30`**

Expected: Compilation starts (likely fails on missing `src/main.rs`, which is fine for this step).

---

## Chunk 2: Mock Provider

**Files:**
- Create: `crates/argus-repl/src/mock_provider.rs`

The mock provider implements `LlmProvider` from `argus-protocol`, returning fixed text responses in round-robin via `stream_complete_with_tools()`. No tool calls are ever emitted.

- [ ] **Step 1: Create `crates/argus-repl/src/mock_provider.rs`**

```rust
//! Mock LLM provider for REPL testing.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError,
    LlmEventStream, LlmProvider, LlmStreamEvent, ModelMetadata, ProviderCapabilities,
    ToolCall, ToolCallDelta, ToolCompletionRequest, ToolCompletionResponse, ToolDefinition,
};
use futures_util::stream::{self, Iter};
use rust_decimal::Decimal;

static REPL_MOCK_RESPONSES: &[&str] = &[
    "收到。这是第 1 轮对话。",
    "这是第 2 轮。上下文持续累积。",
    "这是第 3 轮。你的消息历史在增长。",
    "这是第 4 轮。我们可以继续测试。",
    "这是第 5 轮。上下文应该已经相当长了。",
];

/// Mock provider for REPL testing.
#[derive(Debug, Clone)]
pub struct ReplMockProvider {
    counter: Arc<AtomicUsize>,
}

impl ReplMockProvider {
    #[must_use]
    pub fn new() -> Self {
        Self {
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn next_response(&self) -> String {
        let idx = self.counter.fetch_add(1, Ordering::SeqCst);
        REPL_MOCK_RESPONSES[idx % REPL_MOCK_RESPONSES.len()].to_string()
    }
}

impl Default for ReplMockProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmProvider for ReplMockProvider {
    fn model_name(&self) -> &str {
        "mock"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities { thinking: true }
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionResponse, LlmError> {
        let text = self.next_response();
        Ok(CompletionResponse {
            content: text,
            reasoning_content: None,
            finish_reason: FinishReason::Stop,
            input_tokens: 10,
            output_tokens: text.chars().count() as u32,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let text = self.next_response();
        Ok(ToolCompletionResponse {
            content: text,
            reasoning_content: None,
            tool_calls: vec![],
            finish_reason: FinishReason::Stop,
            input_tokens: 10,
            output_tokens: text.chars().count() as u32,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn stream_complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let text = self.next_response();
        let input_tokens = 10u32;
        let output_tokens = text.chars().count() as u32;

        // Build a one-shot stream: ContentDelta + Finished + Usage
        let events = vec![
            Ok(LlmStreamEvent::ContentDelta {
                delta: text.clone(),
            }),
            Ok(LlmStreamEvent::Finished {
                finish_reason: FinishReason::Stop,
            }),
            Ok(LlmStreamEvent::Usage {
                input_tokens,
                output_tokens,
            }),
        ];

        let stream: LlmEventStream = Box::pin(stream::iter(events));
        Ok(stream)
    }

    async fn model_metadata(&self) -> Result<ModelMetadata, LlmError> {
        Ok(ModelMetadata {
            id: "mock".to_string(),
            context_length: Some(128_000),
        })
    }
}
```

- [ ] **Step 2: Verify the file compiles**

Run: `cargo check -p argus-repl 2>&1`
Expected: Should succeed (no `main.rs` yet, but mock_provider.rs should compile cleanly).

---

## Chunk 3: Main REPL Binary

**Files:**
- Create: `crates/argus-repl/src/main.rs`
- Create: `crates/argus-repl/src/lib.rs` (optional re-export for testing)

This is the core of the REPL. It wires together: CLI args parsing, `ArgusWing::init()`, tool registration, session/thread creation with mock provider, and the async event-loop REPL.

- [ ] **Step 1: Create `crates/argus-repl/src/main.rs`**

```rust
//! argus-repl: Interactive REPL for end-to-end testing.

use std::sync::Arc;

use argus_approval::ApprovalPolicy;
use argus_protocol::{SessionId, ThreadEvent, ThreadId};
use argus_repl::mock_provider::ReplMockProvider;
use argus_thread::{KeepRecentCompactor, Thread, ThreadBuilder};
use argus_tool::{GlobTool, GrepTool, HttpTool, ReadTool, ToolManager};
use argus_wing::ArgusWing;
use clap::Parser;
use futures_util::StreamExt;
use tokio::{
    io::AsyncBufReadExt,
    signal::ctrl_c,
    sync::Mutex,
};

/// Parse command-line arguments.
#[derive(Debug, Parser)]
#[command(name = "argus-repl")]
#[command(about = "Interactive REPL for end-to-end ArgusClaw testing")]
struct Args {
    /// Enable verbose output (shows token stats and detailed events).
    #[arg(long)]
    verbose: bool,

    /// Custom database path.
    #[arg(long)]
    db: Option<String>,

    /// Enable debug logging.
    #[arg(long, default_value = "false")]
    debug: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize tracing
    let filter = if args.debug {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug"))
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();

    // Initialize ArgusWing
    let wing = ArgusWing::init(args.db.as_deref())
        .await
        .expect("ArgusWing::init failed");

    // Create tool manager with safe tools only (no ShellTool)
    let tool_manager = Arc::new({
        let tm = ToolManager::new();
        tm.register(Arc::new(ReadTool::new()));
        tm.register(Arc::new(GrepTool::new()));
        tm.register(Arc::new(GlobTool::new()));
        tm.register(Arc::new(HttpTool::new()));
        tm
    });

    // Get default template (for agent record)
    let template = wing
        .get_default_template()
        .await?
        .expect("Default template 'ArgusWing' not found");

    // Create session
    let session_id = wing
        .create_session("repl-session")
        .await
        .expect("Failed to create session");

    // Setup auto-approve policy
    let mut policy = ApprovalPolicy::default();
    policy.auto_approve = true;
    policy.apply_shorthands();

    // Create mock provider
    let mock_provider: Arc<dyn argus_protocol::llm::LlmProvider> =
        Arc::new(ReplMockProvider::new());

    // Get compactor
    let compactor = Arc::new(KeepRecentCompactor::with_defaults());

    // Build thread with ThreadBuilder directly (bypassing ProviderResolver)
    let thread = ThreadBuilder::new()
        .id(ThreadId::new())
        .session_id(session_id)
        .agent_record(Arc::new(template))
        .provider(mock_provider)
        .tool_manager(tool_manager)
        .compactor(compactor)
        .config(argus_thread::ThreadConfig::default())
        .build()
        .expect("ThreadBuilder::build failed");

    let thread = Arc::new(Mutex::new(thread));
    let thread_id = thread.lock().await.id();

    println!(
        "[Argus REPL] Session #{} created, thread: {}",
        session_id.inner(),
        thread_id
    );

    // Subscribe to thread events
    let mut event_rx = {
        let t = thread.lock().await;
        t.subscribe()
    };

    // Flag to exit REPL
    let should_exit = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let should_exit_clone = should_exit.clone();

    // Spawn event consumer task
    let event_handle = tokio::spawn(async move {
        let mut content_buffer = String::new();
        while let Ok(event) = event_rx.recv().await {
            match &event {
                ThreadEvent::Processing { event: llm_event, .. } => {
                    match llm_event {
                        argus_protocol::llm::LlmStreamEvent::ReasoningDelta { delta } => {
                            println!("[Reasoning] {}", delta);
                        }
                        argus_protocol::llm::LlmStreamEvent::ContentDelta { delta } => {
                            content_buffer.push_str(delta);
                        }
                        argus_protocol::llm::LlmStreamEvent::ToolCallDelta { .. } => {
                            // Mock never emits this, but handle it anyway
                        }
                        argus_protocol::llm::LlmStreamEvent::Usage { .. } => {
                            // Usage events ignored in default mode
                        }
                        argus_protocol::llm::LlmStreamEvent::Finished { .. } => {
                            if !content_buffer.is_empty() {
                                println!("[Content] {}", content_buffer);
                                content_buffer.clear();
                            }
                        }
                        argus_protocol::llm::LlmStreamEvent::RetryAttempt {
                            attempt,
                            max_retries,
                            error,
                        } => {
                            if args.verbose {
                                println!("[Retry] {}/{}: {}", attempt, max_retries, error);
                            }
                        }
                    }
                }
                ThreadEvent::ToolStarted { tool_name, .. } => {
                    if args.verbose {
                        println!("[ToolStarted] {}", tool_name);
                    }
                }
                ThreadEvent::ToolCompleted { tool_name, result, .. } => {
                    let truncated = if result.len() > 200 {
                        format!("{}...", &result[..200])
                    } else {
                        result.clone()
                    };
                    println!("[ToolCompleted] {}: {}", tool_name, truncated);
                }
                ThreadEvent::TurnCompleted { token_usage, .. } => {
                    if args.verbose {
                        println!(
                            "[Event: TurnCompleted] tokens={} input, {} output",
                            token_usage.input_tokens, token_usage.output_tokens
                        );
                    }
                }
                ThreadEvent::TurnFailed { error, .. } => {
                    println!("[Error] {}", error);
                }
                ThreadEvent::WaitingForApproval { request } => {
                    println!("[Approval] {} pending", request.tool_name);
                }
                _ => {
                    // ApprovalResolved, Idle, Compacted — ignored
                }
            }
        }
    });

    // REPL input loop
    let stdin = tokio::io::stdin();
    let mut lines = tokio::io::BufReader::new(stdin).lines();

    // Also listen for Ctrl+C
    tokio::spawn(async move {
        ctrl_c().await.ok();
        should_exit_clone.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    loop {
        if should_exit.load(std::sync::atomic::Ordering::SeqCst) {
            println!("\n[Argus REPL] Goodbye.");
            break;
        }

        print!("> ");
        tokio::io::AsyncWriteExt::write_all(
            &mut tokio::io::stdout(),
            b"> ",
        )
        .await
        .ok();
        tokio::io::AsyncWriteExt::flush(&mut tokio::io::stdout())
        .await
        .ok();

        let line = lines.next_line().await;
        let line = match line {
            Ok(Some(l)) => l,
            Ok(None) => break,
            Err(_) => break,
        };

        let input = line.trim();
        if input.is_empty() || input == "exit" {
            println!("[Argus REPL] Goodbye.");
            break;
        }

        // Send message
        let mut t = thread.lock().await;
        if let Err(e) = t.send_message(input.to_string(), None).await {
            println!("[Error] send_message failed: {}", e);
        }
    }

    // Wait for event consumer
    event_handle.abort();

    Ok(())
}
```

**Note on tool registration:** Create a fresh `ToolManager::new()` directly in `main.rs` and register only safe tools (Read, Grep, Glob, Http). Do NOT call `wing.register_default_tools()` — that adds ShellTool which we want to exclude.

- [ ] **Step 2: Create `crates/argus-repl/src/lib.rs`**

```rust
//! argus-repl library.

pub mod mock_provider;
```

- [ ] **Step 3: Build and fix compilation errors**

Run: `cargo build -p argus-repl 2>&1`
Expected: Some compilation errors — fix them iteratively until clean.

Common issues to watch for:
- `Arc<dyn LlmProvider>` — the trait already has `Send + Sync` bounds, so this works
- `broadcast::channel` — no manual channel needed, use `thread.subscribe()` directly
- `ThreadConfig::default()` — check the import from `argus-thread`
- `ctrl_c()` — needs `tokio::signal::ctrl_c()` from tokio
- If `ApprovalPolicy` is needed at runtime, wire it into the thread's `HookRegistry` — but since all tools are auto-approved, the `ApprovalPolicy` only needs to be set on the `ApprovalManager` (via `wing.approval_manager()`) if needed
- The `ApprovalPolicy` auto-approve flag clears `require_approval` so no `WaitingForApproval` events should fire; if they do, check `ApprovalManager::update_policy()`

- [ ] **Step 4: Run and test the REPL**

Run: `cargo run -p argus-repl -- --debug`
Expected: Shows startup message with session/thread IDs, accepts input, prints structured output.

Try typing a message and pressing Enter. The mock provider should respond with round-robin fixed text.

- [ ] **Step 5: Test verbose mode**

Run: `cargo run -p argus-repl -- --verbose`
Expected: Shows `[Event: LLM call]`, token stats after each turn, etc.

- [ ] **Step 6: Test Ctrl+C and exit**

Verify `exit` command and `Ctrl+C` both exit cleanly.

---

## Chunk 4: Polish and Verification

**Files:**
- Modify: `crates/argus-repl/src/main.rs` (fix issues from compilation)

- [ ] **Step 1: Test multiple turns (round-robin)**

Type 5+ messages and verify the responses cycle through the 5 mock responses and wrap.

- [ ] **Step 2: Run `cargo clippy -p argus-repl 2>&1` and fix warnings**

- [ ] **Step 3: Run `prek` (static analysis)**

Run: `prek` at project root. Fix any issues reported.

- [ ] **Step 4: Commit**

```bash
git add crates/argus-repl/ Cargo.toml
git commit -m "feat(argus-repl): add interactive REPL for end-to-end testing

- New standalone argus-repl binary crate
- Mock LLM provider with round-robin fixed responses
- Registers safe tools (Read, Grep, Glob, Http)
- Structured REPL output: [Reasoning], [Content], [ToolCompleted]
- Verbose mode with token stats
- Supports --verbose, --db, --debug flags

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Key Reference Files

| Purpose | Path |
|---------|------|
| LlmProvider trait | `crates/argus-protocol/src/llm/mod.rs:556` |
| LlmStreamEvent enum | `crates/argus-protocol/src/llm/mod.rs:388` |
| ThreadBuilder usage | `crates/argus-thread/src/thread.rs:103` |
| Thread::subscribe | `crates/argus-thread/src/thread.rs:203` |
| Thread::send_message | `crates/argus-thread/src/thread.rs:258` |
| ApprovalPolicy | `crates/argus-approval/src/policy.rs` |
| KeepRecentCompactor | `crates/argus-thread/src/compact.rs` |
| ToolManager (tools) | `crates/argus-tool/src/lib.rs` |
| ArgusWing::init | `crates/argus-wing/src/lib.rs:89` |
| TokenUsage fields | `crates/argus-protocol/src/token_usage.rs:7` |
