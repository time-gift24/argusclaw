# Argus-Turn Crate 开发指南

> 特性：对话轮次执行引擎，负责 LLM 调用、工具执行、Hook 生命周期和流式事件广播。


- **单个 Turn 的执行逻辑**
- **LLM 调用与响应处理**
- **工具调用执行**（并行）
- **Hook 生命周期管理**
- **流式事件广播**

## 模块结构

```
src/
├── lib.rs              # 公共 API 导出
├── config.rs           # TurnConfig、TurnInput、TurnOutput、TurnStreamEvent
├── error.rs            # TurnError 错误类型
├── execution.rs        # 向后兼容的执行包装器
├── turn.rs             # 核心 Turn 结构和执行逻辑
├── hooks.rs            # Hook 类型重导出
└── bin/
    └── turn.rs         # CLI 测试工具

tests/
└── integration_test.rs # 集成测试
```

## 核心概念

### 1. Turn 结构

**Turn** 是单个对话轮次的完整上下文和执行器：

```rust
pub struct Turn {
    pub id: String,                          // 自动生成的 turn ID
    turn_number: u32,                        // 在 thread 中的序号
    thread_id: String,                       // 所属 thread ID
    messages: Vec<ChatMessage>,              // 消息历史
    provider: Arc<dyn LlmProvider>,          // LLM 提供者
    tools: Vec<Arc<dyn NamedTool>>,          // 可用工具（直接拥有）
    hooks: Vec<Arc<dyn HookHandler>>,        // 生命周期钩子（直接拥有）
    config: TurnConfig,                      // 执行配置
    stream_tx: broadcast::Sender<TurnStreamEvent>,     // 流事件发送器
    thread_event_tx: broadcast::Sender<ThreadEvent>,  // 线程事件发送器
    _forwarder_handle: Option<JoinHandle<()>>,        // 事件转发任务句柄
}
```

**设计模式**：**拥有资源**（vs 之前的借用模式）
- Turn 直接拥有 tools 和 hooks
- 无需 ToolManager 或 HookRegistry
- 简化生命周期管理

### 2. TurnConfig

```rust
pub struct TurnConfig {
    pub max_tool_calls: Option<u32>,        // 单次 LLM 响应最大工具调用数 (默认 10)
    pub tool_timeout_secs: Option<u64>,     // 单个工具执行超时（秒，默认 120）
    pub max_iterations: Option<u32>,        // 最大迭代次数 (默认 50)
}
```

**默认值**：
```rust
impl Default for TurnConfig {
    fn default() -> Self {
        Self {
            max_tool_calls: Some(10),
            tool_timeout_secs: Some(120),
            max_iterations: Some(50),
        }
    }
}
```

### 3. Turn 执行流程

```
Turn::execute()
  ↓
spawn_event_forwarder()  # 启动事件转发任务
  ↓
execute_loop()
  ↓
for iteration in 0..max_iterations {
    ├─ 1. Fire BeforeCallLLM hooks
    ├─ 2. Call LLM (streaming)
    │    ├─ Accumulate LlmStreamEvent
    │    └─ Forward to stream_tx
    ├─ 3. Process finish_reason
    │    ├─ Stop → Add assistant message → Return
    │    ├─ ToolUse → Execute tools → Continue
    │    └─ Length → Return error
    ├─ 4. Execute tools (parallel)
    │    ├─ Fire BeforeToolCall hook
    │    ├─ Send ToolStarted event
    │    ├─ Execute tool with timeout
    │    ├─ Send ToolCompleted event
    │    └─ Fire AfterToolCall hook
    ├─ 5. Add tool result messages to history
    └─ Fire TurnEnd hook
}
```

**关键特性**：
- **流式优先**：始终使用 `stream_complete_with_tools`
- **自动降级**：不支持流式时降级到非流式
- **并行工具**：多个工具调用并行执行
- **超时控制**：每个工具调用有独立超时

