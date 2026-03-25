# Turn Log 增量记录实现计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 turn-log 从内存累积+结束写文件改为 append-only JSONL 增量写入，支持恢复；SessionId/ThreadId 升级为 UUIDv7。

**Architecture:**
- `TraceWriter` 从 `BufWriter<File>` (sync) 改为 `tokio::fs::File` (async append)
- JSONL 每事件一行直接落盘，不在内存累积
- 恢复 API 返回 `impl Stream<Item = Result<TurnLogEvent, TurnLogError>>`
- SessionId 从 `i64` 改为 `Uuid` wrapper，ThreadId 使用 `Uuid::now_v7()`

**Tech Stack:** Rust (tokio async I/O), serde_json, uuid v7, thiserror

---

## Chunk 1: Foundation — Workspace & ID Types

### Chunk 1.1: UUID v7 Feature

**Files:**
- Modify: `Cargo.toml` (workspace)

- [ ] **Step 1: 添加 uuid v7 feature**

```toml
# Cargo.toml workspace
uuid = { version = "1", features = ["serde", "v4", "v7"] }
```

- [ ] **Step 2: 验证编译**

Run: `cargo check -p argus-protocol 2>&1 | tail -5`
Expected: 编译成功，无 warnings

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore: add uuid v7 feature"
```

### Chunk 1.2: ThreadId → UUIDv7

**Files:**
- Modify: `crates/argus-protocol/src/ids.rs:22-26`

- [ ] **Step 1: 修改 ThreadId::new() 使用 Uuid::now_v7()**

```rust
// crates/argus-protocol/src/ids.rs

impl ThreadId {
    /// Create a new ThreadId using UUIDv7 (time-sortable).
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}
```

- [ ] **Step 2: 验证编译**

Run: `cargo check -p argus-protocol 2>&1 | tail -5`
Expected: 编译成功

- [ ] **Step 3: 检查所有 ThreadId 用法兼容 UUIDv7**

Run: `cargo check 2>&1 | grep -i "threadid\|Uuid\|v4\|v7" | grep -i "error\|mismatch" | head -20`
Expected: 无类型不匹配错误

- [ ] **Step 4: 验证 UUIDv7 时间可排序性**

```rust
// 临时测试 - 不提交
fn test_uuidv7_sortable() {
    let ids: Vec<_> = (0..100)
        .map(|_| uuid::Uuid::now_v7().to_string())
        .collect();
    // 验证每生成一个休息 1ms 保证时间递增
    for i in 1..ids.len() {
        let prev = uuid::Uuid::parse_str(&ids[i-1]).unwrap();
        let curr = uuid::Uuid::parse_str(&ids[i]).unwrap();
        let prev_ts = prev.get_timestamp().unwrap().to_datetime().unix_timestamp();
        let curr_ts = curr.get_timestamp().unwrap().to_datetime().unix_timestamp();
        assert!(prev_ts <= curr_ts, "UUIDv7 should be monotonically sortable");
    }
}
```

- [ ] **Step 5: Commit**

```bash
git add crates/argus-protocol/src/ids.rs
git commit -m "refactor(protocol): ThreadId uses UUIDv7 for time-sortable IDs"
```

### Chunk 1.3: SessionId → Uuid Wrapper

**Files:**
- Modify: `crates/argus-protocol/src/ids.rs:4-16`

- [ ] **Step 1: 将 SessionId 从 i64 改为 Uuid wrapper**

```rust
// crates/argus-protocol/src/ids.rs

use uuid::Uuid;

