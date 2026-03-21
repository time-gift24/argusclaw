# Turn Trace Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement turn-level persistence and audit logging in argus-turn by writing each LLM iteration's request/response + tool executions to JSON trace files.

**Architecture:** TraceWriter creates JSON files per thread/turn at `traces/{thread_id}/{turn_number}.json`. Each LLM call produces an iteration record. Tool executions are recorded per iteration. The trace file is finalized when the turn completes.

**Tech Stack:** Rust, serde_json, std::fs, chrono (for timestamps)

---

## Task 1: Create trace.rs with basic types

**Files:**
- Create: `crates/argus-turn/src/trace.rs`
- Modify: `crates/argus-turn/src/lib.rs`

**Step 1: Create trace.rs with data structures**

```rust
//! Turn execution trace - iteration-level audit logging.

use serde::Serialize;
use std::path::PathBuf;

/// Configuration for trace recording.
#[derive(Debug, Clone)]
pub struct TraceConfig {
    /// Whether tracing is enabled.
    pub enabled: bool,
    /// Directory where trace files are written.
    pub trace_dir: PathBuf,
}

impl TraceConfig {
    /// Create a new TraceConfig.
    pub fn new(enabled: bool, trace_dir: PathBuf) -> Self {
        Self { enabled, trace_dir }
    }

    /// Create a disabled TraceConfig (no tracing).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            trace_dir: PathBuf::new(),
        }
    }
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self::disabled()
    }
}

/// LLM request captured for a single iteration.
#[derive(Debug, Clone, Serialize)]
pub struct LlmRequest {
    pub messages: Vec<serde_json::Value>,
    pub tools: Vec<serde_json::Value>,
}

/// LLM response captured for a single iteration.
#[derive(Debug, Clone, Serialize)]
pub struct LlmResponse {
    pub content: Option<String>,
    pub reasoning_content: Option<String>,
    pub tool_calls: Vec<serde_json::Value>,
    pub finish_reason: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Single tool execution result.
#[derive(Debug, Clone, Serialize)]
pub struct ToolExecution {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub result: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}

/// A single iteration's record (one LLM call + tool executions).
#[derive(Debug, Clone, Serialize)]
pub struct IterationRecord {
    pub iteration: u32,
    pub llm_request: LlmRequest,
    pub llm_response: LlmResponse,
    pub tools: Vec<ToolExecution>,
}

/// The final output of a turn.
#[derive(Debug, Clone, Serialize)]
pub struct FinalOutput {
    pub token_usage: TokenUsageRecord,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenUsageRecord {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

/// The complete trace file structure.
#[derive(Debug, Clone, Serialize)]
pub struct TraceFile {
    pub version: String,
    pub thread_id: String,
    pub turn_number: u32,
    pub start_time: String,
    pub end_time: Option<String>,
    pub iterations: Vec<IterationRecord>,
    pub final_output: Option<FinalOutput>,
}
```

**Step 2: Run cargo check to verify compilation**

Run: `cd /Users/wanyaozhong/projects/argusclaw && cargo check -p argus-turn`
Expected: No errors

**Step 3: Export from lib.rs**

Modify `crates/argus-turn/src/lib.rs` to add:
```rust
pub mod trace;
pub use trace::{TraceConfig, IterationRecord, ToolExecution, LlmRequest, LlmResponse};
```

Run: `cargo check -p argus-turn`
Expected: No errors

**Step 4: Commit**

```bash
git add crates/argus-turn/src/trace.rs crates/argus-turn/src/lib.rs
git commit -m "feat(argus-turn): add trace module with basic data structures"
```

---

## Task 2: Implement TraceWriter

**Files:**
- Modify: `crates/argus-turn/src/trace.rs`

**Step 1: Add TraceWriter struct and creation logic**

Add to trace.rs:

