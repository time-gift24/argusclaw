## ADDED Requirements

### Requirement: MCP 服务器支持标准 HTTP 类型配置

系统 SHALL 支持使用标准 MCP 配置格式配置 MCP 服务器，包括 HTTP 类型配置。

### Requirement: MCP 服务器支持标准 Stdio 类型配置

系统 SHALL 支持使用标准 MCP 配置格式配置 MCP 服务器，包括 Stdio 类型配置。

### Requirement: MCP 服务器配置字段

MCP 服务器配置 SHALL 包含以下字段：

- `name`: 服务器标识名称（用于工具命名，如 `mcp_filesystem_read`）
- `display_name`: 显示名称
- `server_type`: 服务器类型，`"http"` 或 `"stdio"`
- `enabled`: 是否启用
- 对于 `server_type = "http"`:
  - `url`: SSE 端点 URL
  - `headers`: HTTP 请求头（可选，用于认证等）
- 对于 `server_type = "stdio"`:
  - `command`: 启动命令
  - `args`: 命令参数数组

### Requirement: MCP 服务器 CRUD 操作

系统 SHALL 提供以下 MCP 服务器配置管理操作：

- 创建 MCP 服务器配置
- 读取 MCP 服务器配置
- 更新 MCP 服务器配置
- 删除 MCP 服务器配置
- 测试 MCP 服务器连接

### Requirement: MCP 工具自动注册

系统 SHALL 在启动时自动加载所有已启用且配置完整的 MCP 服务器，并将其提供的工具注册到工具管理器。

### Requirement: MCP 工具命名规范

系统 SHALL 使用 `mcp_{server_name}_{tool_name}` 格式命名 MCP 工具，例如 `mcp_zread_read`。

## REMOVED Requirements

### Requirement: 使用 transport 字段区分传输类型

**Reason**: 被标准的 `server_type` + 配置字段替代

**Migration**: 使用新的配置格式
