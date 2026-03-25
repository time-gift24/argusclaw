# Turn Log 增量记录与恢复设计

**日期**: 2026-03-25
**状态**: 设计中
**负责人**: @claude

## 概述

将 turn-log 从"内存累积 + 结束时一次性写入"改为 **append-only JSONL 增量写入**，并支持从指定 turn 恢复完整执行状态。同时将 Session 和 Thread ID 升级为 UUIDv7，引入时间可排序性以支持定期清理。

## 背景

当前 `TraceWriter` 在 Turn 执行过程中将所有 `IterationRecord` 累积在内存中，只在 `finish_success()` 或 `finish_failure()` 时一次性将完整的 `TraceFile` JSON 写入文件。这种设计存在以下问题：

1. **断电/崩溃丢失**：进程异常终止时，整个 turn 的日志全部丢失
2. **无法增量恢复**：只能从完整的 turn 文件恢复，无法从中间状态重建
3. **Session/Thread ID 无时间信息**：使用随机 UUID，无法按时间批量清理旧数据

## 目标

1. append-only 写入，每次事件直接落盘，崩溃可恢复
2. 支持从任意 turn 恢复完整执行状态
3. Session/Thread ID 使用 UUIDv7，支持按时间排序和清理
4. streaming delta 可配置开关（默认开启）

---

## 文件结构

```
{trace_dir}/
└── {session_uuidv7}/
    └── {thread_uuidv7}/
        ├── meta.json              # 元数据：当前最大 turn 序号
        ├── plan.json             # 任务计划（现有 plan store，与本 spec 无关，保持不变）
        └── turns/
            ├── 1.jsonl
            ├── 2.jsonl
            └── ...
```

- 目录在 Session 创建时即创建（SessionManager 负责）
- `meta.json` 在每次 Turn 结束后更新 `current_turn` 字段
- 文件以 append 模式打开，追加写入

---

## JSONL 事件格式

共用结构：

```json
{"v":"1","thread_id":"...","turn":1,"type":"...","ts":"...","data":{...}}
```

字段说明：
- `v`: 版本，固定 `"1"`
- `thread_id`: 线程 UUID（字符串）
- `turn`: turn 序号
- `ts`: ISO 8601 时间戳（UTC）
- `type`: 事件类型
- `data`: 事件数据（类型相关）

### 事件类型表

| type | 触发时机 | data 内容 |
|------|---------|----------|
| `turn_start` | Turn 开始时 | `{system_prompt, model}` |
| `user_input` | 用户消息入队时 | `{content, role}` |
| `llm_req` | LLM 请求发送前 | `{messages, tools}` |
| `llm_delta` | LLM content delta | `{delta, is_complete}` |
| `tool_call_start` | 工具调用开始 | `{id, name, arguments}` |
| `tool_call_delta` | 工具参数增量 | `{id, delta}` |
| `tool_result` | 工具执行完成 | `{id, name, result, duration_ms, error?}` |
| `llm_response` | LLM 完整响应 | `{content, reasoning_content, tool_calls, finish_reason}` |
| `turn_end` | Turn 正常结束时 | `{token_usage: {input, output, total}, finish_reason}` |
| `turn_error` | Turn 因错误中断时 | `{error: String, at_iteration?}` |

> `llm_delta` 和 `tool_call_delta` 受 `include_streaming_deltas` 配置控制。
> `turn_end` 与 `turn_error` 二者互斥，标志 turn 的终态。

### 完整示例

```jsonl
{"v":"1","thread_id":"0192c4e0-1234-5678-9abc-def012345678","turn":1,"type":"turn_start","ts":"2026-03-25T10:00:00Z","data":{"system_prompt":"You are helpful.","model":"gpt-4o"}}
{"v":"1","thread_id":"0192c4e0-1234-5678-9abc-def012345678","turn":1,"type":"user_input","ts":"2026-03-25T10:00:00Z","data":{"content":"Hello!","role":"user"}}
{"v":"1","thread_id":"0192c4e0-1234-5678-9abc-def012345678","turn":1,"type":"llm_req","ts":"2026-03-25T10:00:01Z","data":{"messages":[...],"tools":[...]}}
{"v":"1","thread_id":"0192c4e0-1234-5678-9abc-def012345678","turn":1,"type":"llm_delta","ts":"2026-03-25T10:00:02Z","data":{"delta":"Hello","is_complete":false}}
{"v":"1","thread_id":"0192c4e0-1234-5678-9abc-def012345678","turn":1,"type":"tool_call_start","ts":"2026-03-25T10:00:03Z","data":{"id":"call_abc","name":"bash","arguments":{}}}
{"v":"1","thread_id":"0192c4e0-1234-5678-9abc-def012345678","turn":1,"type":"tool_result","ts":"2026-03-25T10:00:05Z","data":{"id":"call_abc","name":"bash","result":"done","duration_ms":2000}}
{"v":"1","thread_id":"0192c4e0-1234-5678-9abc-def012345678","turn":1,"type":"turn_end","ts":"2026-03-25T10:00:06Z","data":{"token_usage":{"input":100,"output":50,"total":150},"finish_reason":"stop"}}
```

