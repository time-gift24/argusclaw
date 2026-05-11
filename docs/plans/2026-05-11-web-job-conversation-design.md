# Web Job Conversation Design

日期：2026-05-11

## 背景

Web `/chat` 已支持主对话、运行活动和后台 Job 事件展示，但 Job 目前只作为右侧 transient activity 出现。用户希望在主对话中看到已经派发过的 subagent/job，点击后进入一个子页面查看该 Job 的对话信息。该子页面应像普通对话一样展示 transcript，但必须保持只读，不允许继续对话。

## 目标

- 在 Web 主对话页展示当前父线程派发过的 subagent/job，刷新后仍可查询。
- 点击 Job 进入独立路由 `/chat/jobs/:jobId`。
- Job 子页面左上角展示面包屑和返回父对话入口。
- Job 子页面复用普通对话的消息展示体验，但不渲染 composer，不允许发送、取消或切换模型。
- 后端用产品语义暴露 Job conversation API，不让前端推导 scheduler tool 协议或 job runtime 内部 session 约定。

## 非目标

- 不在 Web UI 暴露 `dispatch_job`、`get_job_result`、`send_message` 等完整 scheduler 操作台。
- 不允许用户从 Job 子页面继续向 Job thread 发送消息。
- 不重塑普通 `/chat` 为通用 thread 路由。
- 不引入 desktop store 或 shared frontend core。

## 推荐方案

采用“父线程 Job 列表 API + 只读 Job 对话页”。

后端新增父线程派发记录查询和 Job conversation 查询接口。前端在 `/chat` 中展示历史可查询的“已派发 subagent”列表，并在 `/chat/jobs/:jobId` 中展示只读 Job transcript。实时运行活动仍由现有 `RuntimeActivityRail` 负责，历史派发记录由新的 Job 列表负责，二者语义分离。

## 后端 API

### 列出父线程派发 Job

`GET /api/v1/chat/sessions/:sessionId/threads/:threadId/jobs`

返回当前父线程派发过的 subagent/job：

```json
{
  "data": [
    {
      "job_id": "job-123",
      "title": "job:job-123",
      "subagent_name": "Researcher",
      "status": "running",
      "created_at": "2026-05-11T10:00:00Z",
      "updated_at": "2026-05-11T10:01:00Z",
      "result_preview": null,
      "bound_thread_id": "thread-456"
    }
  ]
}
```

`bound_thread_id` 只用于前端判断是否可进入，不作为发送消息的凭据。状态和结果摘要来自 repository/job manager 的事实来源。该接口应遵守当前 chat 用户权限，非当前用户不可读取他人的 parent thread 派发记录。

### 获取 Job 对话

`GET /api/v1/chat/jobs/:jobId`

服务端通过 `ArgusWing::job_thread_binding(jobId)` 解析 execution thread，并封装 job runtime 的内部 session 约定。响应包含只读上下文、消息和 snapshot 摘要：

```json
{
  "job_id": "job-123",
  "title": "job:job-123",
  "status": "completed",
  "thread_id": "thread-456",
  "session_id": "thread-456",
  "parent_session_id": "session-abc",
  "parent_thread_id": "thread-parent",
  "messages": [],
  "turn_count": 2,
  "token_count": 1200,
  "plan_item_count": 0
}
```

前端不需要调用普通 `/chat/sessions/:sessionId/threads/:threadId/messages` 来读取 Job thread。这样可以避免把 `SessionId(threadId)` 这类内部细节扩散到 UI。

### Job 对话事件流

`GET /api/v1/chat/jobs/:jobId/events`

如果 Job 仍在 `queued` 或 `running`，前端订阅该只读事件流。事件 payload 复用现有 `ChatThreadEventEnvelope` 语义，但 URL 和权限模型使用 Job conversation 语义。SSE 失败时前端降级轮询 `GET /api/v1/chat/jobs/:jobId`。

不新增任何 Job route 的 `POST /messages`，也不把普通 chat send API 暴露给 Job 子页面。

## Web 组件

### DispatchedJobsPanel

