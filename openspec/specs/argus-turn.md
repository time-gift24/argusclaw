# argus-turn — 单轮对话执行引擎

## 职责

执行单个对话轮次（Turn）。从 Thread 接收消息历史和配置，执行 LLM → Tool → LLM 循环，广播流式事件，返回更新后的消息历史。

**不同于 Thread**：Thread 管理多轮会话的长期状态；Turn 管理单次执行的瞬时状态。Turn 执行后销毁，不维护任何会话状态。

## 执行流程

```
Turn::execute()
    │
    ├── spawn_event_forwarder()    ← 后台任务，转发 stream_tx → thread_event_tx
    │
    ├── (可选) TraceWriter 创建    ← 迭代级审计日志
    │
    └── execute_loop()
            │
            ▼
for iteration in 0..max_iterations {
    │
    ├── 1. fire BeforeCallLLM hooks
    │      ├─ HookAction::Continue     → 继续
    │      ├─ HookAction::ModifyMessages → 修改消息后继续
    │      └─ HookAction::Block       → 返回 TurnError::LlmCallBlocked
    │
    ├── 2. 构建 ToolCompletionRequest
    │      ├─ messages（可能已被 hooks 修改）
    │      ├─ tools（从 self.tools 转换）
    │      └─ agent_record 配置（max_tokens, temperature, thinking）
    │
    ├── 3. call_llm_streaming()    ← 始终使用流式 API
    │      ├─ provider.stream_complete_with_tools() 优先
    │      └─ 不支持流式时降级到 complete_with_tools
    │
    ├── 4. 累积流事件 → ToolCompletionResponse
    │
    ├── 5. process_finish_reason()
    │      ├─ Stop      → NextAction::Return → fire TurnEnd hooks → return
    │      ├─ ToolUse   → NextAction::ContinueWithTools → 执行工具
    │      └─ Length    → NextAction::LengthExceeded → return error
    │
    ├── 6. (如需工具) execute_tools_parallel()
    │      ├─ fire BeforeToolCall hooks
    │      ├─ broadcast ToolStarted 事件
    │      ├─ tokio::time::timeout(单个工具, 120s)
    │      ├─ sanitize_tool_output() ← 安全过滤
    │      ├─ broadcast ToolCompleted 事件
    │      ├─ fire AfterToolCall hooks
    │      └─ 添加 ToolResult 消息到 history
    │
    └── 7. 回到循环开始，用更新后的 history 再次调用 LLM
}
```

**达到 max_iterations（默认 50）**：返回 `TurnError::MaxIterationsReached`。

## 核心概念

### Turn（直接拥有资源）

```
Turn
  tools: Vec<Arc<dyn NamedTool>>    ← 直接拥有，不依赖 ToolManager
  hooks: Vec<Arc<dyn HookHandler>> ← 直接拥有，不依赖 HookRegistry
```

设计决策：Turn 不持有 ToolManager/HookRegistry 的引用，而是每次执行时从外部传入具体的 tools/hooks。这避免了生命周期复杂性——Turn 是一次性的（execute 后就 drop），不需要共享注册表。

### TurnConfig

```rust
max_tool_calls: Option<u32>      // 单次 LLM 响应最多工具数（默认 10）
tool_timeout_secs: Option<u64>    // 单工具超时（默认 120s）
max_iterations: Option<u32>       // 最大迭代数（默认 50）
safety_config: SafetyConfig       // 工具输出安全过滤配置
trace_config: Option<TraceConfig> // 追踪配置
```

**max_tool_calls 的作用**：当 LLM 返回超过 N 个工具调用时，只执行前 N 个。这样即使 LLM"冲动"想一次调用很多工具，系统也能强制它分步执行。Turn 还会自动注入一条 system message 告诉 LLM 这个限制。

### TraceWriter（迭代级审计）

