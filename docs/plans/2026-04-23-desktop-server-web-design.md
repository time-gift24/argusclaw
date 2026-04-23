# Desktop 拆分为 Server + Web 设计

**日期：** 2026-04-23

## 目标

把当前以 Tauri 为入口的 desktop 演进成三部分协作的形态：

- 保留现有 `crates/desktop` 桌面端
- 新增基于 `axum` 的 `argus-server`
- 新增基于 `React + Vite` 的 web 前端

首版只覆盖 `chat + settings + thread monitor` 三条主链路，并满足以下已确认约束：

- Rust 继续作为核心服务端实现，复用现有 workspace crate
- web 继续沿用 `React + Vite` 生态，不引入 `Next.js`
- desktop 和 web 最终共用后端共享状态
- tool 执行采用混合模式：共享能力上收服务端，本地强能力保留在 desktop
- `v1` 不做多用户，不做登录，按“单实例单操作者环境”设计
- 用户端实时协议采用 `REST + SSE`，不强上 `WebSocket`

## 当前问题

当前仓库虽然已经把核心能力下沉到 Rust workspace，但对外产品形态仍然偏 desktop 一体化：

1. `crates/desktop/src-tauri` 通过 Tauri command 暴露 API，前端直接依赖 `invoke`
2. 前端状态通过 Tauri `"thread:event"` 事件流收口，和浏览器网络协议没有抽象层
3. `chat`、`settings`、`thread monitor` 的页面和 store 都默认运行在 desktop 壳里

这导致两个直接问题：

- 现有前端逻辑难以原样复用到 web
- 想增加服务端时，容易误把“重写 UI”当成主任务，而真正该拆的是 transport 边界

问题核心不是业务能力缺失，而是 transport 与前端状态模型耦合过紧。

## 设计决策

### 1. 保留 Rust workspace 作为唯一业务核心

现有 `argus-session`、`argus-agent`、`argus-job`、`argus-tool`、`argus-mcp`、`argus-wing` 继续作为唯一业务核心，不为 web 再造一套实现。

`argus-wing` 仍是应用 facade，也是 server 与 desktop bridge 的共同能力入口。新增服务端时，不应绕过 facade 直接让 HTTP handler 依赖底层 repository 或 manager。

### 2. 新增 `crates/argus-server` 作为网络 transport 层

新增一个独立 crate：`crates/argus-server`。

它的职责是：

- 启动 `ArgusWing`
- 暴露 `REST` API
- 暴露 `SSE` 实时事件流
- 承载服务实例级配置、中间件、序列化和错误映射

它的职责不包括：

- 新增第二套业务状态
- 直接拼底层 repository
- 承担 desktop 本地能力执行

服务端与客户端的边界统一为一句话：

**服务端拥有共享状态，desktop 拥有本地能力。**

### 3. 用户端协议采用 `REST + SSE`

`v1` 不采用 `WebSocket` 作为默认用户端协议，原因是现有前端模型本身就是“命令请求 + 单向事件订阅”：

- 请求型操作走命令调用
- 实时更新通过事件推送增量收口

因此首版协议定为：

- `REST`：providers、agent templates、MCP、sessions、threads、messages、snapshot、monitor
- `SSE`：thread events、job runtime events、thread monitor updates

建议的接口形态：

- `GET /api/v1/providers`
- `POST /api/v1/providers`
- `GET /api/v1/agents/templates`
- `GET /api/v1/sessions`
- `POST /api/v1/sessions`
- `GET /api/v1/sessions/:session_id/threads`
- `POST /api/v1/sessions/:session_id/threads/:thread_id/messages`
- `GET /api/v1/sessions/:session_id/threads/:thread_id/snapshot`
- `GET /api/v1/monitor/thread-pool`
- `GET /api/v1/monitor/job-runtime`
- `GET /api/v1/events`

`/api/v1/events` 返回统一 envelope，至少包含：

- `channel`
- `session_id?`
- `thread_id?`
- `payload`

后续如果 desktop 本地能力节点需要服务端主动下发任务，再单独评估 `WebSocket` 作为节点控制面协议；该需求不并入 `v1` 用户端协议。

### 4. 前端改成“共享核心 + 双 transport”

推荐路线不是复制一份 desktop UI，而是抽离共享前端核心。

新增目录：

- `packages/app-core`
- `apps/web`

其中：

- `packages/app-core` 放共享 types、DTO、transport 抽象、feature store、runtime、hooks、共享页面组件
- `apps/web` 只放 web 壳、入口、路由装配和 web 专属初始化
- `crates/desktop` 继续作为 Tauri 壳，但逐步消费 `packages/app-core`

共享前端核心先拆成三块：

1. `protocol`
2. `transport`
3. `features`

`transport` 层定义统一 `AppTransport`，至少提供：

- auth/bootstrap 能力
- provider / template / MCP 管理
- session / thread / message / snapshot API
- thread monitor API
- `subscribeThreadEvents()`
- `subscribeMonitorEvents()`

然后分别实现：

- `TauriTransport`
- `HttpSseTransport`

