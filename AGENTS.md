熟悉 CLAUDE.md

## MCP 能力摘要

- 服务管理：支持 MCP Server 的增删改查（`list/get/upsert/delete`），并持久化 `status/last_checked_at/last_success_at/last_error/discovered_tool_count`。
- 传输协议：支持 `stdio`、`http`、`sse` 三种 transport。
  - `stdio`：`command + args + env`
  - `http/sse`：`url + headers`
- 连接测试：支持对“未保存配置”与“已保存配置”做连接测试，返回结构化结果（状态、耗时、消息、发现的工具）。
- 工具发现：连接成功后写入并维护 server 的 discovery 快照（`mcp_server_tools`），前端可查看最新 discovered tools。
- Agent 绑定：支持给 Agent 绑定多个 MCP Server，并支持两种授权模式：
  - 全量授权（`allowed_tools = null`）
  - 白名单授权（`allowed_tools = [tool_name_original, ...]`）
- 运行时注入：会话线程在运行时通过 `McpToolResolver` 按 Agent 绑定解析并注入 MCP 工具。
  - 仅 `ready` 的 server 参与注入；
  - server 不可用时进入重试流程并记录状态。
- 连接健壮性：内置 supervisor 轮询、退避重试、ready 重检；支持 streamable HTTP 多协议版本握手回退及 legacy SSE 兼容。

## MCP 变更边界（必须遵守）

- MCP 需求默认只改 MCP 相关模块（settings/mcp、agent-mcp-binding、tauri MCP commands、argus-mcp、repository MCP 表/trait）。
- 未明确要求时，不修改 chat 的 store/runtime/ui 逻辑与消息语义。
