## Context

当前 MCP 服务器配置使用 `transport: "Stdio" | "SSE"` 自定义格式。需要改为使用标准的 MCP 配置文件格式。

标准 MCP JSON 配置格式：

```json
{
  "mcpServers": {
    "server-name": {
      "type": "http",
      "url": "https://...",
      "headers": {
        "Authorization": "Bearer ..."
      }
    }
  }
}
```

或 stdio 类型：

```json
{
  "mcpServers": {
    "server-name": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path"]
    }
  }
}
```

## Goals / Non-Goals

**Goals:**
- 使用标准 MCP 配置格式（`type`, `url`, `headers`, `command`, `args`）
- 支持 HTTP/SSE 和 Stdio 两种传输类型
- 保持向后兼容现有的 MCP 工具注册机制

**Non-Goals:**
- 不实现完整的 MCP `mcpServers` JSON 配置文件解析（仅存储单服务器配置）
- 不实现 MCP 客户端认证功能（headers 中的 token 仅传递给 MCP 服务器）

## Decisions

### 1. 数据库 Schema 更新

**选项 A**: 修改现有 `mcp_servers` 表结构
- 将 `transport` 改为 `server_type`
- 将 `command` 改为 `command` (stdio 用)
- 新增 `url` 字段 (http 用)
- 新增 `headers` JSON 字段 (http 用)
- 新增 `args` JSON 字段 (stdio 用)

**选项 B**: 保持现有表结构，新增配置列

**决定**: 选项 A - 直接修改以匹配标准格式

### 2. Rust 类型定义

```rust
pub enum ServerType {
    Http,
    Stdio,
}

pub struct McpServerConfig {
    pub id: McpServerId,
    pub name: String,
    pub display_name: String,
    pub server_type: ServerType,  // 替换原来的 transport
    pub url: Option<String>,      // HTTP 类型用
    pub headers: Option<HashMap<String, String>>, // HTTP 类型用
    pub command: Option<String>,   // Stdio 类型用
    pub args: Option<Vec<String>>, // Stdio 类型用
    pub enabled: bool,
}
```

### 3. 前端类型定义

```typescript
interface McpServerPayload {
  id: number;
  name: string;
  display_name: string;
  server_type: "http" | "stdio";
  url?: string;              // HTTP 类型用
  headers?: Record<string, string>; // HTTP 类型用
  command?: string;          // Stdio 类型用
  args?: string[];           // Stdio 类型用
  enabled: boolean;
}
```

## Risks / Trade-offs

- **数据迁移**: 现有数据库中的 `transport` 字段需要迁移到 `server_type` + 相应配置字段
- **API 变更**: 前端 CRUD API 的请求/响应格式会发生变化

## Migration Plan

1. 创建新的数据库迁移文件 `3__update_mcp_servers_config.sql`
2. 迁移脚本将 `transport = 'Stdio'` 转为 `server_type = 'Stdio'` 并设置 `command`
3. 迁移脚本将 `transport = 'SSE'` 转为 `server_type = 'Http'` 并设置 `url`
4. 更新前端 `mcp-server-form-dialog.tsx` 使用新字段
5. 更新后端 Tauri commands 使用新字段

## Open Questions

- `headers` 中的敏感信息（如 Authorization token）是否需要加密存储？参考现有 provider 的 api_key 处理方式。
