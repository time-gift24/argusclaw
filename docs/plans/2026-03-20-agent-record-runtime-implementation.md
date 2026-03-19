# AgentRecord 运行时共享实现计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Thread 持有 `Arc<AgentRecord>`，共享给 Turn；支持运行时修改配置和单次消息覆盖。

**Architecture:**
- Turn 结构添加 `agent_record: Arc<AgentRecord>` 字段，移除单独的 `max_tokens`, `temperature`, `thinking` 字段
- Turn 在构建 LLM 请求时从 `Arc<AgentRecord>` 提取配置
- Thread.send_message 支持可选的 `MessageOverride` 参数实现单次覆盖

**Tech Stack:** Rust (argus-thread, argus-turn, argus-protocol)

---

## Task 1: 修改 Turn 结构体

**Files:**
- Modify: `crates/argus-turn/src/turn.rs:181-235`

**Step 1: 查看当前 Turn 结构**

确认现有字段位置。

**Step 2: 修改 Turn 结构体**

将：
```rust
    /// Maximum output tokens for LLM requests.
    #[builder(default, setter(strip_option))]
    pub max_tokens: Option<u32>,

    /// Sampling temperature for LLM requests.
    #[builder(default, setter(strip_option))]
    pub temperature: Option<f32>,

    /// Thinking configuration for LLM requests.
    #[builder(default, setter(strip_option))]
    pub thinking: Option<ThinkingConfig>,
```

替换为：
```rust
    /// Agent record (shared from Thread).
    #[builder(default)]
    agent_record: Arc<AgentRecord>,
```

添加 import：
```rust
use argus_protocol::AgentRecord;
```

**Step 3: 运行 clippy 验证**

```bash
cargo clippy --all-targets --all-features --locked -- -D warnings
```

预期：会有 unused field 警告，尚未修改构建逻辑

**Step 4: Commit**

```bash
git add crates/argus-turn/src/turn.rs
git commit -m "refactor(turn): replace max_tokens/temperature/thinking with agent_record Arc"
```

---

## Task 2: 修改 Turn 构建 LLM 请求的逻辑

**Files:**
- Modify: `crates/argus-turn/src/turn.rs:468-478`

**Step 1: 查看当前请求构建逻辑**

确认 470-478 行的代码位置。

**Step 2: 修改请求构建逻辑**

将：
```rust
            let mut request = ToolCompletionRequest::new(messages.clone(), tools.clone());
            if let Some(max_tokens) = self.max_tokens {
                request.max_tokens = Some(max_tokens);
            }
            if let Some(temperature) = self.temperature {
                request.temperature = Some(temperature);
            }
            if let Some(thinking) = &self.thinking {
                request.thinking = Some(thinking.clone());
            }
```

替换为：
```rust
            let mut request = ToolCompletionRequest::new(messages.clone(), tools.clone());
            if let Some(max_tokens) = self.agent_record.max_tokens {
                request.max_tokens = Some(max_tokens);
            }
            if let Some(temperature) = self.agent_record.temperature {
                request.temperature = Some(temperature);
            }
            if let Some(thinking) = &self.agent_record.thinking_config {
                request.thinking = Some(thinking.clone());
            }
```

**Step 3: 运行 clippy**

```bash
cargo clippy --all-targets --all-features --locked -- -D warnings
```

预期：应该通过（除了可能的 unused import）

**Step 4: Commit**

```bash
git add crates/argus-turn/src/turn.rs
git commit -m "refactor(turn): build request from agent_record"
```

---

## Task 3: 修改 Thread 构建 Turn 的逻辑

**Files:**
- Modify: `crates/argus-thread/src/thread.rs:296-318`

**Step 1: 查看当前 Thread.build Turn 逻辑**

确认代码位置。

**Step 2: 修改 Thread.execute_turn_streaming**

将：
```rust
        // Build Turn using TurnBuilder
        let mut turn = TurnBuilder::default()
            .turn_number(turn_number)
            .thread_id(thread_id.clone())
            .messages(self.messages.clone())
            .provider(self.provider.clone())
            .tools(tools)
            .hooks(hooks)
            .config(self.config.turn_config.clone())
            .stream_tx(stream_tx)
            .thread_event_tx(self.event_sender.clone())
            .build()
            .map_err(|e| ThreadError::TurnBuildFailed(e.to_string()))?;

        // Pass agent-level model params directly to avoid builder setter type mismatch
        turn.max_tokens = self.agent_record.max_tokens;
        turn.temperature = self.agent_record.temperature;
        // Default to disabled when not configured in database to avoid provider's default behavior
        turn.thinking = self
            .agent_record
            .thinking_config
            .clone()
            .or_else(|| Some(ThinkingConfig::disabled()));
```

