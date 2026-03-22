## Why

当前 MCP 服务器配置使用了自定义的 `transport: "Stdio" | "SSE"` 格式，但这不是标准的 MCP 配置方式。标准的 MCP 配置文件使用 `type: "http"` 或 `type: "stdio"`，以及 `url`、`headers` 等标准字段。需要修改实现以遵循 MCP 标准配置格式。

## What Changes

- 将 `transport` 字段改为 `type` 字段，使用标准值 `"http"` 或 `"stdio"`
- HTTP 类型配置使用标准字段：`url`（SSE 端点）和 `headers`（认证等）
- Stdio 类型配置使用标准字段：`command` 和 `args`
- 更新数据库 schema 和 CRUD 操作以匹配新格式
- 更新前端表单和页面以使用新配置格式

## Capabilities

### New Capabilities

- `mcp-server-config`: MCP 服务器配置管理（遵循 MCP 标准配置格式）

### Modified Capabilities

- `mcp-tool`: 更新现有 MCP 工具实现以使用标准配置格式

## Impact

- 数据库：`mcp_servers` 表结构需要修改
- 前端：`/settings/mcp` 页面需要更新表单字段
- 后端：Tauri commands 和 ArgusWing API 需要调整
