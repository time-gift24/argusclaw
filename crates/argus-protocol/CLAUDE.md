# Argus-Protocol

> 特性：共享类型与 trait 的叶子 crate，承载 llm、tool、mcp、event、plan 与 safety contracts。

## 核心职责

- 定义 `LlmProvider`、`NamedTool`、`HookHandler`、`ProviderResolver` 等跨层抽象
- 提供 `ThreadId` / `SessionId` / `AgentId` / `ProviderId` 等强类型 ID
- 统一承载 `ThreadEvent`、`ThreadPoolSnapshot`、plan 参数、安全输出与 MCP 记录

## 关键模块

- `src/events.rs`：thread / mailbox / pool 事件
- `src/llm/*`：消息、请求、流式事件、provider record
- `src/tool.rs`：tool traits 与执行上下文
- `src/mcp.rs`：MCP records 与绑定关系
- `src/plan.rs`：`UpdatePlanArgs`、step status
- `src/safety.rs`、`src/ssrf.rs`：输出与网络安全边界

## 修改守则

- 这里是叶子模块：不写 SQL、不做 orchestration、不塞业务状态机
- 尽量用枚举、新类型和结构化 record，避免裸字符串协议
- 任何字段或语义变更都会波及大量上层 crate，改动时同步检查文档和测试
