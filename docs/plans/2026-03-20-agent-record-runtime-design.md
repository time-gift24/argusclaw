# AgentRecord 运行时共享设计

## Context

当前 Thread 和 Turn 之间传递 agent 配置的方式不够简洁：

```rust
// Thread 中构建 Turn 时
let mut turn = TurnBuilder::default()
    ...
    .build()...;

// 还需要 post-build mutation
turn.max_tokens = self.agent_record.max_tokens;
turn.temperature = self.agent_record.temperature;
turn.thinking = self.agent_record.thinking_config.clone()...;
```

此外，用户无法在运行时动态修改 agent 配置进行测试。

## 目标

1. Thread 持有 `Arc<AgentRecord>`，以共享方式传递给 Turn
2. Turn 直接接收 `Arc<AgentRecord>`，按需提取配置
3. 支持运行时修改 agent 配置（前端 + 后端）
4. 支持单次消息发送时的临时配置覆盖

## 设计

### 数据结构

**Thread 结构变更：**

```rust
pub struct Thread {
    ...
    /// Agent record (shared with Turn).
    /// Can be modified at runtime for testing.
    agent_record: Arc<AgentRecord>,
    ...
}
```

**Turn 结构变更：**

```rust
pub struct Turn {
    ...
    /// Agent record (shared from Thread).
    agent_record: Arc<AgentRecord>,
    ...
}
```

### 合并逻辑

Turn 在构建 LLM 请求时，从 `Arc<AgentRecord>` 按需提取：

```rust
impl Turn {
    fn build_request(&self, messages: Vec<ChatMessage>, tools: Vec<ToolDefinition>) -> ToolCompletionRequest {
        let mut request = ToolCompletionRequest::new(messages, tools);

        // 从共享 AgentRecord 提取配置
        if let Some(max_tokens) = self.agent_record.max_tokens {
            request.max_tokens = Some(max_tokens);
        }
        if let Some(temperature) = self.agent_record.temperature {
            request.temperature = Some(temperature);
        }
        if let Some(thinking) = &self.agent_record.thinking_config {
            request.thinking = Some(thinking.clone());
        }

        request
    }
}
```

### Thread.build_turn 辅助方法

```rust
impl Thread {
    fn build_turn(&self, messages: Vec<ChatMessage>, tools: Vec<Arc<dyn NamedTool>>, hooks: Vec<Arc<dyn HookHandler>>) -> Result<Turn, ThreadError> {
        TurnBuilder::default()
            .turn_number(self.turn_count + 1)
            .thread_id(self.id.to_string())
            .messages(messages)
            .provider(self.provider.clone())
            .agent_record(self.agent_record.clone())  // 共享 Arc
            .tools(tools)
            .hooks(hooks)
            .config(self.config.turn_config.clone())
            .stream_tx(stream_tx)
            .thread_event_tx(self.event_sender.clone())
            .build()
            .map_err(|e| ThreadError::TurnBuildFailed(e.to_string()))
    }
}
```

### 单次消息覆盖

前端发送消息时可以传入临时配置：

```rust
pub struct MessageOverride {
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub thinking_config: Option<ThinkingConfig>,
}

impl Thread {
    pub async fn send_message(
        &mut self,
        user_input: String,
        override: Option<MessageOverride>,
    ) -> Result<(), ThreadError> {
        let mut effective_record = self.agent_record.clone();

        // 如果有临时覆盖，创建一个修改后的 AgentRecord
        if let Some(overrides) = override {
            effective_record = Arc::new(AgentRecord {
                max_tokens: overrides.max_tokens.or(effective_record.max_tokens),
                temperature: overrides.temperature.or(effective_record.temperature),
                thinking_config: overrides.thinking_config.or(effective_record.thinking_config.clone()),
                ..(*effective_record).clone()
            });
        }

        // 构建 Turn 时使用 effective_record
        ...
    }
}
```

### 前端临时保存能力

前端需要维护一个本地的"运行时 AgentRecord 状态"：

```
用户修改温度 → 前端更新本地状态 →下次发送消息时携带完整 AgentRecord
```

前端数据结构：
```typescript
interface RuntimeAgentState {
  agentRecord: AgentRecord;
  // 是否有未保存的修改
  isDirty: boolean;
}
```

API 层面通过 `PATCH /threads/:id/agent-config` 更新运行时配置。

## 实现步骤

1. **修改 Turn 结构**
   - 添加 `agent_record: Arc<AgentRecord>` 字段
   - 移除单独的 `max_tokens`, `temperature`, `thinking` 字段
   - 修改 `build_request` 从 agent_record 提取配置

2. **修改 Thread.build_turn**
   - 接收 `Arc<AgentRecord>` 参数
   - 传递给 TurnBuilder

3. **实现 send_message 覆盖**
   - 添加 `MessageOverride` 参数
   - 临时创建修改后的 AgentRecord

4. **前端集成**
   - 维护 RuntimeAgentState
   - 提供临时配置 UI
   - 发送消息时携带 AgentRecord

## 验证

```bash
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test -p argus-thread -p argus-turn
```
