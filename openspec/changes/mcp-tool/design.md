## Context

argusclaw 目前通过 `ToolManager` 管理内置工具（shell, read, glob, grep, http），每个工具实现 `NamedTool` trait。用户无法添加外部 MCP 服务器提供的工具。

目标是通过 rust-mcp-sdk 实现 MCP client 功能，使用户能够：
1. 在前端配置 MCP 服务器（名称、传输类型、认证）
2. 测试 MCP 服务器可达性
3. 将 MCP 服务器的工具连接到指定的 agent

现有技术栈：
- **持久化层**：SQLite via sqlx，迁移管理，AES-256-GCM 加密（argus-crypto）
- **工具系统**：ToolManager + NamedTool trait
- **前端**：React 19 + Tauri 2.0 + shadcn/ui
- **错误处理**：thiserror，每个 crate 定义自己的 error.rs

## Goals / Non-Goals

**Goals:**
- 使用 rust-mcp-sdk 实现 MCP client (Stdio 和 SSE 传输)
- MCP 服务器配置的 CRUD（列表、创建、更新、删除、测试连接）
- MCP 工具通过 `mcp_{server}_{tool}` 命名接入 agent
- auth token 加密存储（复用 argus-crypto 的 Cipher）
- 动态工具发现：启动时连接 MCP 服务器并获取工具列表

**Non-Goals:**
- 不实现 MCP server（仅 client）
- 不支持 MCP prompts、resources（仅 tools）
- 不支持 MCP 服务器认证的细粒度配置（token 级别）
- 不在前端暴露 MCP 协议细节

## Decisions

### 1. 新建 `mcp` 子模块在 `argus-tool` 而非独立 crate

**Decision**: 将 MCP client 实现放在 `crates/argus-tool/src/mcp/` 下

**Rationale**:
- MCP client 在概念上是工具的提供者，与 `ToolManager` 紧耦合
- 减少 crate 数量，降低复杂度
- 避免循环依赖问题（MCP client 不需要依赖 argus-repository）

**Alternatives**:
- 新建 `argus-mcp` crate：更干净但增加维护负担，且 MCP client 只被 argus-tool 使用

### 2. `McpServerConfig` 定义在 `argus-protocol`

**Decision**: `McpServerConfig` 类型和 `TransportType` 枚举放在 `argus-protocol/src/mcp.rs`

**Rationale**:
- `argus-protocol` 是叶子模块，无内部依赖，适合定义核心配置类型
- 其他 crate（repository, tool）都依赖 protocol，便于共享类型
- 遵循现有模式（`AgentRecord`, `LlmProviderRecord` 也在 protocol 定义）

### 3. 工具命名：`mcp_{server_name}_{tool_name}`

**Decision**: MCP 工具名称格式为 `mcp_{server_name}_{tool_name}`

**Rationale**:
- 明确标识为 MCP 工具，与原生工具区分
- server_name 确保多服务器场景下工具名不冲突
- 简单直观，LLM 容易理解

**Example**:
```
Server: "filesystem", Tool: "read" → "mcp_filesystem_read"
Server: "github", Tool: "create_issue" → "mcp_github_create_issue"
```

### 4. 数据库表 `mcp_servers`

**Decision**: 新建 `mcp_servers` 表，结构如下：

```sql
CREATE TABLE mcp_servers (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT NOT NULL UNIQUE,           -- "filesystem" (用于工具命名)
    display_name TEXT NOT NULL,                    -- "Filesystem MCP"
    transport    TEXT NOT NULL,                    -- "stdio" | "sse"
    command      TEXT,                             -- for stdio, e.g., "npx -y @modelcontextprotocol/server-filesystem"
    url          TEXT,                             -- for sse
    auth_token   BLOB,                             -- AES-256-GCM encrypted
    auth_nonce   BLOB,
    enabled      INTEGER NOT NULL DEFAULT 1,
    created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

**Rationale**:
- 复用现有的加密模式（`encrypted_api_key` / `api_key_nonce` 列）
- `name` 唯一约束防止工具命名冲突
- `enabled` 标志支持软禁用

### 5. McpClientPool 管理连接生命周期

**Decision**: `McpClientPool` 在 `ArgusWing::init()` 时创建并注册 MCP 工具到 `ToolManager`

```rust
pub struct McpClientPool {
    clients: RwLock<HashMap<String, Arc<ClientRuntime>>>,
    server_configs: DashMap<String, McpServerConfig>,
}
```

**Rationale**:
- 在 `ArgusWing::init()` 中初始化，确保 MCP 工具在应用启动时可用
- 连接复用：避免每次工具调用都启动进程
- 线程安全：`DashMap` 支持并发访问

**初始化流程**（遵循现有 `register_default_tools()` 模式）：

```rust
// desktop/src-tauri/src/lib.rs
let wing = rt.block_on(ArgusWing::init(None)).expect("初始化失败");