/// Session ID - UUIDv7 (time-sortable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    /// Create a new SessionId using UUIDv7.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// Parse a SessionId from a string representation.
    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(s)?))
    }

    /// Get the inner UUID value.
    pub fn inner(&self) -> &Uuid {
        &self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
```

- [ ] **Step 2: 验证编译（预期大量类型错误，这是正常的）**

Run: `cargo check 2>&1 | grep "SessionId\|session_id" | grep "expected\|mismatch\|cannot" | head -30`
Expected: 有类型错误，需要后续 chunks 修复

- [ ] **Step 3: Commit**

```bash
git add crates/argus-protocol/src/ids.rs
git commit -m "refactor(protocol): SessionId upgraded from i64 to Uuid wrapper"
```

---

## Chunk 2: Trace — Event Types & Error Types

### Chunk 2.1: TurnLogEvent Enum

**Files:**
- Modify: `crates/argus-turn/src/lib.rs` (export)
- Create: `crates/argus-turn/src/events.rs` (new file)
- Test: `crates/argus-turn/tests/trace_events_test.rs` (new)

- [ ] **Step 1: 创建 events.rs 定义 TurnLogEvent**

```rust
//! Turn log events - incremental JSONL event types.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use argus_protocol::llm::ChatMessage;
use argus_protocol::token_usage::TokenUsage;

/// Single event in a turn's JSONL log.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "snake_case")]
pub enum TurnLogEvent {
    TurnStart { system_prompt: String, model: String },
    UserInput { content: String, role: String },
    LlmRequest { messages: Vec<ChatMessage>, tools: Vec<Value> },
    LlmDelta { delta: String, is_complete: bool },
    ToolCallStart { id: String, name: String, arguments: Value },
    ToolCallDelta { id: String, delta: Value },
    ToolResult {
        id: String,
        name: String,
        result: String,
        duration_ms: u64,
        error: Option<String>,
    },
    LlmResponse {
        content: String,
        reasoning_content: Option<String>,
        tool_calls: Vec<Value>,
        finish_reason: String,
    },
    TurnEnd { token_usage: TokenUsage, finish_reason: String },
    TurnError { error: String, at_iteration: Option<u32> },
}

impl TurnLogEvent {
    /// Common header fields for JSONL serialization.
    pub fn to_jsonl(
        &self,
        thread_id: &str,
        turn: u32,
        ts: &str,
    ) -> String {
        let wrapper = serde_json::json!({
            "v": "1",
            "thread_id": thread_id,
            "turn": turn,
            "ts": ts,
            "type": self.type_name(),
            "data": self,
        });
        serde_json::to_string(&wrapper).unwrap()
    }

    fn type_name(&self) -> &'static str {
        match self {
            TurnLogEvent::TurnStart { .. } => "turn_start",
            TurnLogEvent::UserInput { .. } => "user_input",
            TurnLogEvent::LlmRequest { .. } => "llm_req",
            TurnLogEvent::LlmDelta { .. } => "llm_delta",
            TurnLogEvent::ToolCallStart { .. } => "tool_call_start",
            TurnLogEvent::ToolCallDelta { .. } => "tool_call_delta",
            TurnLogEvent::ToolResult { .. } => "tool_result",
            TurnLogEvent::LlmResponse { .. } => "llm_response",
            TurnLogEvent::TurnEnd { .. } => "turn_end",
            TurnLogEvent::TurnError { .. } => "turn_error",
        }
    }
}
```

- [ ] **Step 2: 更新 lib.rs 导出**

```rust
pub mod events;
pub use events::TurnLogEvent;
```

- [ ] **Step 3: 验证编译**

Run: `cargo check -p argus-turn 2>&1 | tail -10`
Expected: 编译成功

- [ ] **Step 4: 写事件序列化测试**

```rust
// crates/argus-turn/tests/trace_events_test.rs

use argus_turn::TurnLogEvent;

#[test]
fn test_turn_start_serialization() {
    let event = TurnLogEvent::TurnStart {
        system_prompt: "You are helpful.".into(),
        model: "gpt-4o".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"turn_start\""));
    assert!(json.contains("You are helpful"));
}

#[test]
fn test_tool_result_with_error() {
    let event = TurnLogEvent::ToolResult {
        id: "call_1".into(),
        name: "bash".into(),
        result: "".into(),
        duration_ms: 100,
        error: Some("timeout".into()),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"error\":\"timeout\""));
}