每个迭代记录：
- LLM 请求（messages + tools 的 JSON 快照）
- LLM 响应（content, reasoning_content, tool_calls, finish_reason, token 统计）
- 工具执行结果（id, name, arguments, result, duration_ms, error）

文件结构：
```
{trace_dir}/{thread_id}/{turn_number}.json
```

```json
{
  "version": "1.0",
  "thread_id": "...",
  "turn_number": 1,
  "start_time": "...",
  "end_time": "...",
  "iterations": [
    {
      "iteration": 0,
      "llm_request": { "messages": [...], "tools": [...] },
      "llm_response": { "content": "...", "tool_calls": [...] },
      "tools": [{ "id": "...", "name": "...", "result": "..." }]
    }
  ],
  "final_output": { "token_usage": { "input_tokens": ..., "output_tokens": ... } }
}
```

### SafetyConfig（工具输出安全）

`sanitize_tool_output` 会根据 SafetyConfig 过滤工具输出中的敏感内容。触发时：
- 输出被截断
- 日志中记录 `pattern`、`original_len`、`truncated_len`
- 工具结果消息仍保留（截断后版本）

## 约束

- **始终使用流式 API**：内部不使用 `complete_with_tools`，只用 `stream_complete_with_tools`。不支持流式的 provider 才降级。
- **工具并行执行**：多个工具调用在同一 iteration 内并行执行（`futures_util::future::join_all`）。但一个 iteration 内的所有 LLM 调用是串行的（必须等结果再决定下一步）。
- **超时是 per-tool**：每个工具调用有独立的 timeout，不是整体超时。
- **Hooks 直接迭代**：`fire_hooks()` 直接遍历 `self.hooks`，不用 HookRegistry。
- **Hook 的 ModifyTools 不生效**：代码中对此有 `tracing::warn`，但未实际支持。
- **Turn 是 owned**：`.execute()` 消耗 self，这是刻意的——Turn 是一次性的，不应复用。

## 错误处理策略

| 错误类型 | 来源 | 处理 |
|----------|------|------|
| `LlmFailed` | provider 返回错误 | 向上传播 |
| `LlmCallBlocked` | BeforeCallLLM hook 返回 Block | 向上传播 |
| `ToolExecutionFailed` | 工具执行 panic/返回错误 | 记录到消息历史，继续循环 |
| `ToolCallBlocked` | BeforeToolCall hook 返回 Block | 向上传播 |
| `MaxIterationsReached` | 超过 50 次迭代 | 向上传播 |
| `ContextLengthExceeded` | LLM 返回 length | 向上传播 |
| `TimeoutExceeded` | 工具执行超时 | 视为 ToolExecutionFailed，记录并继续 |

**工具执行失败不中断 Turn**：工具执行失败（panic 或返回错误）时，结果记录为错误字符串，加入消息历史，Turn 继续执行。这让 LLM 可以处理或报告工具错误，而不是整个 Turn 失败。

## 事件转发机制

```
stream_tx (TurnStreamEvent) ──forwarder task──▶ thread_event_tx (ThreadEvent)
```

forwarder 是一个 `tokio::spawn` 的后台任务，在 Turn 构造时创建，在 Turn drop 时（通过 channel 关闭）自然终止。

## 向后兼容

`execution.rs` 提供了 `execute_turn()` 和 `execute_turn_streaming()` 函数，接收旧的 `TurnInput`（含 ToolManager/HookRegistry），内部转换为 Turn 执行。这是过渡期用的，最终应全部迁移到 TurnBuilder。

## 下游依赖

```
argus-thread  — 创建 Turn 实例并调用 execute()
argus-session  — 通过 Thread 间接使用
```

## 扩展点

**添加新 Hook 类型**：在 `argus-protocol` 的 `HookEvent` 枚举中添加变体，在 `Turn::fire_hooks()` 中处理。

**添加新 TurnStreamEvent 变体**：在 `config.rs` 的 `TurnStreamEvent` 枚举中添加，在 `spawn_event_forwarder()` 中添加对应的映射。
