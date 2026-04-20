# Argus-Job

> 特性：后台任务与 runtime pool 解耦，`JobManager` 负责 job 派发/恢复/观测，`ThreadPool` 只负责 chat/runtime 生命周期。

## 核心职责

- `JobManager` 对外暴露 dispatch / lookup / cancel 等 job 生命周期接口
- `ThreadPool` 只负责 chat/runtime 注册、装载、驱逐、快照与通用 mailbox 投递
- `JobManager` 自己维护 job binding、parent/child 关系、job runtime state 与 result shadow
- 可选的 `ThreadPoolPersistence` 只承载 runtime/chat 所需的 repository；job 持久化由 `JobManager` 单独持有

## 关键模块

- `src/job_manager.rs`：job 跟踪、binding/recovery、job runtime state、结果消费、mailbox forwarder
- `src/thread_pool.rs`：loaded runtime、resident slot、chat snapshot、generic mailbox delivery
- `src/types.rs`：`JobExecutionRequest`
- `src/bin/smoke-chat.rs`：端到端 smoke chat 工具

## 公开入口

- `JobManager`
- `ThreadPool`
- `JobLookup`
- `JobExecutionRequest`

## 修改守则

- chat thread 是树根；job thread 必须作为父线程的直接子节点
- job 生命周期要同时对齐 `JobRuntimeState`、`JobRuntime*` 事件与持久化状态
- 不要让上层绕过 `JobManager` 或 `ThreadPool` 直接操纵 child runtime