#[test]
fn test_turn_end_serialization() {
    use argus_protocol::TokenUsage;
    let event = TurnLogEvent::TurnEnd {
        token_usage: TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
        },
        finish_reason: "stop".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"turn_end\""));
    assert!(json.contains("\"input_tokens\":100"));
}
```

- [ ] **Step 5: 运行测试**

Run: `cargo test -p argus-turn trace_events 2>&1 | tail -15`
Expected: 3 tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/argus-turn/src/events.rs crates/argus-turn/src/lib.rs crates/argus-turn/tests/trace_events_test.rs
git commit -m "feat(turn): add TurnLogEvent enum for JSONL incremental logging"
```

### Chunk 2.2: TurnLogError Enum

**Files:**
- Modify: `crates/argus-turn/src/error.rs`

- [ ] **Step 1: 查看现有 error.rs 结构**

Run: `cat crates/argus-turn/src/error.rs`

- [ ] **Step 2: 添加 TurnLogError**

```rust
// 在 error.rs 末尾添加

use std::path::PathBuf;

/// Errors for turn log recovery operations.
#[derive(Debug, thiserror::Error)]
pub enum TurnLogError {
    #[error("turn file not found: {0}")]
    TurnNotFound(PathBuf),

    #[error("malformed JSON event at line {line}: {reason}")]
    MalformedEvent { line: usize, reason: String },

    #[error("unknown event type: {0}")]
    UnknownEventType(String),

    #[error("truncated event at line {line}: {reason}")]
    TruncatedEvent { line: usize, reason: String },
}
```

- [ ] **Step 3: 验证编译**

Run: `cargo check -p argus-turn 2>&1 | tail -5`
Expected: 编译成功

- [ ] **Step 4: 导出 TurnLogError**

```rust
// crates/argus-turn/src/lib.rs
pub use error::TurnLogError;
```

- [ ] **Step 5: Commit**

```bash
git add crates/argus-turn/src/error.rs crates/argus-turn/src/lib.rs
git commit -m "feat(turn): add TurnLogError enum for log recovery"
```

---

## Chunk 3: Trace — TraceWriter 重构 & 恢复

### Chunk 3.1: TraceWriter Append-Only JSONL

**Files:**
- Modify: `crates/argus-turn/src/trace.rs` (完全重写)
- Test: `crates/argus-turn/tests/trace_writer_test.rs` (修改)

- [ ] **Step 1: 完全重写 trace.rs**

