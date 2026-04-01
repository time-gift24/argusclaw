# MCP Integration Design

**Date:** 2026-04-01
**Status:** Approved

## Goal

为 Argus 引入一套一等公民的 MCP 能力，覆盖三条主链路：

1. 桌面端新增 MCP 设置页，可配置 `stdio`、`http`、`sse` 类型 server。
2. 配置完成后可测试连接，并获取该 server 的 tool 列表。
3. Agent 模版可以绑定 MCP server，并按 tool 做细粒度启用，在实际 turn 循环中供 LLM 调用。

## Confirmed Decisions

- 传输类型首期支持 `stdio + http + sse`。
- MCP server 只在“设置 / MCP”里统一管理，agent 模版只引用已有 server。
- Agent 模版支持两层配置：
  - 先绑定 MCP server。
  - 再对该 server 下的 tools 做白名单选择。
- 某个已绑定 MCP server 当前不可用时，线程和 turn 继续执行，只是不注入该 server 下的 tools。
- 运行时不在前台 turn 中临时建连；系统初始化时启动后台 supervisor 定时尝试连接 MCP server，并维护状态缓存。
- turn 构建阶段只读取当前连接状态；若状态不可用，则触发后台重试，不阻塞本轮执行。

## Non-Goals

首期不包含以下能力：

- MCP resources / prompts / skills
- Agent 内联定义 MCP server
- OAuth / auth handshake UI
- 多租户或作用域化 MCP 配置
- 将 MCP tool 注册到全局内置 `ToolManager`

## High-Level Architecture

新增一条独立于 provider / knowledge / builtin tools 的 MCP 能力线：

- `argus-protocol`
  - 新增 MCP 可序列化类型，供 repository、wing、desktop 共享。
- `argus-repository`
  - 新增 MCP 相关表与 repository trait，实现配置、发现结果、agent 绑定的持久化。
- `argus-mcp`
  - 新增运行时 crate，负责连接管理、健康检查、discovery、tool 调用适配。
- `argus-wing`
  - 作为 desktop 唯一入口，暴露 MCP CRUD、测试连接、查询 tool 列表、读写 agent 绑定等 API。
- `desktop`
  - 新增 MCP 设置页签，并在 agent 编辑页中加入 MCP server 与 tool 选择区域。
- `argus-agent`
  - 在 turn 开始前从 MCP 运行时读取“当前 ready 的 agent 可用 MCP tools”，动态注入到本轮工具池中。

## Data Model

### MCP Server Record

新增 `McpServerRecord`，建议字段：

- `id`
- `display_name`
- `transport_kind`: `stdio | http | sse`
- `enabled`
- `timeout_ms`
- `command`
- `args`
- `env`
- `url`
- `headers`
- `last_status`
- `last_checked_at`
- `last_success_at`
- `last_error`
- `created_at`
- `updated_at`

说明：

- `stdio` 使用 `command + args + env`。
- `http/sse` 使用 `url + headers`。
- `last_status` 等状态字段用于设置页展示最近结果，不作为强实时真相。

### MCP Tool Discovery Snapshot

新增 `McpDiscoveredToolRecord`，按 server 缓存最近一次成功 `tools/list` 的结果：

- `server_id`
- `tool_name_original`
- `tool_name_slug`
- `description`
- `input_schema_json`
- `annotations_json`
- `last_seen_at`

用途：

- 设置页测试成功后展示 tool 列表。
- Agent 编辑页不必强依赖在线连接也能读取上次发现结果。
- 运行时为 MCP tool 适配器提供稳定的 schema 描述。

### Agent Bindings

新增两张关系表：

`agent_mcp_servers`

- `agent_id`
- `server_id`

`agent_mcp_tools`

- `agent_id`
- `server_id`
- `tool_name_original`

绑定语义：

- 绑定了 server，但 `agent_mcp_tools` 没有任何记录：表示该 server 下全部 tools 可用。
- 绑定了 server，且存在 `agent_mcp_tools` 记录：表示该 server 下仅白名单内 tools 可用。

### AgentRecord Boundary

首期不将复杂 MCP 结构直接塞进 `AgentRecord`。

- `AgentRecord.tool_names` 继续只表示内置工具。
- MCP 绑定通过独立 API 读写，模式与当前 `knowledge` 绑定更接近。

这样可以降低对现有 template CRUD 的侵入。

## Naming Strategy

运行时注入给 LLM 的 MCP tool 统一命名为：

`mcp__<server_slug>__<tool_slug>`

示例：

- `mcp__slack__post_message`
- `mcp__github__list_issues`

约束：

- 数据库绑定关系保存 `server_id + tool_name_original`，不只保存拼接后的全局名。
- 运行时使用 slug 名避免与内置工具或其他 server 下的 tool 重名。

## Runtime Design

### Background Supervisor

