# Job ThreadPool Design

**Date:** 2026-03-29

## 背景

当前 `argus-job` 的执行模型本质上是 `dispatch_job -> JobManager -> 临时 TurnBuilder.execute()`。
虽然会为 job 生成一个 `job-*` 形式的 thread 标识，但它并不代表系统中的真实 thread 执行实体，也不参与统一的线程级资源治理。

这带来三个问题：

1. job 的真实执行单位仍是一次性 turn，thread 只是兼容字段，不是系统核心模型。
2. 活跃执行资源没有统一协调器，`JobManager` 既负责业务语义，又隐式持有执行权。
3. 系统无法可靠暴露 thread 级资源监控，尤其是内存使用，只能看到零散的 job 状态和 token 统计。

本设计将 job 的执行单位升级为真正的 thread，并引入统一的 `ThreadPool` 负责活跃 thread 的装载、执行、冷却回收和监控。

## 目标

1. job 的执行单位从一次性 turn 变成持久化 thread。
2. 引入统一的 `ThreadPool` 作为活跃 thread 的唯一调度器。
3. `dispatch_job` 不再负责生成 `thread_id`，而是由 `ThreadPool` 负责创建或恢复并绑定 job-thread。
4. 支持冷 thread 自动回收内存，但保留可恢复的持久化状态。
5. 对外暴露可查询、可订阅的 `ThreadPool` 用量信息，重点包括内存指标。
6. 在 desktop 前端增加独立的 Thread 监控页签。

## 非目标

1. 本次不把所有普通聊天 thread 的执行路径全部重构到 `ThreadPool`。
2. 第一版不提供精确到字节级的 thread 内存 profile，只提供可比较、可排序、可告警的估算指标。
3. 第一版不在前端提供手动回收、强制重载等运维操作。

## 选型结论

采用方案 B：

- `JobManager` 负责 job 语义和生命周期。
- `ThreadPool` 负责 thread 生命周期和执行资源。
- job 会绑定一个真实、持久化的 thread。
- job 完成后对应 thread 保留一段 cooling 时间，再由 `ThreadPool` 自动从内存中驱逐。

没有采用方案 A，因为它会把 thread 继续降级为一次性实现细节。
没有直接采用方案 C，因为那会把这次需求和更大的全局 runtime 重构绑在一起，风险过高。

## 总体架构

### 组件职责

#### `dispatch_job` tool

- 只负责接收 `prompt`、`agent_id`、`context`
- 调用 `JobManager` 发起 job
- 不生成 `thread_id`
- 不直接驱动执行

#### `JobManager`

- 创建 job 记录
- 维护 job 状态流转
- 将执行意图提交给 `ThreadPool`
- 查询 job 与 thread 的绑定关系
- 汇总 job 结果并继续向现有调用方暴露 job 语义

#### `ThreadPool`

- 生成或恢复 job 对应的执行 thread
- 将 job 和 thread 绑定
- 维护活跃 thread 注册表
- 控制排队与最大并发
- 装载、执行、冷却和驱逐 thread runtime
- 采集并汇总 pool 级和 thread 级监控数据

#### `argus-agent::Thread`

- 继续作为单个 thread 的运行时实体
- 不再由 `JobManager` 临时构造并直接执行
- 改由 `ThreadPool` 统一持有、恢复和调度

#### Repository

- `jobs` 表继续记录 job 业务状态
- `threads` 表成为 job 执行的真实载体
- job 与 thread 的绑定关系需要有清晰的持久化语义

#### `ArgusWing`

- 注入 `ThreadPool`
- 对外暴露监控查询和订阅接口
- 把 `ThreadPool` 事件桥接给 desktop

## 生命周期设计

### job 状态

`pending -> queued -> running -> succeeded | failed | cancelled`

### thread pool runtime 状态

`inactive -> loading -> queued -> running -> cooling -> evicted`

说明：

- `inactive` 表示仅存在持久化状态，没有内存 runtime。
- `cooling` 表示最近刚跑完，暂时保留在内存中等待潜在后续请求。
- `evicted` 表示内存 runtime 已被回收，但持久化 thread 仍存在，可再次恢复。

## 执行流