```rust
//! Turn execution trace - append-only JSONL incremental logging.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use tokio::fs::{self, File};
use tokio::io::{AsyncWriteExt, BufWriter};

use super::events::TurnLogEvent;
use super::error::TurnLogError;

/// Configuration for trace recording.
#[derive(Debug, Clone)]
pub struct TraceConfig {
    /// Whether tracing is enabled.
    pub enabled: bool,
    /// Directory where trace files are written.
    pub trace_dir: PathBuf,
    /// Whether to record streaming deltas (llm_delta, tool_call_delta).
    pub include_streaming_deltas: bool,
}

impl TraceConfig {
    /// Create a new TraceConfig with defaults.
    pub fn new(enabled: bool, trace_dir: PathBuf) -> Self {
        Self {
            enabled,
            trace_dir,
            include_streaming_deltas: true,
        }
    }

    /// Create a disabled TraceConfig (no tracing).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            trace_dir: PathBuf::new(),
            include_streaming_deltas: true,
        }
    }
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self::disabled()
    }
}

/// Async writer for append-only JSONL trace files.
pub struct TraceWriter {
    file: BufWriter<File>,
    thread_id: String,
    turn_number: u32,
    ts_offset_ms: i64,
}

impl TraceWriter {
    /// Create a new TraceWriter for the given thread and turn.
    /// Opens file in append mode, creates if not exists.
    pub async fn new(
        thread_id: &str,
        turn_number: u32,
        config: &TraceConfig,
    ) -> std::io::Result<Self> {
        let dir = config.trace_dir.join(thread_id).join("turns");
        fs::create_dir_all(&dir).await?;

        let file_path = dir.join(format!("{}.jsonl", turn_number));
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .await?;

        Ok(Self {
            file: BufWriter::new(file),
            thread_id: thread_id.to_string(),
            turn_number,
            ts_offset_ms: Utc::now().timestamp_millis(),
        })
    }

    /// Write a single event to the JSONL file.
    pub async fn write_event(&mut self, event: &TurnLogEvent) -> std::io::Result<()> {
        let ts = Utc::now().to_rfc3339();
        let json = serde_json::json!({
            "v": "1",
            "thread_id": self.thread_id,
            "turn": self.turn_number,
            "ts": ts,
            "type": event.type_name(),
            "data": event,
        });
        let line = serde_json::to_string(&json).unwrap();
        self.file.write_all(line.as_bytes()).await?;
        self.file.write_all(b"\n").await?;
        self.file.flush().await?;
        Ok(())
    }

    /// Write a turn_end event (used by finish_success).
    pub async fn finish_success(
        mut self,
        token_usage: &argus_protocol::TokenUsage,
    ) -> std::io::Result<()> {
        let event = TurnLogEvent::TurnEnd {
            token_usage: token_usage.clone(),
            finish_reason: "stop".into(),
        };
        self.write_event(&event).await?;
        self.file.flush().await?;
        Ok(())
    }

    /// Write a turn_error event (used by finish_failure).
    pub async fn finish_failure(mut self, error: &str) -> std::io::Result<()> {
        let event = TurnLogEvent::TurnError {
            error: error.to_string(),
            at_iteration: None,
        };
        self.write_event(&event).await?;
        self.file.flush().await?;
        Ok(())
    }
}
```

> **注意**: `TurnLogEvent::type_name()` 需要在 events.rs 中实现为 `pub fn type_name(&self) -> &'static str`。如果上面 Chunk 2.1 的 `type_name` 是 private，需要改为 `pub(crate)` 或放在 trace.rs 的 helper 中。

- [ ] **Step 2: 修复 events.rs 中 type_name 的可见性**

如果 `TurnLogEvent::type_name` 是 private，在 events.rs 中改为：

```rust
pub(crate) fn type_name(&self) -> &'static str {
```

或在 trace.rs 中定义本地 helper：

```rust
fn event_type_name(event: &TurnLogEvent) -> &'static str {
    match event {
        TurnLogEvent::TurnStart { .. } => "turn_start",
        TurnLogEvent::UserInput { .. } => "user_input",
        TurnLogEvent::LlmRequest { .. } => "llm_req",
        TurnLogEvent::LlmDelta { .. } => "llm_delta",
        TurnLogEvent::ToolCallStart { .. } => "tool_call_start",
        TurnLogEvent::ToolCallDelta { .. } => "tool_call_delta",
        TurnLogEvent::ToolResult { .. } => "tool_result",
        TurnLogEvent::LlmResponse { .. } => "llm_response",
        TurnLogEvent::TurnEnd { .. } => "turn_end",
        TurnLogEvent::TurnError { .. } => "turn_error",
    }
}
```

- [ ] **Step 3: 验证编译**

Run: `cargo check -p argus-turn 2>&1 | tail -10`
Expected: 编译成功

- [ ] **Step 4: 修改现有 trace_writer_test.rs 适配新 API**

现有测试可能使用旧的 `finish_success` 签名，检查并更新。

- [ ] **Step 5: 运行测试**

