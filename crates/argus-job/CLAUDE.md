# Argus-Job

> 特性：后台任务与统一 thread pool，负责子线程派发、恢复和生命周期管理。

## 核心职责

- `JobManager` 对外暴露 dispatch / lookup / cancel 等 job 生命周期接口
- `ThreadPool` 负责 chat runtime 与 job runtime 的统一托管、恢复和快照
- 可选的 `ThreadPoolPersistence` 把 job / thread 绑定持久化到 repository

## 关键模块

- `src/job_manager.rs`：job 跟踪、结果消费、mailbox forwarder
- `src/thread_pool.rs`：loaded runtime、child thread 恢复、pool snapshot
- `src/types.rs`：`ThreadPoolJobRequest`
- `src/bin/smoke-chat.rs`：端到端 smoke chat 工具

## 公开入口

- `JobManager`
- `ThreadPool`
- `JobLookup`
- `ThreadPoolJobRequest`

## 修改守则

- chat thread 是树根；job thread 必须作为父线程的直接子节点
- job 生命周期要同时对齐 `ThreadPoolSnapshot`、`ThreadEvent` 与持久化状态
- 不要让上层绕过 `JobManager` 或 `ThreadPool` 直接操纵 child runtime