---

## UUIDv7 升级

### 依赖变更

`Cargo.toml` workspace 中 `uuid` 添加 `v7` feature：

```toml
uuid = { version = "1", features = ["serde", "v4", "v7"] }
```

### ThreadId

```rust
// 之前
pub fn new() -> Self { Self(Uuid::new_v4()) }

// 之后
pub fn new() -> Self { Self(Uuid::now_v7()) }
```

### SessionId

从 `i64` 升级为 `Uuid` wrapper：

```rust
// 之前
pub struct SessionId(pub i64);

// 之后
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self { Self(Uuid::now_v7()) }
    pub fn parse(s: &str) -> Result<Self, uuid::Error> { ... }
}
```

### 数据库 Schema 迁移

需要新增 migration：

1. 新增 `sessions_uuid` 表（TEXT PRIMARY KEY），结构同 sessions 但 id 为 UUID
2. 将现有 sessions 数据迁移到新表，旧 i64 id 映射为固定前缀 + 序号的 UUIDv7（如从 `created_at` 时间戳派生）
3. `threads.session_id` 外键类型相应从 INTEGER 改为 TEXT
4. 新建 `sessions` 表替换旧表（保留 uuid 版本）

> 旧 session 恢复时用派生的 UUIDv7 可排序，但不保证严格单调递增（取决于 created_at 精度）。新 session 直接 `Uuid::now_v7()` 生成。

---

## TraceWriter 重构

### 写入模式

```rust
// 使用 tokio 异步 I/O（符合架构规范）
let file = tokio::fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open(&path)
    .await?;

// 每次事件直接落盘，不在内存累积
let line = serde_json::to_string(&event)?;
tokio::io::AsyncWriteExt::write_all(&mut file, (line + "\n").as_bytes()).await?;
tokio::io::AsyncWriteExt::flush(&mut file).await?;
```

> 选择异步 I/O 而非同步：符合架构规范，且 trace 写入发生在 turn 执行循环中，异步不阻塞主流程。

---

## 恢复 API

### 恢复用途

`recover_turn_events` 返回的事件流可用于：

1. **审计/回放**：只读遍历历史事件，查看完整的 LLM 调用、工具执行过程
2. **执行重建**：从事件流重建 `TurnInput`（messages、tools），用于重新执行失败的 turn

即：恢复 API 提供**只读事件流**，调用方负责解释和使用这些事件。

### TurnLogState（重建用中间结构）

```rust
/// 从 JSONL 事件流重建的完整执行状态，供重新执行使用。
pub struct TurnLogState {
    pub thread_id: ThreadId,
    pub turn_number: u32,
    pub system_prompt: String,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDefinition>,
    pub token_usage: Option<TokenUsage>,
    pub finish_reason: Option<String>,
    pub error: Option<String>,
}
```

### TurnLogEvent 枚举

```rust
pub enum TurnLogEvent {
    TurnStart { system_prompt: String, model: String },
    UserInput { content: String, role: String },
    LlmRequest { messages: Vec<ChatMessage>, tools: Vec<ToolDefinition> },
    LlmDelta { delta: String, is_complete: bool },
    ToolCallStart { id: String, name: String, arguments: Value },
    ToolCallDelta { id: String, delta: Value },
    ToolResult { id: String, name: String, result: String, duration_ms: u64, error: Option<String> },
    LlmResponse { content: String, reasoning_content: Option<String>, tool_calls: Vec<ToolCall>, finish_reason: String },
    TurnEnd { token_usage: TokenUsage, finish_reason: String },
    TurnError { error: String, at_iteration: Option<u32> },
}
```

### TurnLogError 枚举

