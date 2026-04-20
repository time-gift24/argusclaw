# Argus-Job

> 特性：后台任务与 runtime pool 解耦，`JobManager` 负责 job 派发、恢复与观测，复用共享 `argus-thread-pool` 管理运行时。

## 作用域

- 本文件适用于 `crates/argus-job/` 及其子目录。

## 核心职责

- `JobManager` 对外暴露 dispatch / lookup / cancel 等 job 生命周期接口
- 维护 job binding、parent/child 关系、job runtime state 与结果 shadow
- 通过 owner-local builder / recovery helpers 组装 job thread，并把 runtime admission、delivery、cooling 交给共享 `ThreadPool`

## 关键模块

- `src/job_manager.rs`：job 跟踪、binding/recovery、job runtime state、结果消费、mailbox forwarder
- `src/types.rs`：`JobExecutionRequest`、`RecoveredChildJob`
- `src/bin/smoke-chat.rs`：端到端 smoke chat 工具

## 公开入口

- `JobManager`
- `JobLookup`
- `JobExecutionRequest`
- `RecoveredChildJob`

## 修改守则

- chat thread 是树根；job thread 必须作为父线程的直接子节点
- job 生命周期要同时对齐 `JobRuntimeState`、`JobRuntime*` 事件与持久化状态
- 不要在此重新实现 pool-core 或让 `JobManager` 重新拥有 shared `ThreadPool` 的构造权
- 不要让上层绕过 `JobManager` 直接操纵 job child runtime
