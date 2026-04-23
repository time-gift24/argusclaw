# Desktop 拆分为 Server + Web 设计

**日期：** 2026-04-23

## 目标

这次拆分的首阶段目标，已经从“desktop 功能平移到 web”收敛成更小、更稳的一版：

- 保留现有 `crates/desktop`，首阶段不重接、不改壳、不去登录
- 新增基于 `axum` 的 `crates/argus-server`
- 新增一个基于 `React + Vite` 的 `apps/web`
- 先把 web 做成一个**基本可用的管理台**
- 首阶段不再以 `chat`、`thread monitor`、shared frontend core 为主线

已确认约束：

- Rust 继续作为核心服务端实现，复用现有 workspace crate
- web 继续沿用 `React + Vite`，不引入 `Next.js`
- `v1` 不做多用户，不做登录，按单实例单操作者环境设计
- tool 执行仍是混合模式，但不并入首阶段管理台范围
- 用户端长期可以采用 `REST + SSE`，但**首阶段只要求 `REST`**

## 这版相对上一稿的修正

上一稿的问题，不在技术选型，而在首阶段范围过大：

- 把 `chat + settings + thread monitor` 当成 `v1` 主线
- 把 shared frontend core 当成起手式
- 把 desktop rewiring 放到了第一波
- 把 `sessions / threads / messages / SSE` 一起铺进首版 server surface

这和已经确认的目标相冲突：

- `defer-desktop-rewire`
- `defer-shared-core`
- 前端逻辑应该按管理台场景重写
- 服务端应该先关注管理能力
- `frontend-reset-first`，但阶段产物必须是**可用管理台**

所以这版设计的核心修正是：

**首阶段不是迁移 desktop，而是交付一套新的 server + web 管理面。**

## 当前问题

当前仓库的问题，已经不该再表述成“如何复用 desktop chat 前端”，而应该表述成两个更具体的阻塞：

1. 当前前端的信息架构是围绕 desktop 聊天工作台长出来的，不适合直接做管理台
2. 当前对外入口仍主要是 Tauri command，缺少一层稳定的网络管理 API

因此首阶段最应该拆的，不是 chat store 本身，而是：

- `ArgusWing` 到 `axum` 的管理面 transport
- 一套面向管理场景重新组织过的 web 前端

## 设计决策

### 1. 保留 Rust workspace 作为唯一业务核心

现有 `argus-session`、`argus-agent`、`argus-job`、`argus-tool`、`argus-mcp`、`argus-wing` 继续作为唯一业务核心，不为 web 再造第二套实现。

`argus-wing` 仍是应用 facade，也是 server transport 的唯一稳定入口。新增 `argus-server` 时，不应绕过 facade 直接拼接 repository 或 manager。

### 2. 新增 `crates/argus-server`，但首阶段只暴露管理面

新增独立 crate：`crates/argus-server`。

它在首阶段的职责是：

- 启动 `ArgusWing`
- 暴露实例级管理 API
- 承担序列化、错误映射、配置装配

它在首阶段**不负责**：

- `chat`、`thread`、`message` 主链路
- `SSE` 用户事件流
- desktop 本地能力执行
- desktop 远端接入模式

服务端边界可以明确为：

**首阶段 server 只拥有实例级管理状态，不追求先接住全部运行时状态。**

### 3. 首阶段协议采用 `REST`，`SSE` 后置

虽然长期上 `REST + SSE` 仍然合理，但当前首阶段目标是“可用管理台”，不是“运行时聊天工作台”。因此：

- `v1 phase 1` 只要求 `REST`
- `SSE` 在需要 runtime monitor 或事件驱动页面时再引入

首阶段建议的 API 面：

- `GET /api/v1/health`
- `GET /api/v1/bootstrap`
- `GET /api/v1/providers`
- `POST /api/v1/providers`
- `PATCH /api/v1/providers/:provider_id`
- `GET /api/v1/agents/templates`
- `POST /api/v1/agents/templates`
- `PATCH /api/v1/agents/templates/:template_id`
- `GET /api/v1/mcp/servers`
- `POST /api/v1/mcp/servers`
- `PATCH /api/v1/mcp/servers/:server_id`
- `GET /api/v1/settings`
- `PUT /api/v1/settings`

`bootstrap` 的作用是给 web 管理台提供最小初始化数据，例如：

- 服务实例基础信息
- 可用 provider / template / MCP 摘要
- 版本或运行状态摘要

### 4. Web 首阶段是“重写后的管理台”，不是 desktop 的裁剪版

首阶段新增 `apps/web`，但不再假设它要消费 desktop 的页面组合方式。

web 管理台应该按新的信息架构组织，建议至少包含：

- Overview / Health
- Providers
- Templates
- MCP Servers
- Settings

这里的 `frontend-reset-first`，含义不是只做设计稿或壳子，而是：

- 用新的管理台心智模型重组页面
- 让服务端 API 以管理场景为中心收敛
- 阶段结束时交付一个**基本可用的管理台**

“基本可用”至少意味着：

