# ThreadPool / Job 解耦设计

**日期：** 2026-04-18

## 目标

把 `ThreadPool` 收敛为 chat/runtime pool，把 job orchestration 和 job runtime 观测全部回收到 `JobManager`。

这次重构不再保留“统一 thread-pool 兼容视图”的旧假设，而是同时修正内部职责边界和外部观测边界：

- `ThreadPool` 只负责 runtime 注册、装载/驱逐、容量控制、订阅、chat/runtime 状态快照和通用消息投递
- `JobManager` 负责 job 绑定、job 恢复、job runtime 构建、job 执行、job 持久化、job result shadow、结果回投和 job runtime 状态/事件
- `SessionScheduler` 通过显式 job API 协调子 job，而不是直接把 `ThreadPool` 当作 job graph
- desktop / Tauri 改成 `ThreadPoolState + JobRuntimeState` 双源模型

## 当前问题

现在的 `ThreadPool` 同时背着三层职责：

1. runtime pool
2. job binding / child-thread graph
3. job execution / persistence / result routing

这带来几个结构性问题：

- `ThreadPool` 需要理解 `job_id`、parent/child job thread、job result consume 语义，边界过重
- `JobManager` 只是一个薄入口，但真正的 job 生命周期主要发生在 `ThreadPool`
- `SessionScheduler` 直接读取 `ThreadPool` 的 job-facing API，导致“pool 是 job coordinator”的事实继续向上传染
- `ThreadPoolJobRequest`、`claim_queued_job_result`、`recover_child_jobs` 这类名字把 job 语义伪装成 pool / thread graph 语义，造成理解成本

问题核心不是实现细节重复，而是 job orchestration 放错了所有者。

## 设计决策

### 1. `ThreadPool` 只保留 chat/runtime pool 语义

`ThreadPool` 保留以下职责：

- 注册 chat runtime
- 维护 runtime resident slot
- 装载 / 附着 runtime
- runtime running / cooling / evicted 状态迁移
- chat/runtime snapshot / metrics / subscriptions
- 通用 `deliver_mailbox_message`
- chat runtime 装载与 user message 投递

`ThreadPool` 不再拥有以下职责：

- job runtime 身份与观测
- `job_id -> thread_id` 绑定
- parent / child job thread 关系缓存
- job trace metadata 恢复入口
- job thread 持久化绑定
- job runtime 构建
- job 执行与 turn result 等待
- job 状态/result 持久化
- delivered-but-not-consumed 的 job result shadow
- 对外 `ThreadPoolState` 中的 `job_id` / job kind / job lifecycle 事件

### 2. `JobManager` 成为 job orchestration 与 job runtime 观测的唯一所有者

`JobManager` 新增私有 job runtime store，统一管理：

- `job_id -> execution_thread_id`
- `child_thread_id -> parent_thread_id`
- `parent_thread_id -> child jobs`
- delivered job result shadow
- tracked job state
- `job_runtimes: HashMap<ThreadId, JobRuntimeSummary>`

`dispatch_job` 的内部流程改为由 `JobManager` 主导：

1. 准备或恢复 job binding
2. 注册 job runtime 到 `ThreadPool`
3. 构建并装载 job runtime
4. 执行 task assignment
5. 等待 turn 结束并持久化结果
6. 把结果回投到 originating thread
7. 更新 job runtime state / lifecycle 事件

### 3. job 查询接口改成显式 job 语义

从 `ThreadPool` 暴露出去的 job-facing API 全部迁走，并改名为显式 job 语义接口：

- `recover_thread_binding` -> `recover_job_execution_thread_id`
- `recover_parent_thread_id` -> `recover_parent_job_thread_id`
- `recover_child_jobs` -> `recover_child_jobs_for_thread`
- `claim_queued_job_result` -> `claim_delivered_job_result`

这些接口挂在 `JobManager`，不再伪装成通用 thread graph / pool API。

### 4. 外部观测协议拆成 chat/runtime + job 两条线

这轮不改 `ThreadPool` 的公开名字，但会去掉它对 job 的对外承载。

协议收口策略是：

- `ThreadPoolState` / `ThreadPoolSnapshot` / `ThreadPoolRuntimeSummary` 收成 chat-only
- 新增 `JobRuntimeState` / `JobRuntimeSnapshot` / `JobRuntimeSummary`
- `ThreadPoolQueued/Started/Cooling/Evicted/MetricsUpdated` 只代表 chat/runtime
- job 运行时新增独立 `JobRuntimeQueued/Started/Cooling/Evicted/MetricsUpdated`
- desktop monitor 在 store 侧 merge 两条数据源，UI 可以继续保留本地 `kind: "chat" | "job"`

这样 `ThreadPool` 不再向外传播 job 身份，job monitor 也不用再借用 chat/runtime 池的协议壳。

## 实施顺序

1. 写设计文档和 implementation plan
2. 锁住现有 job 行为测试
3. 把 job store / helper / execution 流迁回 `JobManager`
4. 清掉 `ThreadPool` 中的 job graph、job persistence、job result shadow 和 job-facing 观测
5. 把 `SessionScheduler` 改成依赖 `JobManager` 的显式 job API
6. 拆协议：`ThreadPoolState` chat-only，新增 `JobRuntimeState`
7. 把 job-specific 测试迁移到 `job_manager::tests`
8. 保留命名 follow-up，但不在这一轮扩大为 rename pass

## 本轮命名收口

本轮立即收口的冗余名字：

- `ThreadPoolJobRequest` -> `JobExecutionRequest`
- `claim_queued_job_result` -> `claim_delivered_job_result`
- 其他显式 job API 全部去掉“伪通用线程拓扑”命名

## 后续命名计划

职责拆分完成后，再单独做 naming pass：

- 审查 `thread_pool_*` façade 中哪些已经不是 pool 原生概念
- 评估把内部纯 runtime state 进一步显式命名
- 单独决定是否把 `ThreadPool` 重命名为 `RuntimePool`

这个后续 pass 不并入当前重构，以控制 diff 和风险。
