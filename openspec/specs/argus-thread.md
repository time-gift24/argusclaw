# argus-thread — 多轮对话会话管理

## 职责

管理多轮对话会话（Thread）。每个 Thread 维护消息历史，在用户发消息时创建 Turn 执行，执行完成后更新消息历史。

```
用户发消息
    │
    ▼
Thread::send_message(user_input)
    │
    ├── Compactor.compact()     ← 检查是否需要压缩上下文
    │
    ├── ChatMessage::user()     ← 把用户输入加入历史
    │
    ├── Thread 收集 tools/hooks  ← 从 ToolManager/HookRegistry 取当前快照
    │
    └── Thread::execute_turn_streaming()
            │
            ▼
         Turn::execute()
            │
            ├── LLM → Tool → LLM 循环
            ├── 流式事件 → TurnStreamEvent → ThreadEvent
            │
            ▼
         TurnOutput { messages, token_usage }
            │
            ▼
Thread::apply_turn_output()    ← 更新 messages，重新计算 token_count
```

## 核心概念

### Thread

- **生命周期**：Thread 是长期存活的对象，跨多次 `send_message` 调用
- **状态**：`Idle`（默认）/ `Processing`（执行 Turn 时）
- **共享资源**：ToolManager（全局工具注册表）、HookRegistry（全局钩子注册表）、消息历史
- **非共享**：Turn 每次从 Manager/Registry 取当前快照，独立执行后销毁

**重要**：Thread 不直接执行 LLM 调用。Turn 执行 LLM 调用，Thread 只负责协调。

### Compactor（上下文压缩）

两种内置策略，都通过 `Compactor` trait 实现：

**KeepRecentCompactor**（默认）：
- 当 token_count 超过 context_window × 80% 时触发
- 保留 system message + 最近 N 条非 system 消息
- 简单但可能丢失早期对话重要信息

**KeepTokensCompactor**：
- 当 token_count 超过 context_window × 80% 时触发
- 保留 system message + 从后往前累加，直到 token 达到 context_window × 50%
- 更精确但计算更复杂

两种策略的共同约束：
- system message 永远不删除（`Role::System`）
- token 估算使用简单启发式：`content.len() / 4`（不够精确但够用）
- 压缩是单向的（删了就回不来），所以阈值设为 80% 留有余量

**CompactorManager**：管理多个 Compactor 实例，支持按名字注册和查找。用于 agent 配置中指定用哪种策略。

### ThreadConfig

```rust
compact_threshold_ratio: f32   // 触发压缩的阈值（默认 0.8）
turn_config: TurnConfig         // 下传给 Turn 的执行配置
```

### Token 估算

使用简单启发式：`estimate_tokens(content) = max(1, content.len() / 4)`

这个估算不精确（中文/token 比远高于英文），但对于判断"上下文是否太长"足够用。如需精确 token 计数，应使用 tiktoken 或类似库。

## 约束

- **Thread 是 `&mut self`**：`send_message` 需要 `&mut self`，因为要修改 messages、token_count、turn_count
- **一次只处理一条消息**：没有并发执行多个 Turn 的机制（`&mut self` 隐式保证）
- **消息历史存在 Thread 内**：内存中维护，未持久化（argus-repository 负责持久化）
- **Thread 不负责工具执行**：只传给 Turn
- **Thread 不直接 fire hooks**：Turn 执行时直接调用 hooks

## 事件流

```
Turn
  │ stream_tx (TurnStreamEvent)
  │ thread_event_tx (ThreadEvent)
  │
  ├─ TurnStreamEvent::LlmEvent → ThreadEvent::Processing
  ├─ TurnStreamEvent::ToolStarted → ThreadEvent::ToolStarted
  ├─ TurnStreamEvent::ToolCompleted → ThreadEvent::ToolCompleted
  ├─ TurnCompleted → ThreadEvent::TurnCompleted
  ├─ TurnFailed → ThreadEvent::TurnFailed
  └─ Idle → ThreadEvent::Idle
```

外部订阅者（CLI、Tauri frontend）通过 `Thread::subscribe()` 获取 `broadcast::Receiver<ThreadEvent>`。

## 下游依赖

```
argus-session  — 创建和管理 Thread
argus-repository  — 持久化 Thread 状态
```

## 扩展点

**添加新 Compactor**：实现 `Compactor` trait（`compact()` + `name()`），注册到 `CompactorManager`。

**不支持的功能**（设计边界）：
- 并发多个 Turn：Thread 用 `&mut self` 防止
- 持久化：Thread 只管内存，持久化由 argus-repository 负责
- 工具注册：Thread 不管理工具，工具由 ToolManager 管理