在 `ArgusWing::init()` 和 `ArgusWing::with_pool()` 初始化期间启动 `McpRuntimeSupervisor`。

职责：

- 加载所有 `enabled` MCP server。
- 按 transport 建立连接。
- 首次连接成功后立即做一次 `tools/list`。
- 将连接状态与发现结果写入内存缓存，并刷新 discovery snapshot。
- 周期性做健康检查与重连。
- 接收前台“优先重试”信号。

状态建议：

- `ready`
- `connecting`
- `retrying`
- `failed`
- `disabled`

辅助元数据：

- `last_checked_at`
- `last_success_at`
- `last_error`
- `discovered_tool_count`

### Turn Preparation

保持 `Thread::build_turn()` 同步，不在其中执行联网操作。

在 `Thread::begin_turn()` 中增加 MCP 准备步骤：

1. 读取当前 agent 的 MCP server 绑定与 tool 白名单。
2. 向 `AgentMcpToolResolver` 查询这些 server 的当前状态与 discovery 缓存。
3. 对处于 `ready` 状态的 server，构造 MCP `NamedTool` 适配器并注入本轮工具池。
4. 对非 `ready` 状态的 server，仅记录 notice，并向后台 supervisor 发送重试信号。

结果：

- 本轮 turn 只消费当前缓存状态。
- 不在前台阻塞等待 MCP 建连。

### MCP Tool Adapter

每个 MCP tool 以运行时适配器实现 `NamedTool`：

- `name()` 返回 `mcp__<server_slug>__<tool_slug>`
- `definition()` 返回 discovery snapshot 提供的 schema 与描述
- `execute(args)` 转发到 `McpConnectionManager::call_tool(server_id, original_tool_name, args)`

对 `Turn` 来说，MCP tool 与 builtin tool 没有执行语义差异。

## Desktop UX

### Settings / MCP

新增 `设置 / MCP` 页签，结构参考现有 provider 设置页：

- 列表页
  - 展示名称、transport、状态、最近检查时间、发现到的 tool 数量
  - 支持新增、编辑、删除、手动测试连接
- 编辑页
  - 根据 transport 切换不同表单
  - 未保存时支持 `test input`
  - 已保存时支持 `test connection`
  - 右侧展示最近一次 discovery 到的 tool 列表

### Agent Editor

在现有 agent 编辑页新增 MCP 区块：

- 加载 server 列表与各 server 最近 discovery 的 tools
- 先勾选 server
- 再展开选择该 server 下允许的 tools
- 若某 server 尚无 discovery snapshot，允许绑定 server，但不给出 tool 细选，并提示先测试连接

## Error Handling

### stdio

重点处理：

- command 不存在
- 启动超时
- 子进程提前退出
- MCP 协议握手失败

### http / sse

重点处理：

- URL 不可达
- 请求超时
- 非 2xx 响应
- SSE 流中断
- 非法 MCP 响应

### User-Facing Behavior

- 设置页测试连接返回结构化结果：
  - `status`
  - `message`
  - `latency_ms`
  - `checked_at`
  - `discovered_tools`
- 运行时失败不写进模型上下文。
- 线程通过 `ThreadEvent::Notice` 向前端发送轻提示，例如：
  - `Slack MCP 当前不可用，本轮未注入 3 个工具，可在设置页重试连接。`

## Testing Strategy

### Repository

- migration 测试覆盖新建表与索引
- SQLite round-trip 测试覆盖：
  - `mcp_servers`
  - `mcp_server_tools`
  - `agent_mcp_servers`
  - `agent_mcp_tools`

### Runtime

- supervisor 初始化时加载 enabled server
- 连接成功后刷新 discovery snapshot
- 失败后指数退避
- 收到优先重试信号后能提前尝试连接

### Agent / Turn

- `begin_turn()` 在 server `ready` 时成功注入 MCP tools
- `begin_turn()` 在 server `failed` 时不注入 tools，但会发 notice 并触发后台重试
- builtin tools 与 MCP tools 同时存在时，名称与过滤逻辑正确

### Desktop

- 设置页新增 / 编辑 / 测试连接 / 查看 discovery tools
- Agent 编辑页绑定 server、细选 tools、保存再回读

## Phase 1 Scope

首期交付边界：

- 支持 `stdio + http + sse`
- 仅支持 MCP tools
- MCP server 统一在设置页管理
- Agent 模版支持 server 绑定与 tool 白名单
- 后台 supervisor 维护连接健康与 discovery
- turn 前只读取当前状态并动态注入 ready 的 MCP tools
- server 不可用时线程继续执行

## Deferred Work

后续可演进但不纳入首期：

- MCP resources / prompts
- OAuth / auth re-entry UI
- Agent 私有 inline MCP server
- 更细的权限策略与审批规则
- 会话级或线程级 MCP 覆盖
- 自动刷新 tool schema 的增量事件通知

