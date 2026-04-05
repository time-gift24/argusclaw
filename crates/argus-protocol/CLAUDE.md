# Argus-Protocol 核心类型

> 特性：整个项目的核心类型库（叶子模块），定义 ThreadId、ThreadEvent、LlmProvider、NamedTool 等核心类型。

## 模块结构

```
src/
├── lib.rs              # 公共 API 导出
├── ids.rs              # 强类型 ID（ThreadId、SessionId、AgentId）
├── events.rs           # ThreadEvent 事件类型
├── hooks.rs            # Hook 系统（HookEvent、HookHandler）
├── agent.rs            # AgentRecord
├── risk_level.rs       # RiskLevel 枚举
├── token_usage.rs      # TokenUsage 统计
├── tool.rs             # NamedTool trait
├── ssrf.rs             # SSRF 保护中间件
├── http_client.rs      # HTTP 客户端
├── config.rs           # 配置类型
├── message_override.rs # 消息覆盖
└── llm/
    ├── mod.rs         # LLM 类型
    ├── messages.rs    # ChatMessage
    ├── provider.rs    # LlmProvider trait
    ├── completion.rs  # CompletionRequest/Response
    └── stream.rs      # LlmStreamEvent
```

## 核心 trait

| Trait | 说明 |
|-------|------|
| `LlmProvider` | LLM 提供者抽象 |
| `NamedTool` | 工具抽象 |
| `HookHandler` | Hook 处理器 |
| `ProviderResolver` | Provider 解析器 |

## 核心类型

### ID 类型
- `ThreadId`：线程 ID（强类型 UUID）
- `SessionId`：会话 ID
- `AgentId`：Agent ID
- `ProviderId`：Provider ID

### 事件类型
- `ThreadEvent`：线程生命周期事件
- `HookEvent`：Hook 触发事件

### 风险等级
```rust
pub enum RiskLevel {
    Low,      // 低风险
    Medium,    // 中风险
    High,      // 高风险
    Critical,  // 极高风险
}
```

## 设计原则

### 1. 叶子模块
- 不依赖任何其他 argus-* crate
- 仅依赖外部 crate（serde、uuid、chrono、thiserror）

### 2. 强类型
- 使用枚举和新类型避免字符串类型
- 避免类型混淆

### 3. 无 I/O
- 不直接处理 I/O
- 所有 I/O 由调用者管理