### 4. 事件流传播

Turn 通过两个 broadcast channel 传播事件：

```
Turn 内部
  ↓
stream_tx (TurnStreamEvent)
  ↓
Event Forwarder Task
  ↓
thread_event_tx (ThreadEvent)
  ↓
外部订阅者（Thread、Agent、CLI）
```

**事件转换映射**：
```rust
match event {
    TurnStreamEvent::LlmEvent(llm_event) => {
        ThreadEvent::Processing {
            thread_id,
            turn_number,
            event: llm_event,  // RetryAttempt、ContentDelta 等
        }
    }
    TurnStreamEvent::ToolStarted { ... } => {
        ThreadEvent::ToolStarted { ... }
    }
    TurnStreamEvent::ToolCompleted { ... } => {
        ThreadEvent::ToolCompleted { ... }
    }
}
```

### 5. Hook 系统

**Hook 类型**（从 `argus_protocol` 重导出）：

```rust
pub enum HookEvent {
    BeforeCallLLM,      // LLM 调用前
    BeforeToolCall,     // 工具调用前
    AfterToolCall,      // 工具调用后
    TurnEnd,            // Turn 结束
}

pub enum HookAction {
    Continue,                           // 继续执行
    ModifyMessages(Vec<ChatMessage>),  // 修改消息
    ModifyTools(Vec<ToolDefinition>),  // 修改工具
    Modify { messages, tools },         // 同时修改
    Block(String),                      // 阻止执行
}
```

**Hook 上下文**：
```rust
// BeforeCallLLM
pub struct BeforeCallLLMContext<'a> {
    pub messages: &'a [ChatMessage],
    pub tools: &'a [ToolDefinition],
    pub iteration: u32,
}

// ToolCall hooks
pub struct ToolHookContext {
    pub event: HookEvent,
    pub tool_name: String,
    pub tool_call_id: String,
    pub tool_input: serde_json::Value,
    pub tool_result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub thread_id: String,
    pub turn_number: u32,
}
```

## 公共 API

### 新 API（推荐）

```rust
use argus_turn::{Turn, TurnBuilder, TurnConfig};
use argus_protocol::llm::{ChatMessage, LlmProvider};

let turn = TurnBuilder::default()
    .turn_number(1)
    .thread_id("thread-123".to_string())
    .messages(vec![ChatMessage::user("Hello!")])
    .provider(provider)
    .tools(vec![Arc::new(my_tool)])
    .hooks(vec![Arc::new(my_hook)])
    .config(TurnConfig::default())
    .stream_tx(stream_tx)
    .thread_event_tx(thread_event_tx)
    .build()?;

let output = turn.execute().await?;

println!("Token usage: {:?}", output.token_usage);
println!("Messages: {}", output.messages.len());
```

### 旧 API（向后兼容）

```rust
use argus_turn::{TurnInput, TurnInputBuilder, execute_turn};

let input = TurnInputBuilder::new()
    .provider(provider)
    .messages(vec![ChatMessage::user("Hello!")])
    .tool_manager(tool_manager)
    .tool_ids(vec!["echo".to_string()])
    .build()?;

let output = execute_turn(input, TurnConfig::default()).await?;
```

### TurnOutput

```rust
pub struct TurnOutput {
    pub messages: Vec<ChatMessage>,  // 更新的消息历史
    pub token_usage: TokenUsage,     // Token 使用统计
}
```

## 错误处理

所有错误类型：

```rust
pub enum TurnError {
    LlmFailed(LlmError),                      // LLM 调用失败
    LlmCallBlocked { reason: String },        // Hook 阻止 LLM
    ToolNotFound(String),                     // 工具未找到
    ToolExecutionFailed { name, reason },     // 工具执行失败
    ToolCallBlocked { reason: String },       // Hook 阻止工具
    MaxIterationsReached(u32),                // 超过最大迭代
    ContextLengthExceeded(usize),             // 上下文超长
    TimeoutExceeded,                          // 超时
    ProviderNotConfigured,                    // Provider 未配置
    BuildFailed(String),                      // Builder 失败
}
```