```rust
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::time::Instant;

pub struct TraceWriter {
    file: BufWriter<File>,
    thread_id: String,
    turn_number: u32,
    start_time: Instant,
    iterations: Vec<IterationRecord>,
}

impl TraceWriter {
    /// Create a new TraceWriter for the given thread and turn.
    pub fn new(thread_id: &str, turn_number: u32, config: &TraceConfig) -> std::io::Result<Self> {
        let dir = config.trace_dir.join(thread_id);
        fs::create_dir_all(&dir)?;

        let file_path = dir.join(format!("{}.json", turn_number));
        let file = File::create(file_path)?;
        let writer = BufWriter::new(file);

        Ok(Self {
            file: writer,
            thread_id: thread_id.to_string(),
            turn_number,
            start_time: Instant::now(),
            iterations: Vec::new(),
        })
    }

    /// Write an iteration record.
    pub fn write_iteration(&mut self, iteration: IterationRecord) -> std::io::Result<()> {
        self.iterations.push(iteration);
        Ok(())
    }

    /// Finalize the trace file.
    pub fn finish(mut self, token_usage: &argus_protocol::TokenUsage) -> std::io::Result<()> {
        let trace = TraceFile {
            version: "1.0".to_string(),
            thread_id: self.thread_id,
            turn_number: self.turn_number,
            start_time: format!("{:?}", self.start_time.elapsed()),
            end_time: None,
            iterations: self.iterations,
            final_output: Some(FinalOutput {
                token_usage: TokenUsageRecord {
                    input_tokens: token_usage.input_tokens,
                    output_tokens: token_usage.output_tokens,
                    total_tokens: token_usage.total_tokens,
                },
            }),
        };

        serde_json::to_writer(&mut self.file, &trace)?;
        self.file.flush()?;
        Ok(())
    }
}
```

**Step 2: Run cargo check**

Run: `cargo check -p argus-turn`
Expected: No errors

**Step 3: Commit**

```bash
git add crates/argus-turn/src/trace.rs
git commit -m "feat(argus-turn): implement TraceWriter for iteration recording"
```

---

## Task 3: Add chrono dependency and update timestamps

**Files:**
- Modify: `crates/argus-turn/Cargo.toml`
- Modify: `crates/argus-turn/src/trace.rs`

**Step 1: Add chrono to Cargo.toml**

Add under `[dependencies]`:
```toml
chrono = { version = "0.4", features = ["serde"] }
```

**Step 2: Update trace.rs to use chrono timestamps**

Modify `TraceFile`:
```rust
use chrono::{DateTime, Utc};

pub struct TraceFile {
    pub version: String,
    pub thread_id: String,
    pub turn_number: u32,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub iterations: Vec<IterationRecord>,
    pub final_output: Option<FinalOutput>,
}
```

Update `TraceWriter::finish`:
```rust
pub fn finish(mut self, token_usage: &argus_protocol::TokenUsage) -> std::io::Result<()> {
    let trace = TraceFile {
        version: "1.0".to_string(),
        thread_id: self.thread_id,
        turn_number: self.turn_number,
        start_time: Utc::now(),
        end_time: Some(Utc::now()),
        iterations: self.iterations,
        final_output: Some(FinalOutput {
            token_usage: TokenUsageRecord {
                input_tokens: token_usage.input_tokens,
                output_tokens: token_usage.output_tokens,
                total_tokens: token_usage.total_tokens,
            },
        }),
    };
    // ... rest same
}
```

**Step 3: Run cargo check**

Run: `cargo check -p argus-turn`
Expected: No errors

**Step 4: Commit**

```bash
git add crates/argus-turn/Cargo.toml crates/argus-turn/src/trace.rs
git commit -m "feat(argus-turn): add chrono for timestamp handling in traces"
```

---

## Task 4: Integrate TraceWriter into TurnBuilder

**Files:**
- Modify: `crates/argus-turn/src/turn.rs`

**Step 1: Add trace_config field to Turn struct**

Find the Turn struct in `turn.rs` and add:
```rust
#[builder(default)]
trace_config: Option<TraceConfig>,
```

And add import at top:
```rust
use super::trace::{TraceWriter, TraceConfig};
```

**Step 2: Add trace_writer field to Turn struct**

```rust
#[builder(default)]
trace_writer: Option<TraceWriter>,
```

**Step 3: Update TurnBuilder to accept trace_config**

Add to TurnBuilder impl:
```rust
/// Set trace configuration.
pub fn trace_config(mut self, config: TraceConfig) -> Self {
    self.trace_config = Some(config);
    self
}
```

**Step 4: Create TraceWriter in build() if configured**

Find the build() method in TurnBuilder (generated by derive_builder) and add logic. Since TurnBuilder is auto-generated, we need to use a builder pattern. Instead, modify `Turn::execute()` to create the writer.

**Step 5: Create TraceWriter at start of execute()**

In `Turn::execute()` (around line 276), after the forwarder spawn:

```rust
// Create trace writer if configured
let trace_writer = self.trace_config.as_ref()
    .filter(|c| c.enabled)
    .map(|c| {
        TraceWriter::new(&self.thread_id, self.turn_number, c)
            .map_err(|e| TurnError::Other(e.to_string()))
    })
    .transpose()?;
```

**Step 6: Run cargo check**

Run: `cargo check -p argus-turn`
Expected: No errors (may have warnings about unused fields)

**Step 7: Commit**

```bash
git add crates/argus-turn/src/turn.rs
git commit -m "feat(argus-turn): add TraceWriter integration to Turn"
```

---

## Task 5: Record iterations in execute_loop()

**Files:**
- Modify: `crates/argus-turn/src/turn.rs`

**Step 1: Add iteration recording after each LLM response**

In `execute_loop()` (around line 572), after `process_finish_reason`, we need to record the iteration. First, refactor to capture the request/response for tracing:

```rust
// Before calling LLM, capture the request
let request_for_trace = LlmRequest {
    messages: messages.iter().map(|m| serde_json::to_value(m).unwrap()).collect(),
    tools: tools.iter().map(|t| serde_json::to_value(t.definition()).unwrap()).collect(),
};

// Call LLM
let response = self.call_llm_streaming(request).await?;

// After getting response, create iteration record
let iteration_record = IterationRecord {
    iteration: iteration as u32,
    llm_request: request_for_trace,
    llm_response: LlmResponse {
        content: response.content.clone(),
        reasoning_content: response.reasoning_content.clone(),
        tool_calls: response.tool_calls.iter().map(|tc| serde_json::to_value(tc).unwrap()).collect(),
        finish_reason: format!("{:?}", response.finish_reason),
        input_tokens: response.input_tokens,
        output_tokens: response.output_tokens,
    },
    tools: Vec::new(), // Will be filled after tool execution
};
```

**Step 2: Record tool executions**

After tools are executed (around line 654), add tool results to the iteration record:

```rust
// After adding tool results to messages, also record for trace
let mut tool_executions = Vec::new();
for result in &tool_results {
    tool_executions.push(ToolExecution {
        id: result.tool_call_id.clone(),
        name: result.name.clone(),
        arguments: serde_json::Value::Null, // TODO: capture original args
        result: result.content.clone(),
        duration_ms: 0, // TODO: measure actual duration
        error: None,
    });
}
iteration_record.tools = tool_executions;
```

**Step 3: Write iteration to trace_writer**

After creating the iteration_record:

```rust
if let Some(ref mut writer) = trace_writer {
    let _ = writer.write_iteration(iteration_record);
}
```

**Step 4: Run cargo check**

Run: `cargo check -p argus-turn`
Expected: No errors

**Step 5: Commit**

```bash
git add crates/argus-turn/src/turn.rs
git commit -m "feat(argus-turn): record iterations in execute_loop"
```

---

## Task 6: Finalize trace on turn completion

**Files:**
- Modify: `crates/argus-turn/src/turn.rs`

**Step 1: Write final output when turn completes**

In `Turn::execute()` (around line 345), before returning result:

```rust
// Finalize trace
if let Some(mut writer) = trace_writer {
    if let Ok(ref output) = result {
        let _ = writer.finish(&output.token_usage);
    }
}
```

**Step 2: Pass trace_writer through the execution**

Since `execute()` consumes `self`, we need to move trace_writer into execute() or store it in the struct. The simpler approach is to create it at execute() start and pass it through.

**Step 3: Run cargo check**

Run: `cargo check -p argus-turn`
Expected: No errors

**Step 4: Commit**

```bash
git add crates/argus-turn/src/turn.rs
git commit -m "feat(argus-turn): finalize trace on turn completion"
```

---

## Task 7: Add unit tests for TraceWriter

**Files:**
- Create: `crates/argus-turn/tests/trace_test.rs`
- Modify: `crates/argus-turn/Cargo.toml`

**Step 1: Add tempfile dev dependency**

Add to `[dev-dependencies]` in Cargo.toml:
```toml
tempfile = "3"
```

**Step 2: Write tests for TraceWriter**

