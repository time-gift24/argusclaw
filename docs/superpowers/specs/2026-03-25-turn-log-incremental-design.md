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
        ├── plan.json              # 现有 plan store（保持不变）
        └── turns/
            ├── 1.jsonl            # Turn 1 的增量日志
            ├── 2.jsonl            # Turn 2 的增量日志
            └── ...
```

目录在 turn 执行**开始时**即创建，文件以 append 模式打开。

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
| `turn_end` | Turn 结束时 | `{token_usage: {input, output, total}, finish_reason}` |

> `llm_delta` 和 `tool_call_delta` 受 `include_streaming_deltas` 配置控制。

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
1. `sessions.id` 从 `INTEGER PRIMARY KEY` 改为 `TEXT PRIMARY KEY`
2. `threads.session_id` 外键类型相应变更
3. 现有数据迁移：旧 i64 SessionId 映射为 UUID 字符串

---

## TraceWriter 重构

### 写入模式

```rust
let file = OpenOptions::new()
    .create(true)
    .append(true)
    .open(path)?;

// 每次事件直接落盘，不在内存累积
writeln!(self.file, "{}", serde_json::to_string(&event)?)?;
self.file.flush()?;
```

### 保留字段兼容

现有的 `TraceFile` JSON 格式保留作为**恢复时聚合视图**（可选），不影响 JSONL 增量写入。

---

## 恢复 API

### 函数签名

```rust
/// 从指定 turn 开始读取 JSONL 事件流，用于恢复执行状态。
pub fn recover_turn_events(
    trace_dir: &Path,
    session_id: SessionId,
    thread_id: ThreadId,
    from_turn: u32,
) -> impl Stream<Item = Result<TurnLogEvent, TurnLogError>>;
```

### 恢复粒度

按 `from_turn` 显式定位，读取对应 JSONL 文件所有行。

### 错误处理

- 文件不存在 → `TurnLogError::TurnNotFound`
- JSON 解析失败 → `TurnLogError::MalformedEvent`
- 非法的 event type → `TurnLogError::UnknownEventType`

---

## 配置

```rust
pub struct TraceConfig {
    /// 是否启用追踪
    pub enabled: bool,
    /// 追踪文件根目录
    pub trace_dir: PathBuf,
    /// 是否记录 streaming deltas（llm_delta, tool_call_delta）
    pub include_streaming_deltas: bool,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
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
| `crates/argus-turn/src/trace.rs` | `TraceWriter` 改为 append-only JSONL |
| `crates/argus-turn/src/config.rs` | `TraceConfig` 新增 `include_streaming_deltas` |
| `crates/argus-session/src/manager.rs` | SessionId 类型变更影响 |
| `crates/argus-repository/` | DB schema 迁移 |
| `Cargo.toml` | uuid 添加 v7 feature |
| Tauri commands | SessionId 类型变更影响 |

---

## 测试计划

1. JSONL 写入完整性：执行一个 turn 后逐行解析，验证所有预期事件类型都存在
2. 崩溃恢复：模拟写入中途进程终止，验证 JSONL 文件不损坏
3. UUIDv7 可排序性：创建多个 Session/Thread，验证 UUID 时间戳递增
4. 恢复 API：读取指定 turn 的 JSONL，重建事件流并验证顺序
5. streaming deltas 开关：配置为 false 时验证不产生 delta 事件

---

## 工作量估算

| 模块 | 任务 | 优先级 |
|------|------|--------|
| 依赖 | uuid 添加 v7 feature，编译验证 | P1 |
| 协议层 | `ThreadId` 改 UUIDv7 | P1 |
| 协议层 | `SessionId` 从 i64 改为 Uuid | P1 |
| 追踪 | `TraceWriter` 改为 append JSONL | P1 |
| 追踪 | 新增 `TurnLogEvent` 枚举和 JSONL 序列化 | P1 |
| 追踪 | 新增 `recover_turn_events` 恢复函数 | P1 |
| 配置 | `TraceConfig` 新增 `include_streaming_deltas` | P1 |
| 会话层 | `SessionManager` 适配 SessionId 类型变更 | P1 |
| 持久化 | 数据库 migration：SessionId i64→TEXT | P1 |
| CLI | 适配 SessionId 类型变更 | P1 |
| 测试 | JSONL 写入/解析测试 | P1 |
| 测试 | 崩溃恢复测试 | P2 |