## 测试

### 单元测试

```bash
# Turn builder 测试
cargo test turn_builder

# Config 测试
cargo test turn_config

# Error 测试
cargo test turn_error_display
```

### 集成测试

```bash
# 简单集成测试
cargo test test_turn_integration_simple

# 工具调用测试
cargo test test_turn_integration_with_tool_call

# Builder 验证测试
cargo test test_turn_builder_validation

# 重试事件流测试
cargo test test_turn_streams_retry_events
```

### 测试工具

**FlakyProvider**（integration_test.rs:173-287）：
```rust
struct FlakyProvider {
    stream_failures: Mutex<usize>,  // 失败次数计数器
}

let provider = Arc::new(FlakyProvider::new(3));  // 失败 3 次
let retry_provider = Arc::new(RetryProvider::new(
    provider,
    RetryConfig::default()
));

// 前 3 次调用失败，第 4 次成功
let turn = TurnBuilder::default()
    .provider(retry_provider)
    .build()?;

turn.execute().await?;
```

## CLI 工具

**argus-turn-cli** 提供 Turn 测试 interface：

```bash
# 执行简单 Turn
argus-turn-cli execute

# 测试工具执行
argus-turn-cli tool-test

# 测试重试行为
argus-turn-cli mock-test
```

**配置文件** (`turn.toml`)：
```toml
base_url = "https://api.openai.com/v1"
api_key = "sk-..."
model = "gpt-4o-mini"
```

## 开发指南

### 创建自定义 Tool

```rust
use argus_protocol::tool::{NamedTool, ToolError, ToolInput};
use async_trait::async_trait;

struct MyTool;

#[async_trait]
impl NamedTool for MyTool {
    fn name(&self) -> &str {
        "my_tool"
    }

    fn description(&self) -> &str {
        "Does something useful"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "input": { "type": "string" }
            },
            "required": ["input"]
        })
    }

    async fn execute(&self, input: ToolInput)
        -> Result<serde_json::Value, ToolError>
    {
        let args = input.arguments()?;
        let message = args.get("input")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed(
                "Missing 'input' parameter".to_string()
            ))?;

        // 执行逻辑
        Ok(serde_json::json!({ "result": format!("Processed: {}", message) }))
    }
}

// 使用
let turn = TurnBuilder::default()
    .tools(vec![Arc::new(MyTool)])
    .build()?;
```

### 创建自定义 Hook

```rust
use argus_protocol::events::{HookHandler, HookEvent, HookAction, HookContext};
use async_trait::async_trait;

struct MyHook;

#[async_trait]
impl HookHandler for MyHook {
    async fn handle(&self, event: HookEvent, ctx: HookContext)
        -> Result<HookAction, anyhow::Error>
    {
        match event {
            HookEvent::BeforeCallLLM => {
                if let HookContext::BeforeCallLLM(ctx) = ctx {
                    // 检查或修改消息
                    if ctx.messages.len() > 100 {
                        return Ok(HookAction::Block("Too many messages".to_string()));
                    }
                }
                Ok(HookAction::Continue)
            }
            HookEvent::BeforeToolCall => {
                // 记录工具调用
                tracing::info!("Tool call about to execute");
                Ok(HookAction::Continue)
            }
            _ => Ok(HookAction::Continue),
        }
    }
}

// 使用
let turn = TurnBuilder::default()
    .hooks(vec![Arc::new(MyHook)])
    .build()?;
```

### 监听 Turn 事件