- 能打开 web 管理台
- 能看到真实服务状态
- 至少核心配置对象能完成真实的读取与修改闭环

### 5. 首阶段不抽 `packages/app-core`

shared frontend core 仍然可能是正确方向，但不应作为第一阶段前提。

原因有两个：

1. 当前 desktop 前端的主心智模型是聊天工作台，而不是管理台
2. 如果一开始先抽共享层，容易把旧状态模型和旧页面组合方式一起复制到 web

因此首阶段明确：

- 不创建 `packages/app-core` 作为必须项
- 不要求先定义 `TauriTransport` / `HttpSseTransport` 双实现
- 不把 desktop store 抽离当成 web 起步条件

shared core 的评估时机，应该放在 web 管理台信息架构稳定之后。

### 6. 首阶段不改 desktop

desktop 在这轮里继续按原有方式工作：

- 不改主路由
- 不移除现有登录相关流
- 不让 desktop 先切 server
- 不在这一轮把 desktop 壳改成共享 shell

这能把第一阶段的风险控制在 server + web 两个新面向上，而不是把已有产品一起拉进迁移。

### 7. 单实例、无登录

服务形态仍明确为：

- 一台 `argus-server` 对应一个操作者环境
- 不做业务登录
- 不做多用户隔离
- provider、template、MCP、settings 都按实例级资源处理

如果部署层需要保护，可依赖内网、反向代理或外层访问控制，但不把登录系统纳入本轮设计。

### 8. `chat`、`monitor`、本地能力节点全部后置

首阶段明确不做：

- `chat` web 化
- `thread monitor` web 化
- `sessions / threads / messages / snapshot`
- `SSE` 事件流
- desktop 作为本地能力节点注册
- desktop 连接远端 `argus-server`

这些内容不是被否定，而是被重新放回正确顺序：

- `Phase 1`：管理台
- `Phase 2`：monitor 与需要事件流的页面
- `Phase 3`：chat / shared core / desktop rewire

## 仓库组织

首阶段推荐的最小目录增量：

- `crates/argus-server`
- `apps/web`

其中：

- `crates/desktop` 保持独立，不并入新的前端共享层
- `apps/web` 可以先作为独立 Vite app 存在
- 如果后续再需要前端 workspace 或 `packages/app-core`，应在第二阶段以后再评估

换句话说，这一版刻意避免为了“未来也许会共享”而先做大规模目录重组。

## 分阶段路线

### Phase 1：可用管理台

交付物：

- `argus-server`
- `apps/web`
- 可用的管理台页面和最小管理 API

范围：

- Providers
- Templates
- MCP Servers
- Settings
- Health / Overview

不包含：

- Chat
- Thread monitor
- SSE
- Shared core
- Desktop rewiring

### Phase 2：运行状态与事件流

只有当管理台稳定且确实需要时，再引入：

- Runtime monitor 页面
- `SSE`
- 更细的 health / runtime 观测

### Phase 3：更深的产品收敛

后续再评估：

- `chat` 服务化
- `packages/app-core`
- desktop 连接远端 server
- desktop 本地能力节点模型

## 测试策略

首阶段测试不再围绕 chat store，而是围绕管理能力：

1. Rust 服务端集成测试
   覆盖 `health`、`bootstrap`、providers、templates、MCP、settings 的 REST 行为和错误 envelope。

2. Web 管理台页面测试
   覆盖页面渲染、导航、表单提交、错误展示、成功回显。

3. 冒烟测试
   验证“打开管理台 -> 读取实例状态 -> 修改一项真实配置 -> 刷新后仍可见”的闭环。

4. Desktop 回归
   本阶段不改 desktop 逻辑，因此只做最低限度的“不受影响”验证，不把 desktop 测试放在主路径。

## 非目标

本轮明确不做：

- desktop 下线
- desktop 去登录
- shared frontend core
- `chat` 和 `thread monitor` 首发 web 化
- `SSE` 首阶段上线
- 多用户 / 多租户 / 权限系统
- 本地 tool 直接上 web

## 风险与控制

主要风险有三类：

1. `ArgusWing` 当前管理面 API 不够顺手
   处理方式是补 facade 方法，而不是让 `argus-server` 绕过 facade 直接拼底层。

2. web 管理台信息架构如果继续沿用 desktop 习惯，会把旧心智模型带过去
   处理方式是明确按管理台重新组织导航和页面，不以 desktop 现有布局为模板。

3. 首阶段如果重新把 desktop 或 shared core 拉进来，会再次扩 scope
   处理方式是把 desktop rewiring 和 shared core 提升为显式后续阶段，不作为首阶段任务。

## 结论

这次 desktop -> server + web 的合理切法，不是“先把 desktop 的核心抽共享，再接 server”，而是：

1. 先做 `argus-server`
2. 先做新的 web 管理台
3. 先把管理能力跑通
4. 再决定哪些前端逻辑值得共享、哪些 desktop 能力值得后续迁移

这样更符合当前真实目标，也更符合 YAGNI。
