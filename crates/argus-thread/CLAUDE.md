# Argus-Thread 线程管理

> 特性：线程管理，协调 turn 和 tool 执行，管理消息历史和事件广播。

## 模块结构

```
src/
├── lib.rs              # 公共 API 导出
├── thread.rs           # Thread：核心线程实现
├── config.rs           # ThreadConfig：线程配置
├── compact.rs          # Compactor：消息压缩策略
├── types.rs            # ThreadInfo、ThreadState
└── error.rs           # ThreadError、CompactError
```

## 核心概念

### 1. Thread 结构

**Thread** 是多轮对话会话的管理器：

```rust
pub struct Thread {
    id: ThreadId,                           // 强类型 ID
    agent_record: Arc<AgentRecord>,         // Agent 配置
    session_id: SessionId,                  // 父会话 ID
    title: Option<String>,                  // 可选标题
    messages: Vec<ChatMessage>,             // 消息历史
    provider: Arc<dyn LlmProvider>,         // LLM 提供者
    tool_manager: Arc<ToolManager>,         // 工具管理器
    compactor: Arc<dyn Compactor>,          // 上下文压缩器
    hooks: Option<Arc<HookRegistry>>,       // Hook 注册表
    config: ThreadConfig,                   // 线程配置
    token_count: u32,                       // 当前 token 数
    turn_count: u32,                        // Turn 计数
}
```

**关键特性**：
- 多轮对话：顺序执行多个 Turn
- 消息历史：内存中维护完整消息历史
- 事件广播：通过 broadcast channel 实时推送事件
- 上下文压缩：自动压缩过长的上下文

### 2. Compactor 策略

**Compactor** trait 定义上下文压缩策略：

```rust
#[async_trait]
pub trait Compactor: Send + Sync {
    async fn compact(&self, context: &mut CompactContext<'_>) -> Result<(), CompactError>;
    fn name(&self) -> &'static str;
}
```

**内置实现**：
- `KeepRecentCompactor`：保留最近 N 条消息
- `KeepTokensCompactor`：保留最近 N 个 token

### 3. ThreadConfig

```rust
pub struct ThreadConfig {
    pub max_tokens: Option<u32>,           // 最大 token 数（触发压缩）
    pub compactor_name: String,            // 压缩策略名
}
```

## 公共 API

```rust
use argus_thread::{Thread, ThreadBuilder, ThreadConfig};

// 创建 Thread
let thread = ThreadBuilder::new()
    .provider(my_provider)
    .compactor(my_compactor)
    .build()?;

// 发送消息（自动创建 Turn）
thread.send_message("Hello!".to_string()).await?;

// 监听事件
let mut rx = thread.subscribe();
while let Ok(event) = rx.recv().await {
    println!("Event: {:?}", event);
}
```

## 依赖关系

### 上游依赖
- `argus-protocol`：ThreadId、ThreadEvent、HookRegistry 等
- `argus-turn`：Turn 执行
- `argus-tool`：ToolManager

### 下游消费者
- `argus-session`：Session 管理 Thread

## 设计原则

### 1. Builder 模式
- 使用 `ThreadBuilder` 创建 Thread
- 必需字段在构建时验证

### 2. 事件驱动
- Thread 通过 broadcast channel 广播事件
- 订阅者（CLI、Tauri）实时接收更新

### 3. 上下文压缩
- Compactor 策略可插拔
- 自动在 token 限制前压缩
