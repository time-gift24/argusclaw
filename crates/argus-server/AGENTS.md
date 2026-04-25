# Argus-Server

> 特性：基于 axum 的实例级管理面与运行状态 transport，在 server 内私有装配 ServerCore。

## 作用域

- 本文件适用于 `crates/argus-server/` 及其子目录。

## 核心职责

- 启动并持有 `ServerCore`
- 在 `ServerCore` 内装配 provider、template、MCP、session、job、thread-pool、tool、auth 等 server 运行组件
- 暴露 health / bootstrap / providers / templates / mcp / runtime / runtime/events / tools 管理 API
- Phase 5 起允许暴露 server-only chat API：sessions / threads / messages / send / cancel / rename / model binding / snapshot / activate / thread events
- 负责 HTTP 请求校验、序列化与错误映射

## 已归档阶段上下文

- Phase 1：新增 `argus-server` 作为 axum 管理 transport，提供 health、bootstrap、providers、templates、MCP 的首批 REST API。
- Phase 2：新增 runtime snapshot 和 runtime SSE；runtime events 只推送运行状态快照，前端可降级为轮询。
- Phase 3A：`argus-server` 从 `argus-wing` 解耦，成为与 desktop facade 平等的应用入口；装配逻辑集中到私有 `ServerCore`。
- Phase 3B/4：补齐 providers/templates/MCP 删除、测试连接、MCP tools 发现，以及 tools 注册表可见性。
- Phase 5A/5B：新增 server-only chat REST API，覆盖 session/thread/message/send/cancel/rename/model binding/snapshot/activate/thread events。
- Phase 6：`POST /api/v1/chat/sessions/with-thread` 接受可选 `name`，旧客户端缺省时使用非空默认名。
- Phase 7：新增 server-only agent run API，支持按 `agent_id + prompt` 触发外部 run，并通过独立持久化 `run_id` 查询运行状态。
- 后续 Web chat 的 TinyRobot、runtime activity 和组件拆分属于 `apps/web` 侧；server 只保持稳定 REST/SSE 契约。

## Public API

- `GET /api/v1/health`
- `GET|PUT /api/v1/account`
- `GET /api/v1/bootstrap`
- `GET /api/v1/runtime`
- `GET /api/v1/runtime/events`
- `GET /api/v1/tools`
- `GET|POST /api/v1/providers`
- `PATCH|DELETE /api/v1/providers/{provider_id}`
- `POST /api/v1/providers/test`
- `POST /api/v1/providers/{provider_id}/test`
- `GET|POST /api/v1/agents/templates`
- `POST /api/v1/agents/runs`
- `GET /api/v1/agents/runs/{run_id}`
- `PATCH|DELETE /api/v1/agents/templates/{template_id}`
- `GET|POST /api/v1/mcp/servers`
- `PATCH|DELETE /api/v1/mcp/servers/{server_id}`
- `POST /api/v1/mcp/servers/test`
- `POST /api/v1/mcp/servers/{server_id}/test`
- `GET /api/v1/mcp/servers/{server_id}/tools`
- `GET|POST /api/v1/chat/sessions`
- `POST /api/v1/chat/sessions/with-thread`
- `PATCH|DELETE /api/v1/chat/sessions/{session_id}`
- `GET|POST /api/v1/chat/sessions/{session_id}/threads`
- `GET|PATCH|DELETE /api/v1/chat/sessions/{session_id}/threads/{thread_id}`
- `PATCH /api/v1/chat/sessions/{session_id}/threads/{thread_id}/model`
- `POST /api/v1/chat/sessions/{session_id}/threads/{thread_id}/activate`
- `GET|POST /api/v1/chat/sessions/{session_id}/threads/{thread_id}/messages`
- `POST /api/v1/chat/sessions/{session_id}/threads/{thread_id}/cancel`
- `GET /api/v1/chat/sessions/{session_id}/threads/{thread_id}/events`

## 修改守则

- `argus-server` 不依赖 `argus-wing`；两者是平等的应用入口
- 下层 manager / repository 的直接装配只允许集中在 `ServerCore`，route handler 只调用 `ServerCore` 暴露的窄方法
- agent run API 的 `run_id` 是独立资源 ID，不允许把普通 chat `thread_id` 当作 run 查询成功
- chat / thread / message API 仅按 server-only 边界扩展；不改 desktop 主流程；thread event SSE 只允许镜像现有 `ThreadEvent`，不新增 desktop rewiring
- 不新增 settings/admin_settings 持久化、repository、migration 或 HTTP route；实例名作为产品展示文案由 bootstrap 返回
- `bootstrap.rs` 只返回 web shell 需要的最小实例初始化摘要，不承担 settings/profile 语义
- `ServerCore::init(database_path)` 负责连接数据库、migration、manager/runtime 装配与 builtin template seed
- `ServerCore::with_pool(pool)` 只用于测试和 in-memory SQLite harness
- 默认数据库路径保持 `DATABASE_URL` 优先，否则 `~/.arguswing/sqlite.db`
- 默认 trace 路径保持 `TRACE_DIR` 优先，否则 `~/.arguswing/traces`
- 默认 bind address 保持 `ARGUS_SERVER_ADDR` 优先，否则 `127.0.0.1:3000`
- response shape、状态码和错误 envelope 改动前必须同步更新 server 测试与 web API client
- 路由保持窄接口，避免把 desktop 命令面直接平移成大而全的 server surface

## 验证

常用验证：

```bash
cargo test -p argus-server -- --nocapture
cargo tree -p argus-server | rg argus-wing
rg 'argus_wing|ArgusWing' crates/argus-server
```

期望：`cargo tree -p argus-server | rg argus-wing` 无匹配；源码中不允许出现 `argus_wing` 依赖入口，`ArgusWing` 只可作为用户可见品牌文案。
