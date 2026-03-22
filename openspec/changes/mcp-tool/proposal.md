## Why

用户需要能够将外部 MCP (Model Context Protocol) 服务器连接到指定的 agent，使 agent 能够使用 MCP 服务器提供的工具。当前 argusclaw 仅支持内置工具 (shell, read, glob 等)，无法接入 MCP 生态系统中丰富的工具集。

## What Changes

- **新增 `mcp_servers` 数据表**：存储 MCP 服务器配置（名称、传输类型、命令/URL、加密的 auth token）
- **新增 `McpServerConfig` 类型**：定义 MCP 服务器配置的强类型
- **新增 `McpToolError` 错误变体**：MCP 工具调用失败的错误类型
- **新增 `argus-tool::mcp` 模块**：包含 `McpClientPool`（管理多个 MCP 服务器连接）和 `McpTool`（包装 MCP 工具为 `NamedTool` 接口）
- **新增 MCP 服务器 CRUD API**：通过 Tauri commands 暴露给前端
- **新增前端 MCP 管理页面**：`/settings/mcp` 页面，列表展示、添加、编辑、删除、测试连接
- **Agent 配置集成**：Agent 的 `tool_names` 支持引用 MCP 工具，命名格式为 `mcp_{server_name}_{tool_name}`

## Capabilities

### New Capabilities

- `mcp-server-config`: MCP 服务器配置管理 - 支持 Stdio 和 SSE 两种传输类型，存储加密的 auth token，提供连接测试功能
- `mcp-client`: MCP 客户端实现 - 使用 rust-mcp-sdk 连接 MCP 服务器，动态发现工具，将 MCP 工具适配为 `NamedTool` 接口
- `mcp-tool-naming`: MCP 工具命名规范 - MCP 服务器 "filesystem" 的工具 "read" 在 agent 配置中引用为 "mcp_filesystem_read"

### Modified Capabilities

- (无 - 不修改现有 spec 行为)

## Impact

- **新增依赖**：`rust-mcp-sdk` crate
- **数据库**：新增 `mcp_servers` 表
- **新增模块**：`crates/argus-tool/src/mcp/`
- **API 变更**：Tauri commands 新增 MCP 相关命令
- **前端**：新增 `/settings/mcp` 路由和页面组件