```rust
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

### 截断行恢复策略

JSONL 文件最后一行可能因进程崩溃而被截断（不完整的 JSON）。恢复时：

- **解析成功的行**：正常返回事件
- **截断行（JSON 解析失败且内容非空）**：跳过该行，记录 warning log，返回 `TurnLogError::TruncatedEvent` 的警告事件（不 abort），继续处理后续行
- **空行**：忽略

### 函数签名

```rust
/// 从指定 turn 开始读取 JSONL 事件流。
pub fn recover_turn_events(
    trace_dir: &Path,
    session_id: SessionId,
    thread_id: ThreadId,
    from_turn: u32,
) -> impl Stream<Item = Result<TurnLogEvent, TurnLogError>>;
```

### 恢复粒度

按 `from_turn` 显式定位，先读取 `meta.json` 确认该 turn 是否存在，然后读取对应的 `turns/{from_turn}.jsonl` 所有行。

---

## 配置

```rust
pub struct TraceConfig {
    /// 是否启用追踪（默认 false，即默认不追踪）
    pub enabled: bool,
    /// 追踪文件根目录
    pub trace_dir: PathBuf,
    /// 是否记录 streaming deltas（llm_delta, tool_call_delta），默认 true
    pub include_streaming_deltas: bool,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            enabled: false,  // 默认关闭，与现有的 disabled() 行为一致
            trace_dir: PathBuf::new(),
            include_streaming_deltas: true,
        }
    }
}
```

---

## 影响范围

### 需修改的文件

| 文件 | 改动 |
|------|------|
| `crates/argus-protocol/src/ids.rs` | `ThreadId::new()` 改 UUIDv7；`SessionId` 从 i64 改为 Uuid |
| `crates/argus-turn/src/trace.rs` | `TraceWriter` 改为 append-only JSONL + async I/O |
| `crates/argus-turn/src/config.rs` | `TraceConfig` 新增 `include_streaming_deltas`；默认 `enabled=false` |
| `crates/argus-turn/src/error.rs` | 新增 `TurnLogError` enum |
| `crates/argus-session/src/manager.rs` | SessionId 类型变更；创建 session 目录 |
| `crates/argus-repository/migrations/` | 新增 migration：SessionId i64→TEXT |
| `crates/argus-session/src/session.rs` | SessionId 类型变更影响 |
| `crates/argus-thread/src/thread.rs` | 适配 session 目录结构变更 |
| `Cargo.toml` | uuid workspace 添加 v7 feature |
| `crates/argus-wing/src/` | SessionId 类型变更影响 facade |
| `crates/desktop/src-tauri/src/commands/` | 所有使用 SessionId 的 Tauri commands 适配（list_sessions, create_session, delete_session 等） |
| `crates/desktop/lib/` | TypeScript 侧 SessionId 类型映射更新 |

---

## 测试计划

1. **JSONL 写入完整性**：执行一个 turn 后逐行解析，验证所有预期事件类型（`turn_start`、`llm_req`、`tool_result`、`turn_end`）都存在
2. **崩溃恢复**：
   - 用 subprocess 执行 turn，写入中途 `SIGKILL` 终止进程
   - 用 `recover_turn_events` 读取生成的 JSONL
   - 验证：a) 能跳过截断行返回 `TruncatedEvent` 警告；b) 之前写入的完整行全部正常恢复
3. **UUIDv7 可排序性**：创建多个 Session/Thread，验证 UUID 时间戳字段递增
4. **恢复 API**：
   - 读取指定 turn 的 JSONL，逐事件验证类型和顺序
   - 传入不存在的 turn，验证返回 `TurnLogError::TurnNotFound`
   - 传入包含 malformed 行的 JSONL，验证返回 `TurnLogError::MalformedEvent`
5. **streaming deltas 开关**：配置 `include_streaming_deltas=false` 时验证 JSONL 中无 `llm_delta`/`tool_call_delta` 行
6. **meta.json 一致性**：执行多个 turn 后验证 `meta.json` 的 `current_turn` 与实际最大文件序号一致

---

## 工作量估算

| 模块 | 任务 | 优先级 |
|------|------|--------|
| 依赖 | uuid workspace 添加 v7 feature，编译验证 | P1 |
| 协议层 | `ThreadId::new()` 改 UUIDv7 | P1 |
| 协议层 | `SessionId` 从 i64 改为 Uuid wrapper | P1 |
| 追踪 | 新增 `TurnLogEvent` 枚举 | P1 |
| 追踪 | 新增 `TurnLogError` enum（`error.rs`） | P1 |
| 追踪 | 新增 `TurnLogState` 中间结构 | P1 |
| 追踪 | `TraceWriter` 改为 append-only async JSONL | P1 |
| 追踪 | 新增 `recover_turn_events` 恢复函数 | P1 |
| 追踪 | `meta.json` 管理（创建、更新） | P1 |
| 配置 | `TraceConfig` 新增 `include_streaming_deltas`，`enabled` 默认 false | P1 |
| 会话层 | `SessionManager` 适配 SessionId 类型变更 + 创建 session 目录 | P1 |
| 持久化 | 数据库 migration：SessionId i64→TEXT | P1 |
| CLI | 适配 SessionId 类型变更 | P1 |
| Desktop | Tauri commands SessionId 类型适配 | P1 |
| Desktop | TypeScript SessionId 类型映射更新 | P1 |
| 测试 | JSONL 写入/解析测试 | P1 |
| 测试 | 崩溃恢复测试（SIGKILL） | P2 |
| 测试 | meta.json 一致性测试 | P2 |