这样 desktop 初期仍可保持原有能力路径，而 web 可以直接接入 `argus-server`。

### 5. Web 首版范围

`v1 web` 只覆盖三条主链路：

- chat
- settings
- thread monitor

其中 `settings` 的定位要明确成：

**服务实例管理页，而不是用户个人设置页。**

首版至少包含：

- Provider 管理
- Agent Template 管理
- MCP Server 管理
- 服务运行状态和基础观测

不在 `v1` 范围内的内容：

- desktop 特有窗口能力
- 本地 filesystem / shell / browser 工具直接搬到 web
- 多用户空间、租户、组织、权限模型

### 6. 单实例、无登录

`v1` 服务形态明确为：

- 一台 `argus-server` 对应一个操作者环境
- 不做业务登录
- 不做多用户数据隔离
- provider、template、MCP、session、thread 都按实例级资源处理

这意味着当前 desktop 的本地账号体系不是 web/server 首版的前提条件。部署侧如需保护服务，可依赖内网隔离、反向代理或外层访问控制，但不把登录系统纳入本轮设计。

### 7. 混合工具执行

tool 执行模型采用混合模式：

- 共享且适合中心化的能力可由服务端执行
- 强本地、强权限、强机器绑定的能力继续保留在 desktop

因此 `v1` 不要求 web 直接调用本地 tool。desktop 后续可以演进成“桌面壳 + 本地能力节点”，但该节点协议不并入当前首版。

### 8. 错误恢复与状态一致性

网络版前端继续沿用现有 desktop 的收口原则：

- `SSE` 提供增量事件
- `snapshot` 才是最终事实来源
- 前端保留 `pendingUserMessage`、`pendingAssistant` 这类可见性补偿态
- 一旦断流、丢事件、解析失败，回退到 `get_thread_snapshot` 或 monitor snapshot 刷新

也就是说，事件通道只负责“尽快更新”，不负责“唯一真相”。

### 9. 仓库组织

推荐的目标目录结构：

- `crates/argus-server`
- `apps/web`
- `packages/app-core`
- `crates/desktop`

为支撑 `apps/web` 和 `packages/app-core`，根目录需要新增前端 workspace 管理文件，例如：

- `pnpm-workspace.yaml`

desktop 当前独立前端工程中的共用配置也需要逐步上提或共享，例如：

- TypeScript base config
- ESLint 共享配置
- Vite alias / path 约定

这些共享配置只在确有复用需要时再上提，不在第一步做大规模工具链改造。

## 迁移顺序

推荐按以下顺序渐进迁移：

1. 抽离前端 transport 接口，让现有 desktop store 不再直接依赖 Tauri
2. 抽离共享前端核心到 `packages/app-core`
3. 新增 `crates/argus-server`，把现有 `ArgusWing` facade 暴露成 `REST + SSE`
4. 新增 `apps/web`，复用共享核心实现 `chat + settings + thread monitor`
5. 最后再引入 desktop 连接远端 server 的模式，以及本地能力节点模型

这个顺序的目标是分散风险：

- 先验证共享前端核心是否能从 desktop 中成功抽离
- 再验证 `ArgusWing` 到 `axum` 的映射是否稳定
- 最后才处理 desktop/server 共存和本地能力接入

## 测试策略

测试按四层铺开：

1. Rust 服务端集成测试
   覆盖 `argus-server` 的 REST / SSE 映射、错误 envelope 和 snapshot 恢复路径。

2. Transport 合约测试
   同一组前端 contract 测试同时覆盖 `TauriTransport` 与 `HttpSseTransport`。

3. Store / runtime 回归测试
   优先锁住 `chat-store`、`chat-runtime`、thread monitor 收口行为，确保抽共享层后行为不漂移。

4. 冒烟测试
   保留一条 desktop 本地链路和一条 web 连 server 链路，验证 `chat + settings + thread monitor` 主路径。

## 非目标

本轮明确不做：

- desktop 下线或 Tauri bridge 全量删除
- 多用户 / 多租户 / 权限系统
- `WebSocket` 用户端协议
- 把所有本地 tool 立即上收服务端
- 先于需求引入 OpenAPI codegen 或大规模前端基建重构

## 风险与控制

主要风险有三类：

1. transport 抽离过程中，desktop 现有 chat 行为回归
2. server API 与现有 Tauri command 语义不一致，导致共享层变成双份逻辑
3. 过早引入本地能力节点和安全模型，扩大首版范围

对应控制策略：

- 先抽 transport，不先改页面
- server API 尽量对齐现有 facade 与 command 语义
- 用 feature-by-feature 迁移，不做一把切
- 本地能力节点后置，不与 web 首版绑定

## 结论

这次拆分的推荐路线是：

- 核心业务继续留在 Rust workspace 与 `ArgusWing`
- 新增 `axum` 服务端作为共享状态与网络 transport
- 新增共享前端核心，让 desktop 与 web 复用状态模型和页面能力
- `v1` 以 `REST + SSE`、单实例无登录、`chat + settings + thread monitor` 为边界

这样既能保留现有 desktop 的迭代速度，也能为 web 和后续服务化演进建立稳定边界。