放在 `/chat` 的运行活动区域附近，显示“已派发 subagent”。数据来自父线程 Job 列表 API，而不是只从 SSE 内存态累积。每行展示：

- subagent/job 名称
- 状态
- 更新时间
- 结果摘要

点击可进入 `/chat/jobs/:jobId?fromSession=:sessionId&fromThread=:threadId`。query 仅用于返回体验，不作为权限或事实来源。

### JobChatPage

新增只读页面，路由为 `/chat/jobs/:jobId`。页面保持沉浸式消息阅读体验，顶部加入轻量导航：

- 面包屑：`对话 / Job job-123`
- 左上角“返回父对话”按钮

返回优先使用 API 返回的 `parent_session_id` 和 `parent_thread_id`；无法恢复父线程时退回 `/chat`。

### Conversation Reuse

Job 页复用 `ChatConversationPanel`、`toRobotMessages`、markdown/tool rendering 和流式 pending assistant 展示逻辑。它不渲染 `ChatComposerBar`，也不调用 `sendChatMessage`、`cancelChatThread`、`updateChatThreadModel`。

### RuntimeActivityRail

继续负责当前 turn 的实时运行活动。Job activity 行可跳转到 Job 对话页；普通工具调用仍打开详情弹窗。历史 Job 查询由 `DispatchedJobsPanel` 承担，避免把 transient runtime activity 和持久派发记录混为一体。

## 数据流

1. `/chat` 加载或切换 active thread 后，调用 `listChatThreadJobs(sessionId, threadId)`。
2. SSE 收到 `job_dispatched`、`job_result` 或 `job_runtime_*` 事件时，静默刷新 Job 列表。
3. 用户点击 Job 行，跳转 `/chat/jobs/:jobId?fromSession=...&fromThread=...`。
4. `JobChatPage` 调用 `getChatJobConversation(jobId)` 获取只读上下文和 messages。
5. 如果 Job 未完成，订阅 `subscribeChatJobConversation(jobId)`；失败后降级轮询。
6. Job 完成或失败后停止实时订阅，保留最终 transcript。

## 错误处理

- 父线程 Job 列表加载失败：不影响主对话，面板显示“派发记录加载失败，可刷新重试”。
- 父线程没有派发记录：显示“暂无派发的 subagent”。
- Job 尚未绑定 thread：Job 页显示“Job 已创建，执行线程尚未就绪”，保留返回按钮，并短轮询几次。
- Job 不存在或无权限：显示 404/403 风格错误状态，提供返回 `/chat`。
- Job 消息加载失败：保留面包屑和返回按钮，消息区显示错误。
- SSE 中断：切换轮询并在顶部显示轻量提示，不弹全局错误。
- 父对话被删除后打开 Job 页：仍允许通过 `jobId` 查看只读 Job conversation；返回按钮退回 `/chat`。

## 测试范围

### Server

- `GET /api/v1/chat/jobs/:jobId` 在 job 绑定存在时返回只读 conversation。
- job 未绑定时返回 pending 状态，不暴露发送入口。
- 父 thread jobs API 能列出已派发 jobs，并包含状态与结果摘要。
- 非当前用户不可读他人的 parent thread/job conversation。
- 路由层不存在 Job message POST 入口。

### Web

- `/chat` 加载 active thread 后渲染“已派发 subagent”列表。
- SSE 收到 job 事件后刷新列表。
- 点击 Job 跳转 `/chat/jobs/:jobId`。
- Job 页展示面包屑和返回父对话按钮。
- Job 页展示消息，但不渲染 composer，不触发 send/cancel。
- Job 页 SSE 或轮询更新消息。
- 错误、未绑定和空列表状态都有中文反馈。

## 风险

- 当前后端可能已有 `job_id -> thread_id` 绑定，但缺少“父线程派发过哪些 jobs”的持久查询 API；实现时应优先在 repository/job manager 边界补产品语义查询，不让 Web 直接解析 trace 或 tool payload。
- Job runtime session ID 是内部约定，必须由 server API 封装，避免前端依赖。
- 只读边界要用路由和组件双重约束验证，不能只靠隐藏 composer。
