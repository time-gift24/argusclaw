# 子代理任务详情抽屉设计

## 背景

当前 desktop 聊天页已经能展示子代理任务卡片，但展示层只有摘要信息：

- `jobStatuses` 只保存任务状态、prompt、摘要 message 和 agent 基本信息
- 聊天页任务卡片只能看到简短结果，无法查看完整任务详情
- Tauri 已透传 `mailbox_message_queued` 事件，但 desktop store 尚未消费这条事件

这导致“子代理详细信息前端不可见”的问题并不是单点样式缺失，而是前端状态模型只接了摘要面，没有接住正文面。

## 目标

- 保留当前聊天页中的子代理任务卡片摘要视图
- 点击任务卡片后打开右侧详情抽屉
- 详情抽屉优先展示最终产出，再展示执行过程
- 不把子代理结果混入主聊天 transcript
- 在不改动现有主聊天结构的前提下，为后续更丰富的结构化任务结果预留扩展位

## 非目标

- 不新增独立“收件箱”页面
- 不把原始 JSON 暴露为首屏信息
- 不改变现有任务卡片的摘要定位
- 不要求本次先改动 Rust 后端事件模型

## 方案对比

### 方案 A：仅放大现有任务卡片内容

只在前端增加一个详情弹层，复用 `jobStatuses` 当前已有字段。

优点：

- 改动最小
- 可快速落地

缺点：

- `job_result.message` 当前本质仍是摘要字符串
- 详情弹层只能“放大摘要”，不能真正补足细节

### 方案 B：新增任务详情状态并消费 mailbox 事件

前端保留 `jobStatuses` 作为摘要态，新增 `jobDetails` 作为详情态，并消费 `mailbox_message_queued` 以补齐完整结果正文。

优点：

- 改动集中在 desktop
- 直接复用现有事件流
- 能明显改善“看不到详细信息”的核心问题
- 为后续结构化 payload 扩展预留空间

缺点：

- 需要新增一层状态模型和详情 UI
- 结果丰富度仍受后端当前 message 质量影响

### 方案 C：前后端一起扩展结构化任务结果

在 Rust 协议层和 Tauri payload 中增加更完整的任务详情对象，再由前端抽屉渲染。

优点：

- 信息最完整
- 长期维护成本更低

缺点：

- 影响面更大
- 会拖慢当前问题的修复节奏

## 推荐方案

采用方案 B，并为方案 C 预留扩展位。

原因：

- 当前问题首先是 desktop 没有接住已有的正文事件
- 先补齐前端状态模型与详情抽屉，能最快解决可见性问题
- 后续若后端需要升级成结构化 payload，只需扩展 `jobDetails` 字段与渲染，不必重做交互

## 信息架构

### 列表层

聊天页继续保留现有“子 agent 任务”卡片列表，负责展示：

- 子代理名称
- 子代理简介
- 当前状态
- prompt 摘要
- 结果摘要

### 详情层

点击任务卡片打开右侧详情抽屉，负责展示完整任务信息。

抽屉分四块：

1. 最终产出
2. 任务信息
3. 执行过程
4. 调试扩展位

## 交互设计

详情采用右侧抽屉而不是居中弹窗或内联展开。

原因：

- 子代理结果经常需要和左侧聊天上下文对照
- 不会挤压主聊天 transcript
- 更适合持续刷新的异步任务状态

交互规则：

- 点击任务卡片打开抽屉
- 抽屉标题显示 agent 名称、任务状态、job id
- 抽屉打开期间若任务状态变化，详情实时刷新
- 抽屉关闭不影响当前聊天会话和滚动位置

## 数据模型

### 保留摘要态

继续保留 `jobStatuses`，服务任务列表。

### 新增详情态

新增 `jobDetails`，建议字段如下：

- `job_id`
- `agent_id`
- `agent_display_name`
- `agent_description`
- `prompt`
- `status`
- `summary_text`
- `result_text`
- `started_at`
- `finished_at`
- `input_tokens`
- `output_tokens`
- `timeline`
- `source_message_id`
- `thread_id`
- `raw_payload` 预留字段

## 事件流设计

### `job_dispatched`

作用：

- 初始化 `jobStatuses[job_id]`
- 初始化 `jobDetails[job_id]`
- 记录 `prompt`、`agent_id`、`started_at`
- 时间线追加“已派发”

### `thread_pool_queued` / `thread_pool_started` / `thread_pool_cooling` / `thread_pool_evicted`

作用：

- 如果事件带 `runtime.job_id`，则更新对应 `jobDetails`
- 仅补执行过程与时间线，不改变详情正文来源

### `job_result`

作用：

- 更新任务完成或失败状态
- 写入 `summary_text`
- 写入 token 信息
- 记录 `finished_at`

说明：

- 这里的 `message` 仍视作摘要来源
- 详情抽屉会在没有正文时回退展示它

### `mailbox_message_queued`

作用：

- 识别 `message_type = job_result` 的回信
- 从 `message.text` 中提取子代理最终产出正文
- 写入 `jobDetails[job_id].result_text`
- 补充消息 id 与时间戳

这条事件是本次补齐“详细信息”的关键。

## 详情抽屉内容

### 最终产出

优先级：

1. `mailbox_message_queued.message.text`
2. `job_result.message`
3. 空态提示

要求：

- 支持长文本滚动
- 保留换行
- 支持基础 Markdown/代码块渲染的演进空间

### 任务信息

展示：

- agent 名称
- agent 描述
- 原始 prompt
- job id
- 开始时间
- 完成时间
- success / failed 状态
- token 使用量

### 执行过程

采用轻量时间线，显示：

- 已派发
- 排队中
- 运行中
- 冷却中
- 已完成 / 失败 / 已驱逐

每个节点附时间戳；失败和驱逐场景显示原因摘要。

### 调试扩展位

本次不把原始 JSON 作为首屏信息，但状态模型预留 `raw_payload` 扩展位，方便后续在需要时打开。

## 异常与降级策略

- 有正文时显示正文
- 无正文但有摘要时显示摘要，并标记“结果摘要”
- 两者都无时显示“任务已结束，但详细结果暂不可用”
- `job_result` 先到、`mailbox_message_queued` 后到时，允许正文覆盖摘要展示源
- 孤立或不匹配的 mailbox 事件直接忽略，不让 UI 报错
- 卡片继续执行长度裁剪；抽屉只做安全上限保护，不做摘要裁剪

## 测试策略

### Store 测试

- `job_dispatched` 初始化 `jobDetails`
- `job_result` 写入状态、摘要、token、完成时间
- `mailbox_message_queued(job_result)` 写入 `result_text`
- 正文晚到时可以覆盖摘要展示源
- `thread_pool_*` 会正确追加时间线
- 无效 mailbox 事件被安全忽略

### 组件测试

- 点击任务卡片打开右侧抽屉
- 抽屉优先显示正文，其次显示摘要
- 失败态、停止态、无正文态文案正确
- 抽屉在任务状态更新后可以实时刷新

### 回归测试

- 现有任务卡片列表行为不变
- 主聊天 transcript 不新增额外结果消息
- 现有 `jobStatuses` 测试继续成立

## 实施边界

优先改动 desktop：

- `crates/desktop/lib/types/chat.ts`
- `crates/desktop/lib/chat-store.ts`
- `crates/desktop/components/assistant-ui/thread.tsx`
- 新增一个任务详情抽屉组件
- desktop 测试文件

Rust 后端暂不改协议，只消费已经存在的事件与字段。