1. `dispatch_job` 接收请求并调用 `JobManager`。
2. `JobManager` 创建 job 记录，初始状态为 `pending`。
3. `JobManager` 调用 `ThreadPool.enqueue_job(job_id, agent_id, prompt, context)`。
4. `ThreadPool` 决定为该 job 创建新的执行 thread，或恢复已有 thread。
5. `ThreadPool` 生成或确认 `thread_id`，并将 job 与 thread 完成绑定。
6. `ThreadPool` 将 job 状态推进为 `queued`，等待执行槽位。
7. `ThreadPool` 装载 `argus-agent::Thread` runtime，并把执行请求投递到该 thread。
8. 执行期间，thread 继续沿用现有 `ThreadEvent`、`ThreadControlEvent` 和 mailbox 机制。
9. 执行完成后，`ThreadPool` 更新 job 结果并广播 job 事件。
10. 对应 thread 转入 `cooling`。
11. cooling 超时后，若没有新的执行请求，则驱逐 runtime，仅保留持久化 thread 与监控摘要。

## 绑定关系设计

### 核心约束

1. `dispatch_job` 不拥有 `thread_id` 创建权。
2. `ThreadPool` 是唯一拥有 thread 生命周期的组件，因此也拥有 thread 绑定权。
3. `JobManager` 只知道 job 语义，不直接拥有 thread runtime。

### 数据语义

当前 repository 中的 `jobs.thread_id` 字段含义偏模糊，需要统一为“job 关联的执行 thread 标识”。

若后续需要支持一个 job 多次恢复同一 thread，则该字段可以继续使用。
若未来需要更丰富的执行关系，可再演化为更明确的 `execution_thread_id` 命名，但本次优先以最小 schema 演进落地。

## 监控设计

### Pool 级快照

`ThreadPoolSnapshot` 应至少包括：

- `max_threads`
- `active_threads`
- `queued_jobs`
- `running_threads`
- `cooling_threads`
- `evicted_threads`
- `estimated_memory_bytes`
- `peak_estimated_memory_bytes`
- `process_memory_bytes`
- `peak_process_memory_bytes`
- `resident_thread_count`
- `avg_thread_memory_bytes`
- `captured_at`

### Thread 级快照

`ThreadRuntimeSnapshot` 应至少包括：

- `thread_id`
- `job_id`
- `agent_id`
- `agent_display_name`
- `status`
- `queued_at`
- `started_at`
- `last_active_at`
- `estimated_memory_bytes`
- `token_usage`
- `recoverable`
- `failure_reason`

### 内存指标策略

第一版采用“双轨指标”：

1. `estimated_memory_bytes`
   基于 thread 运行时对象的可归因估算值，反映线程池大约占用了多少内存。
2. `process_memory_bytes`
   基于系统或进程级采样得到的真实内存观测，反映整个 Argus 进程实际占用了多少内存。

这样设计的原因是 Rust 进程中的真实内存难以精确拆分到单个 thread runtime。
因此 thread 维度更适合做估算和排序，进程维度更适合做总量观测与告警。

### thread 内存估算来源

第一版估算可由以下数据组合：

- thread 消息历史总长度
- mailbox / inbox 当前积压大小
- 最近 turn 输出内容大小
- 计划存储与临时缓存大小
- trace writer 等常驻对象是否启用

估算目标不是绝对精确，而是满足以下能力：

- 找出最重的 thread
- 观察 thread cooling/eviction 前后的变化
- 支持阈值告警和前端排序

## 事件模型

保留现有 job 结果事件语义，同时为 `ThreadPool` 新增专用观测事件。

建议新增：

- `ThreadBoundToJob { job_id, thread_id }`
- `ThreadPoolQueued { job_id, thread_id }`
- `ThreadPoolStarted { job_id, thread_id }`
- `ThreadPoolCooling { job_id, thread_id }`
- `ThreadPoolEvicted { job_id, thread_id, reason }`
- `ThreadPoolMetricsUpdated { snapshot }`

设计原则：

- job 事件回答“任务结果怎么样”
- pool 事件回答“执行资源现在怎么样”

不要把这两类语义混成单一状态流，否则前端和调用方都会变得难以理解。

## 对外 API

`ThreadPool` 建议提供以下核心接口：

