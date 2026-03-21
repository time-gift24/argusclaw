# Turn Trace Design - 持久化与审计日志

## Context

Argusclaw 当前缺少 turn 级别的持久化和审计能力。当前的 `argus-thread` 将消息存储在内存中，进程重启后会话历史丢失。

目标是：
1. **重启恢复**：Thread 重启后能从 DB 加载之前的消息历史
2. **审计回放**：完整记录每个 turn 的执行过程，支持精确回放和调试

参考 ironclaw 的实现方式，trace 应该记录在 turn 内部，记录完整的迭代过程。

## Architecture

```
argus-turn (写入审计日志)
    │
    └── Turn.execute_loop()
            ├── Iteration 0: LLM request/response + tool executions
            ├── Iteration 1: LLM request/response + tool executions
            └── ...
            └── TraceWriter → JSON 文件

argus-thread (消息持久化)
    │
    └── 通过 argus-repository 写入 SQLite (用于重启恢复)
```

## Data Structures

### TraceFile (JSON 文件)

存储路径: `traces/{thread_id}/{turn_number}.json`

```json
{
  "version": "1.0",
  "thread_id": "xxx",
  "session_id": "yyy",
  "turn_number": 1,
  "start_time": "2026-03-21T10:00:00Z",
  "end_time": "2026-03-21T10:00:05Z",
  "iterations": [
    {
      "iteration": 0,
      "llm_request": {
        "model": "gpt-4",
        "messages": [...],
        "tools": [...]
      },
      "llm_response": {
        "content": "...",
        "tool_calls": [...],
        "finish_reason": "tool_calls",
        "token_usage": { "input": 100, "output": 50 }
      },
      "tools": [
        {
          "id": "call_1",
          "name": "echo",
          "arguments": {...},
          "result": "hello",
          "duration_ms": 50,
          "error": null
        }
      ]
    }
  ],
  "final_output": {
    "token_usage": { "input": 500, "output": 200, "total": 700 }
  }
}
```

### Iteration Record

每个 LLM 调用（无论是否产生 tool_calls）产生一条 iteration 记录：

- `llm_request`: 本次迭代的 LLM 请求（messages + tools）
- `llm_response`: LLM 响应（content + tool_calls + finish_reason + token_usage）
- `tools`: 工具执行结果数组（无工具调用时为空数组）

## Data Flow

```
Turn.execute()
    ├── 创建 TraceWriter (文件句柄)
    │
    ├── execute_loop() 开始
    │   └── 每次 LLM 响应后:
    │       └── trace_writer.write_iteration(iteration_record)
    │
    └── Turn 完成后:
        └── trace_writer.finish(final_output)
```

### 持久化时机

| 操作 | 时机 | 存储位置 |
|------|------|----------|
| 创建 trace 文件 | Turn 开始时 | JSON 文件 |
| 写入 iteration | 每次 LLM 响应后 | JSON 文件 |
| 完成 trace | Turn 完成后 | JSON 文件 |
| 写入消息 | Turn 完成后 | SQLite (via repository) |

## File Structure

```
crates/argus-turn/src/
├── lib.rs
├── turn.rs
├── config.rs
├── trace.rs          # 新增: TraceWriter, TraceConfig
└── ...
```

## Key Components

### 1. TraceWriter

```rust
pub struct TraceWriter {
    file: File,
    thread_id: String,
    turn_number: u32,
    writer: BufWriter<File>,
}

impl TraceWriter {
    pub fn new(thread_id: &str, turn_number: u32) -> Result<Self>;
    pub fn write_iteration(&mut self, iteration: &IterationRecord) -> Result<()>;
    pub fn finish(&mut self, output: &TurnOutput) -> Result<()>;
}
```

### 2. TraceConfig

```rust
#[derive(Debug, Clone)]
pub struct TraceConfig {
    pub enabled: bool,
    pub trace_dir: PathBuf,
}
```

### 3. IterationRecord

```rust
#[derive(Debug, Serialize)]
pub struct IterationRecord {
    pub iteration: u32,
    pub llm_request: LlmRequest,
    pub llm_response: LlmResponse,
    pub tools: Vec<ToolExecution>,
}

#[derive(Debug, Serialize)]
pub struct ToolExecution {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub result: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}
```

## Design Decisions

| 决策 | 选择 | 理由 |
|------|------|------|
| 审计日志写入层 | argus-turn | 与 ironclaw 一致，Turn 是完整的执行单元 |
| JSON 管理方式 | 按 thread + turn 编号 | 便于定位和回放特定 turn |
| 日志粒度 | 迭代级别 | 每个 LLM 调用都记录，支持完整回放 |
| 重启恢复方式 | SQLite messages 表 | 现有 repository 基础设施 |
| Undo 处理 | 不清理日志 | 保留审计轨迹，undo 只恢复内存状态 |

## Avoiding Ironclaw's Issues

Ironclaw 的 Undo 导致 DB 记录孤儿：
- **我们的方案**：DB 消息和 JSON 日志都不清理，保留完整历史
- 重启恢复从 DB 加载消息
- JSON 用于精确回放和审计
- Undo 只操作内存，不涉及存储

## Implementation Steps

1. 在 `argus-turn/src/` 新增 `trace.rs`
2. 实现 `TraceWriter` 结构体
3. 实现 `TraceConfig` 配置
4. 在 `TurnBuilder` 中添加 `trace_config` 字段
5. 在 `execute_loop()` 中集成 trace 写入
6. 添加 `TurnOutput::trace_iteration()` 方法

## Files to Modify

| File | Change |
|------|--------|
| `crates/argus-turn/src/trace.rs` | 新增 |
| `crates/argus-turn/src/config.rs` | 新增 TraceConfig |
| `crates/argus-turn/src/turn.rs` | 集成 TraceWriter |
| `crates/argus-turn/src/lib.rs` | 导出新模块 |
| `crates/argus-turn/Cargo.toml` | 添加 serde 依赖 |

## Verification

1. 单元测试: 创建 TraceWriter 并写入/读取 JSON
2. 集成测试: 执行一个 turn，验证 trace 文件生成
3. 工具调用测试: 执行带工具的 turn，验证 iteration 记录完整
4. 重启恢复测试: 写入消息后重启，验证能正确恢复