Run: `cargo test -p argus-turn trace_writer 2>&1 | tail -15`
Expected: tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/argus-turn/src/trace.rs
git commit -m "refactor(turn): TraceWriter uses append-only async JSONL"
```

### Chunk 3.2: recover_turn_events & TurnLogState

**Files:**
- Modify: `crates/argus-turn/src/trace.rs` (添加恢复函数)
- Create: `crates/argus-turn/tests/trace_recovery_test.rs`
- Modify: `crates/argus-turn/src/config.rs` (include_streaming_deltas)

- [ ] **Step 1: 在 trace.rs 末尾添加恢复函数**

```rust
use tokio::io::AsyncBufReadExt;
use tokio_stream::Stream;

/// Turn log state reconstructed from JSONL for replay/recovery.
#[derive(Debug)]
pub struct TurnLogState {
    pub thread_id: String,
    pub turn_number: u32,
    pub system_prompt: Option<String>,
    pub model: Option<String>,
    pub messages: Vec<argus_protocol::llm::ChatMessage>,
    pub tools: Vec<serde_json::Value>,
    pub token_usage: Option<argus_protocol::TokenUsage>,
    pub finish_reason: Option<String>,
    pub error: Option<String>,
}

/// Read JSONL events from a turn file.
pub async fn read_jsonl_events(
    path: &PathBuf,
) -> Result<Vec<TurnLogEvent>, TurnLogError> {
    let content = fs::read_to_string(path).await.map_err(|e| {
        TurnLogError::TurnNotFound(path.clone())
    })?;

    let mut events = Vec::new();
    for (line_idx, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // 尝试解析为带公共头的 wrapper
        if let Ok(wrapper) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(data) = wrapper.get("data") {
                if let Ok(event) = serde_json::from_value(data.clone()) {
                    events.push(event);
                    continue;
                }
            }
        }
        // 尝试直接解析为 TurnLogEvent（某些测试场景）
        match serde_json::from_str::<TurnLogEvent>(line) {
            Ok(event) => events.push(event),
            Err(_) => {
                // 可能是截断行
                tracing::warn!("Malformed JSONL line {}: {}", line_idx + 1, line);
            }
        }
    }
    Ok(events)
}

/// Recover events from a turn JSONL file.
pub fn recover_turn_events(
    trace_dir: &Path,
    session_id: &argus_protocol::SessionId,
    thread_id: &argus_protocol::ThreadId,
    from_turn: u32,
) -> impl Stream<Item = Result<TurnLogEvent, TurnLogError>> {
    // 实现返回一个 tokio_stream，解析每行
    // 如果行被截断，返回 TurnLogError::TruncatedEvent 作为警告，继续
    // 空行忽略
    todo!("recover_turn_events implementation")
}
```

- [ ] **Step 2: 实现 recover_turn_events（完整版）**

用 `tokio_stream::wrappers::LinesStream` 包装文件读取，然后逐行解析。

- [ ] **Step 3: 在 config.rs 中添加 include_streaming_deltas**

```rust
// crates/argus-turn/src/config.rs
// 找到 TraceConfig 定义，添加 include_streaming_deltas
```

- [ ] **Step 4: 验证编译**

Run: `cargo check -p argus-turn 2>&1 | tail -10`
Expected: 编译成功

- [ ] **Step 5: 写恢复测试**

```rust
// crates/argus-turn/tests/trace_recovery_test.rs

use std::io::Write;
use tempfile::tempdir;

#[tokio::test]
async fn test_recover_turn_events_success() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("1.jsonl");

    // 写一个完整的 turn JSONL
    let content = r#"{"v":"1","thread_id":"t1","turn":1,"ts":"2026-03-25T10:00:00Z","type":"turn_start","data":{"system_prompt":"You are helpful","model":"gpt-4o"}}
{"v":"1","thread_id":"t1","turn":1,"ts":"2026-03-25T10:00:01Z","type":"turn_end","data":{"token_usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15},"finish_reason":"stop"}}
"#;
    std::fs::write(&path, content).unwrap();

    let events = argus_turn::read_jsonl_events(&path).await.unwrap();
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], argus_turn::TurnLogEvent::TurnStart { .. }));
    assert!(matches!(events[1], argus_turn::TurnLogEvent::TurnEnd { .. }));
}