```rust
use argus_turn::trace::{TraceWriter, TraceConfig, IterationRecord, LlmRequest, LlmResponse, ToolExecution};
use std::fs;

#[tokio::test]
async fn test_trace_writer_creates_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config = TraceConfig::new(true, temp_dir.path().to_path_buf());

    let mut writer = TraceWriter::new("thread-1", 1, &config).unwrap();

    let iteration = IterationRecord {
        iteration: 0,
        llm_request: LlmRequest {
            messages: vec![],
            tools: vec![],
        },
        llm_response: LlmResponse {
            content: Some("Hello".to_string()),
            reasoning_content: None,
            tool_calls: vec![],
            finish_reason: "stop".to_string(),
            input_tokens: 10,
            output_tokens: 5,
        },
        tools: vec![],
    };

    writer.write_iteration(iteration).unwrap();

    let token_usage = argus_protocol::TokenUsage {
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
    };
    writer.finish(&token_usage).unwrap();

    let trace_path = temp_dir.path().join("thread-1").join("1.json");
    assert!(trace_path.exists());

    let content = fs::read_to_string(&trace_path).unwrap();
    assert!(content.contains("\"iteration\":0"));
    assert!(content.contains("\"content\":\"Hello\""));
}
```

**Step 3: Run tests**

Run: `cargo test -p argus-turn trace`
Expected: PASS

**Step 4: Commit**

```bash
git add crates/argus-turn/tests/trace_test.rs crates/argus-turn/Cargo.toml
git commit -m "test(argus-turn): add TraceWriter unit tests"
```

---

## Task 8: Add integration test with actual Turn execution

**Files:**
- Create: `crates/argus-turn/tests/trace_integration_test.rs`

**Step 1: Write integration test**

```rust
use argus_turn::{Turn, TurnBuilder, TurnConfig};
use argus_protocol::llm::{ChatMessage, Role};
use argus_protocol::{AgentId, AgentRecord, ProviderId};
use argus_test_support::FakeProvider;
use std::sync::Arc;

#[tokio::test]
async fn test_turn_produces_trace_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let trace_config = argus_turn::trace::TraceConfig::new(
        true,
        temp_dir.path().to_path_buf(),
    );

    let provider = Arc::new(FakeProvider::new());
    let agent_record = Arc::new(AgentRecord {
        id: AgentId::new(1),
        display_name: "Test".to_string(),
        description: "Test agent".to_string(),
        version: "1.0".to_string(),
        provider_id: Some(ProviderId::new(1)),
        system_prompt: "You are a test agent.".to_string(),
        tool_names: vec![],
        max_tokens: None,
        temperature: None,
        thinking_config: None,
    });

    let (stream_tx, _) = tokio::sync::broadcast::channel(256);
    let (thread_event_tx, _) = tokio::sync::broadcast::channel(256);

    let turn = TurnBuilder::default()
        .turn_number(1)
        .thread_id("test-thread".to_string())
        .messages(vec![ChatMessage::user("Hello")])
        .provider(provider)
        .tools(vec![])
        .hooks(vec![])
        .config(TurnConfig::default())
        .agent_record(agent_record)
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .trace_config(trace_config)
        .build()
        .unwrap();

    let output = turn.execute().await.unwrap();

    // Verify trace file exists
    let trace_path = temp_dir.path().join("test-thread").join("1.json");
    assert!(trace_path.exists(), "Trace file should exist at {:?}", trace_path);

    // Verify trace content
    let content = std::fs::read_to_string(&trace_path).unwrap();
    assert!(content.contains("\"turn_number\":1"));
    assert!(content.contains("\"iteration\":0"));
}
```

**Step 2: Run integration test**

Run: `cargo test -p argus-turn trace_integration -- --nocapture`
Expected: PASS (may need FakeProvider setup)

**Step 3: Commit**

```bash
git add crates/argus-turn/tests/trace_integration_test.rs
git commit -m "test(argus-turn): add integration test for trace generation"
```

---

## Summary

| Task | Description | Files Modified |
|------|-------------|----------------|
| 1 | Create trace.rs with data structures | trace.rs, lib.rs |
| 2 | Implement TraceWriter | trace.rs |
| 3 | Add chrono for timestamps | Cargo.toml, trace.rs |
| 4 | Integrate TraceWriter into TurnBuilder | turn.rs |
| 5 | Record iterations in execute_loop | turn.rs |
| 6 | Finalize trace on completion | turn.rs |
| 7 | Unit tests for TraceWriter | tests/trace_test.rs, Cargo.toml |
| 8 | Integration test | tests/trace_integration_test.rs |

Total: 8 tasks