替换为：
```rust
        // Build Turn using TurnBuilder
        let turn = TurnBuilder::default()
            .turn_number(turn_number)
            .thread_id(thread_id.clone())
            .messages(self.messages.clone())
            .provider(self.provider.clone())
            .agent_record(self.agent_record.clone())
            .tools(tools)
            .hooks(hooks)
            .config(self.config.turn_config.clone())
            .stream_tx(stream_tx)
            .thread_event_tx(self.event_sender.clone())
            .build()
            .map_err(|e| ThreadError::TurnBuildFailed(e.to_string()))?;
```

注意：`self.agent_record` 已经是 `Arc<AgentRecord>` 类型，直接 clone 即可共享。

**Step 3: 检查是否需要 import**

确认 `crates/argus-thread/src/thread.rs` 中是否已有 `use argus_protocol::AgentRecord;`，应该已经有了。

**Step 4: 运行 clippy**

```bash
cargo clippy --all-targets --all-features --locked -- -D warnings
```

**Step 5: 运行测试**

```bash
cargo test -p argus-thread -p argus-turn
```

**Step 6: Commit**

```bash
git add crates/argus-thread/src/thread.rs
git commit -m "refactor(thread): pass agent_record Arc to Turn"
```

---

## Task 4: 添加 MessageOverride 类型（单次消息覆盖）

**Files:**
- Create: `crates/argus-protocol/src/message_override.rs`
- Modify: `crates/argus-protocol/src/lib.rs`
- Modify: `crates/argus-thread/src/thread.rs`

**Step 1: 创建 MessageOverride 类型**

```rust
//! Message-level configuration override.

use serde::{Deserialize, Serialize};

use super::llm::ThinkingConfig;

/// Override parameters for a single message send.
/// These override the agent's default configuration for one request only.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageOverride {
    /// Override max_tokens for this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Override temperature for this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Override thinking_config for this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<ThinkingConfig>,
}
```

**Step 2: 在 lib.rs 中导出**

在 `crates/argus-protocol/src/lib.rs` 添加：
```rust
pub mod message_override;
pub use message_override::MessageOverride;
```

**Step 3: 修改 Thread.send_message 签名**

将 `crates/argus-thread/src/thread.rs:255` 的：
```rust
    pub async fn send_message(&mut self, user_input: String) -> Result<(), ThreadError> {
```

改为：
```rust
    pub async fn send_message(&mut self, user_input: String, msg_override: Option<MessageOverride>) -> Result<(), ThreadError> {
```

添加 import：
```rust
use argus_protocol::MessageOverride;
```

**Step 4: 实现 override 逻辑**

在 `send_message` 方法开始处添加：

```rust
        // Apply message-level override if provided
        let effective_record = if let Some(overrides) = msg_override {
            let base = self.agent_record.as_ref();
            Arc::new(AgentRecord {
                max_tokens: overrides.max_tokens.or(base.max_tokens),
                temperature: overrides.temperature.or(base.temperature),
                thinking_config: overrides.thinking_config.clone().or_else(|| base.thinking_config.clone()),
                // Keep other fields from base record
                id: base.id.clone(),
                display_name: base.display_name.clone(),
                description: base.description.clone(),
                version: base.version.clone(),
                provider_id: base.provider_id,
                system_prompt: base.system_prompt.clone(),
                tool_names: base.tool_names.clone(),
            })
        } else {
            self.agent_record.clone()
        };
```

注意：需要从 `AgentRecord` 的 Clone derive 或手动 Clone。如果 `AgentRecord` 没有 Clone，需要添加。

**Step 5: 确认 AgentRecord 有 Clone derive**

检查 `crates/argus-protocol/src/agent.rs:16`：
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRecord {
```

确认有 Clone。

**Step 6: 修改 execute_turn_streaming 使用 effective_record**

在 `send_message` 中把 `effective_record` 传递给 `execute_turn_streaming`：

```rust
        self.execute_turn_streaming(effective_record).await
```

修改 `execute_turn_streaming` 签名：
```rust
    async fn execute_turn_streaming(&mut self, agent_record: Arc<AgentRecord>) -> Result<(), ThreadError> {
```

并把 `self.agent_record.clone()` 替换为 `agent_record.clone()`。

**Step 7: 运行 clippy 和测试**

```bash
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test -p argus-thread -p argus-turn
```

**Step 8: Commit**

```bash
git add crates/argus-protocol/src/message_override.rs crates/argus-protocol/src/lib.rs crates/argus-thread/src/thread.rs
git commit -m "feat: add MessageOverride for single-message config override"
```

---

## Task 5: 验证完整流程

**Step 1: 运行完整检查**

```bash
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test -p argus-thread -p argus-turn
```

**Step 2: 确认没有 regression**

检查所有测试通过。

**Step 3: 最终 commit**

如果前面有 amend 需要：
```bash
git add -A
git commit --amend --no-edit
```

---

## 验证命令

所有任务完成后，运行：
```bash
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test -p argus-thread -p argus-turn
```