#[tokio::test]
async fn test_recover_skips_truncated_line() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("1.jsonl");

    let content = r#"{"v":"1","thread_id":"t1","turn":1,"ts":"2026-03-25T10:00:00Z","type":"turn_start","data":{"system_prompt":"You are helpful","model":"gpt-4o"}}
{"v":"1","thread_id":"t1","turn":1,"ts":"2026-03-25T10:00:01Z","type":"tool_result","data":{"id":"call_1","name":"bash","result":"done","duration_ms":100,"error":null}}
{"incomplete
"#;
    std::fs::write(&path, content).unwrap();

    let events = argus_turn::read_jsonl_events(&path).await.unwrap();
    // 截断行被跳过，前两行正常解析
    assert_eq!(events.len(), 2);
}
```

- [ ] **Step 6: 运行恢复测试**

Run: `cargo test -p argus-turn trace_recovery 2>&1 | tail -15`
Expected: tests PASS

- [ ] **Step 7: Commit**

```bash
git add crates/argus-turn/src/trace.rs crates/argus-turn/src/config.rs crates/argus-turn/tests/trace_recovery_test.rs
git commit -m "feat(turn): add recover_turn_events and TurnLogState"
```

---

## Chunk 4: Session Layer — meta.json & Directory Structure

**Files:**
- Modify: `crates/argus-session/src/manager.rs`

- [ ] **Step 1: 更新目录结构**

当前：`{trace_dir}/{thread_id}/`
新：`{trace_dir}/{session_uuidv7}/{thread_uuidv7}/turns/{n}.jsonl`

在 `SessionManager::create_thread` 中，需要先创建 `session_uuidv7` 目录。

- [ ] **Step 2: 实现 meta.json 管理**

在 `session/` 目录创建/更新 `meta.json`：

```json
{"current_turn": 0}
```

每次 Turn 结束后，`meta.json` 的 `current_turn` 更新为当前 turn 序号。

- [ ] **Step 3: 适配 SessionId 类型变更**

所有 `SessionId` 从 `i64` 改为 `Uuid` 的地方需要更新（DashMap key、SQL 参数等）。

- [ ] **Step 4: 验证编译**

Run: `cargo check -p argus-session 2>&1 | tail -15`
Expected: 编译成功

- [ ] **Step 5: Commit**

```bash
git add crates/argus-session/src/manager.rs
git commit -m "refactor(session): update directory structure for UUIDv7 + meta.json"
```

---

## Chunk 5: Database — SessionId Migration

**Files:**
- Create: `crates/argus-repository/migrations/NEW_MIGRATION.sql`
- Modify: `crates/argus-repository/src/sqlite/mod.rs` (如有迁移运行逻辑)
- Modify: 所有引用 `SessionId` 的 repository 文件

- [ ] **Step 1: 创建新的 migration**

```sql
-- crates/argus-repository/migrations/NEW_MIGRATION.sql
-- Migration: sessions.id from INTEGER to TEXT (UUIDv7)

-- 1. Create new sessions table with UUID id
CREATE TABLE sessions_new (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at TEXT,
    updated_at TEXT
);

-- 2. Migrate existing data (old i64 id → UUID derived from created_at)
INSERT INTO sessions_new (id, name, created_at, updated_at)
SELECT
    lower(hex(sha256(randomblob(8))) || '-' || lower(hex(randomblob(4))) || '-7' || lower(hex(randomblob(3))) || '-' || lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(12))),
    name,
    created_at,
    updated_at
FROM sessions;

-- 3. Drop old table
DROP TABLE sessions;

-- 4. Rename new table
ALTER TABLE sessions_new RENAME TO sessions;

-- 5. Update threads.session_id foreign key (already TEXT in some schemas)
-- This step depends on existing schema; verify with: PRAGMA table_info(threads);