// 注册原生工具
wing.register_default_tools();

// 注册 MCP 工具（新增）
rt.block_on(wing.register_mcp_tools()).expect("MCP 工具注册失败");
```

**注意**：
- MCP 工具注册是 async，因为需要连接 MCP 服务器并发现工具
- 遵循现有模式：在 desktop 层调用，而不是在 `ArgusWing::init()` 中
- **单个 MCP 服务器连接失败不影响其他服务器**：迭代所有 enabled 服务器，失败者记录错误并继续
- 应用启动**不阻塞**，但失败的服务器工具在当期 session 不可用

### 6. 错误映射：`McpToolError` 变体

**Decision**: 在 `argus-protocol/src/tool.rs` 的 `ToolError` 枚举中添加 `McpToolError` 变体

```rust
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    // ... existing variants ...

    #[error("MCP tool error [{server}/{tool}]: {context}")]
    McpToolError {
        server: String,
        tool: String,
        context: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}
```

**Rationale**:
- 保持 `ToolError` 单一枚举，无需新增错误类型
- `source` 保留原始错误用于调试
- 符合项目错误处理惯例（thiserror + context）

## Risks / Trade-offs

[Risk] MCP 服务器进程僵死（Stdio 传输）
→ **Mitigation**: 设置进程超时，超时后强制终止；连接池定期检测并重连

[Risk] SSE 连接断开导致工具调用失败
→ **Mitigation**: 工具调用失败后自动重连一次，第二次失败才返回错误

[Risk] 工具定义动态发现增加启动延迟
→ **Mitigation**: 工具发现异步进行，不阻塞 agent 初始化；缓存工具定义

[Risk] Protocol 版本不兼容
→ **Mitigation**: rust-mcp-sdk 提供 `ensure_server_protocole_compatibility()` 检查，版本不匹配时拒绝连接并返回明确错误

[Trade-off] 全量工具暴露 vs 细粒度过滤
→ **Current**: Agent 启用 MCP 服务器时获得其所有工具
→ **Future**: 可在 `AgentRecord` 的 `tool_names` 中筛选特定工具

## Migration Plan

**Phase 1**: 数据库迁移
- 新增 `2__add_mcp_servers.sql` 迁移文件
- 无数据迁移（全新表）

**Phase 2**: 核心实现
- `argus-protocol`: 添加 `McpServerConfig`, `TransportType`, `ToolError::McpToolError`
- `argus-tool`: 添加 `mcp/` 子模块，实现 `McpClientPool`, `McpTool`

**Phase 3**: 持久化层
- `argus-repository`: 添加 `mcp_server` 表的 CRUD 实现

**Phase 4**: API 层
- `desktop/src-tauri`: 添加 MCP 相关 Tauri commands
- `argus-wing`: 暴露 MCP 管理接口

**Phase 5**: 前端
- 添加 `/settings/mcp` 页面
- Agent 编辑页支持选择 MCP 工具

## Open Questions

1. **测试连接超时**: Stdio 和 SSE 的连接测试超时时间建议设为多少？（建议 10s）

2. **进程管理**: Stdio 传输时，进程由谁管理？Pool 还是独立进程管理器？

3. **工具过滤**: 是否需要在 `AgentRecord` 添加 `mcp_server_names` 字段控制可用 MCP 服务器？（当前简化版暂不实现）

---

## Review 检查点

### 初始化流程（关键）
```
desktop/src-tauri/src/lib.rs
  → ArgusWing::init()
  → register_default_tools()         // 原生工具
  → register_mcp_tools().await      // MCP 工具（新增）
```

**检查点**：
- [ ] `register_mcp_tools()` 是 async 方法
- [ ] 单个 MCP 服务器连接失败**不阻塞**其他服务器注册
- [ ] 失败服务器有清晰的错误日志
- [ ] 原生工具和 MCP 工具注册顺序正确