```rust
use tokio::sync::broadcast;
use argus_protocol::events::ThreadEvent;

let (stream_tx, _stream_rx) = broadcast::channel(256);
let (thread_event_tx, mut thread_event_rx) = broadcast::channel(256);

let turn = TurnBuilder::default()
    .stream_tx(stream_tx)
    .thread_event_tx(thread_event_tx)
    .build()?;

// 在后台执行 turn
let turn_handle = tokio::spawn(async move {
    turn.execute().await
});

// 监听事件
while let Ok(event) = thread_event_rx.recv().await {
    match event {
        ThreadEvent::Processing { event, .. } => {
            if let LlmStreamEvent::RetryAttempt { attempt, max_retries, error } = event {
                println!("Retry {}/{}: {}", attempt, max_retries, error);
            }
        }
        ThreadEvent::ToolStarted { tool_name, .. } => {
            println!("Tool {} started", tool_name);
        }
        ThreadEvent::ToolCompleted { tool_name, result, .. } => {
            println!("Tool {} completed: {:?}", tool_name, result);
        }
        ThreadEvent::TurnCompleted { .. } => {
            println!("Turn completed!");
        }
        ThreadEvent::TurnFailed { error, .. } => {
            eprintln!("Turn failed: {}", error);
        }
        _ => {}
    }
}

// 等待 turn 完成
let output = turn_handle.await??;
```

## 设计原则

### 1. 拥有资源
- Turn 直接拥有 tools 和 hooks
- 简化生命周期管理
- 避免复杂的 Arc<Mutex<>> 嵌套

### 2. 流式优先
- 内部始终使用流式 API
- 不支持时自动降级
- 实时事件广播

### 3. 向后兼容
- 保留旧 API（`execute_turn`）
- 内部转换为新 API
- 支持渐进式迁移

### 4. Hook 拦截
- 直接迭代 `self.hooks`
- 无需 HookRegistry
- 支持修改消息、工具、阻止执行

## 依赖关系

### 上游依赖
- `argus-protocol`：核心类型（`LlmProvider`、`NamedTool`、`HookHandler`、`ThreadEvent`）
- `argus-llm`：`RetryProvider`（在 CLI 和测试中使用）
- `argus-test-support`：测试辅助工具
- `argus-tool`：`ToolManager`（旧 API）

### 下游消费者
- `argus-thread`：使用 Turn 执行单个对话轮次
- `claw`：高层 API，通过 Thread 间接使用 Turn

## 关键文件路径

| 功能 | 文件 |
|------|------|
| Turn 核心 | `src/turn.rs` |
| 配置类型 | `src/config.rs` |
| 错误类型 | `src/error.rs` |
| 执行包装器 | `src/execution.rs` |
| 集成测试 | `tests/integration_test.rs` |
| CLI 工具 | `src/bin/turn.rs` |

## 常见问题

### Q: 为什么 Turn 直接拥有 tools 和 hooks？
**A**: 简化生命周期管理。之前的 ToolManager/HookRegistry 设计增加了复杂性，每个 Turn 独立拥有资源更清晰。

### Q: 流式事件如何传播到 ThreadEvent？
**A**: Turn 启动时创建 `Event Forwarder` 任务，监听 `stream_tx` 并转换发送到 `thread_event_tx`。

### Q: 如何处理工具调用超时？
**A**: 每个工具调用使用 `tokio::time::timeout`，超时后返回 `TimeoutExceeded` 错误。

### Q: Hook 可以阻止工具调用吗？
**A**: 可以。在 `BeforeToolCall` hook 中返回 `HookAction::Block(reason)`。

### Q: 新旧 API 有什么区别？
**A**: 新 API 使用 `TurnBuilder` 直接传递 tools 和 hooks，旧 API 使用 `ToolManager` 和 `HookRegistry`。内部已统一到新 API。

## 参考资料

- **Turn 执行生命周期**：`src/turn.rs:150-550`
- **事件转发机制**：`src/turn.rs:309-356`
- **集成测试示例**：`tests/integration_test.rs`
- **Hook 系统**：`argus-protocol` crate