-- 6. Verify
SELECT id FROM sessions LIMIT 5;
```

- [ ] **Step 2: 修复 repository 中的 SessionId 类型**

所有 `sqlx::query` 中 `bind(session_id)` 的地方，从 `i64` 改为 `String`（UUID to_string）。

Run: `cargo check -p argus-repository 2>&1 | grep "SessionId\|session_id" | grep "expected\|mismatch" | head -20`
Expected: 有类型错误需要修复

- [ ] **Step 3: 逐文件修复 SessionId 类型**

常见位置：
- `crates/argus-repository/src/sqlite/session.rs`
- `crates/argus-repository/src/sqlite/thread.rs`

```rust
// i64 → String 示例
sqlx::query("INSERT INTO sessions (id, name, ...) VALUES (?1, ?2, ...)")
    .bind(session_id.to_string())  // 而不是 .bind(session_id.inner())
```

- [ ] **Step 4: 验证编译**

Run: `cargo check -p argus-repository 2>&1 | tail -5`
Expected: 编译成功

- [ ] **Step 5: Commit**

```bash
git add crates/argus-repository/
git commit -m "refactor(repository): migrate SessionId from i64 to UUIDv7"
```

---

## Chunk 6: CLI & Desktop 适配

**Files:**
- Modify: `crates/argus-wing/src/lib.rs` (facade)
- Modify: `crates/desktop/src-tauri/src/commands/*.rs` (SessionId 相关 commands)
- Modify: `crates/desktop/lib/*.ts` (TypeScript types)

- [ ] **Step 1: argus-wing facade 适配 SessionId**

SessionId 从 i64 改为 Uuid，所有 facade 方法签名需要更新。

Run: `cargo check -p argus-wing 2>&1 | grep "SessionId\|session_id" | grep "expected\|mismatch" | head -20`

- [ ] **Step 2: 修复 argus-wing 编译错误**

- [ ] **Step 3: Tauri commands 适配**

修复所有 `list_sessions`, `create_session`, `delete_session`, `rename_session` 等命令的 SessionId 类型。

- [ ] **Step 4: TypeScript 类型映射**

在 desktop lib 中，`SessionId` 从 `number` 改为 `string`（UUID）。

- [ ] **Step 5: 验证整体编译**

Run: `cargo check 2>&1 | tail -10`
Expected: 编译成功

- [ ] **Step 6: Commit**

```bash
git add crates/argus-wing crates/desktop/
git commit -m "refactor(desktop): adapt SessionId UUIDv7 changes across Tauri commands"
```

---

## Chunk 7: Integration — End-to-End

**Files:**
- Modify: `crates/argus-turn/src/turn.rs` (调用 TraceWriter 的地方)

- [ ] **Step 1: 更新 turn.rs 中对 TraceWriter 的调用**

当前 `TraceWriter` 创建和 `write_iteration` 调用需要适配新 API：
- `TraceWriter::new` 现在是 async
- `write_iteration` 改为 `write_event(&TurnLogEvent)`
- `finish_success`/`finish_failure` 现在是 async

- [ ] **Step 2: 注入 TurnLogEvent 到 execute_loop**

在 `execute_loop` 中，每个 LLM 调用、工具调用等位置，调用 `trace_writer.write_event(...)`。

- [ ] **Step 3: 验证整体编译**

Run: `cargo check 2>&1 | tail -10`
Expected: 编译成功

- [ ] **Step 4: 运行完整测试套件**

Run: `cargo test 2>&1 | tail -20`
Expected: 所有测试通过

- [ ] **Step 5: prek 检查**

Run: `cargo fmt --check && cargo clippy --all 2>&1 | tail -20`
Expected: 无 warnings/errors

- [ ] **Step 6: Commit**

```bash
git add crates/argus-turn/src/turn.rs
git commit -m "feat(turn): integrate JSONL incremental logging into execute_loop"
```
