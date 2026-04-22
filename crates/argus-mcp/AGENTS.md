# Argus-MCP

> 特性：MCP server runtime、连接监督与 discovered tool 到 `NamedTool` 的适配层。

## 作用域

- 本文件适用于 `crates/argus-mcp/` 及其子目录。

## 核心职责

- `McpRuntime` 维护 MCP server 连接、snapshot、状态切换与工具发现
- `supervisor.rs` 负责 runtime 生命周期与重连监督
- `tool_adapter.rs` 把 discovered MCP tools 包装成可执行的 `NamedTool`
- `runtime.rs` 也承载连接测试、transport 适配与 session 抽象

## 关键模块

- `src/runtime.rs`
- `src/supervisor.rs`
- `src/tool_adapter.rs`
- `src/error.rs`

## 公开入口

- `McpRuntime`
- `McpRuntimeHandle`
- `McpRuntimeSnapshot`
- `McpToolAdapter`
- `McpToolExecutor`

## 依赖边界

- 上游依赖：`argus-protocol`、`argus-repository`
- 下游消费者：`argus-wing`、`argus-session`、`argus-tool`

## 修改守则

- MCP 连接状态与工具发现结果应统一经过 runtime snapshot 暴露，不要旁路维护第二份状态
- transport 适配、supervision、tool execution 的职责边界不要混在一起
- 新 transport 或新状态字段要同步检查 repository、desktop 与 settings 页面消费者