- `enqueue_job(job_id, agent_id, prompt, context) -> EnqueueResult`
- `get_thread_binding(job_id) -> Option<ThreadId>`
- `cancel_job(job_id) -> Result<()>`
- `collect_metrics() -> ThreadPoolSnapshot`

`ArgusWing` 建议对外补充：

- 查询当前 `ThreadPoolSnapshot`
- 列出活跃或最近 cooling 的 thread 快照
- 订阅 `ThreadPool` 生命周期事件

## 错误处理

遵循“执行链路失败显式化，资源链路失败降级化”的原则。

### 显式失败

- job 创建失败：直接返回错误
- thread 创建或绑定失败：job 标记为 `failed`
- thread 启动失败：job 标记为 `failed`
- thread 运行失败：job 标记为 `failed`，thread 转入 `cooling`

### 降级处理

- 监控采样失败：不阻塞执行，仅在快照中标记指标缺失并记录日志
- 回收失败：不影响 job 结果，暂时保留 runtime 并上报告警
- 前端快照拉取失败：不影响聊天功能，只显示监控降级状态

## Desktop 前端设计

新增独立的 `Thread Monitor` 页签，不与普通会话列表混用。

### 页面结构

#### 顶部总览卡片

- 活跃 thread 数
- running / queued / cooling / evicted
- 估算内存
- 进程内存
- 峰值内存

#### 中间 thread 列表

每行展示：

- `thread_id`
- `job_id`
- agent 信息
- 当前状态
- 最近活动时间
- 估算内存
- token 使用
- 是否可恢复

#### 底部详情区

- 最近状态变更事件
- 失败原因
- 恢复可能性

### 第一版交互范围

- 支持按 `job_id`、`thread_id`、`agent`、`status` 过滤
- 默认展示 `active + cooling`
- 仅做只读监控
- 暂不提供手动回收和重载操作

## 测试策略

### 单元测试

- `ThreadPool` 状态机
- job-thread 绑定逻辑
- 并发与排队限制
- cooling / eviction 逻辑
- 内存估算函数

### 集成测试

- `dispatch_job -> JobManager -> ThreadPool -> Thread -> JobResult`
- evicted thread 再次恢复执行
- 事件广播顺序与语义

### 仓储测试

- job 与 thread 绑定持久化
- 恢复路径下的状态一致性
- job 状态更新与 thread 绑定更新的原子性边界

### 前端测试

- Thread Monitor 页签渲染
- 快照列表过滤
- pool 事件推送后 UI 刷新
- 快照获取失败时的降级显示

## 分阶段落地建议

### 阶段 1

- 引入 `ThreadPool`
- 打通 job-thread 绑定
- 让 `JobManager` 改为通过 `ThreadPool` 执行

### 阶段 2

- 新增 `ThreadPoolSnapshot`
- 实现内存估算与池级监控
- 暴露查询与事件订阅接口

### 阶段 3

- desktop 增加 Thread Monitor 页签
- 展示 pool 概览与 thread 明细

### 阶段 4

- 补全 cooling / eviction 策略
- 补充恢复测试与异常链路测试
- 调整阈值与监控展示

## 风险与取舍

1. 这次改造横跨 `argus-job`、`argus-agent`、`argus-session`、`argus-wing`、protocol、desktop，属于中等偏大的架构变更。
2. thread 内存统计第一版只能做到估算值，不应对外宣传为精确 profile。
3. 若 job 与 thread 绑定更新缺少清晰边界，恢复与回收流程会出现状态撕裂，因此持久化接口必须显式建模。
4. 若过早把所有普通聊天 thread 一并接管到 `ThreadPool`，会放大改造风险，因此本次仅覆盖 job-thread 路径。

## 最终结论

本次需求的最佳落地方式是让 `ThreadPool` 成为 job-thread 的唯一执行协调器：

- `dispatch_job` 只负责入口
- `JobManager` 只负责 job 语义
- `ThreadPool` 负责真实 thread 的创建、绑定、执行、监控和回收

这样既能把 job 的执行单位真正提升为 thread，又能为系统增加统一、可信、可扩展的 thread 资源监控能力，并为 desktop 提供清晰的 Thread 监控页签。
