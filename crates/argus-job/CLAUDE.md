# Argus-Job

> 特性：后台任务与 job runtime supervisor，负责子线程派发、恢复和生命周期管理。

## 核心职责

- `JobManager` 对外暴露 dispatch / lookup / cancel 等 job 生命周期接口
- `JobManager` 组合 `argus-agent::ThreadRuntime`，对外暴露 job -> thread 绑定与 runtime 归口
- `JobRuntimeSupervisor` 保留 job dispatch、队列状态、resident slot 与 observer snapshot 等 job 侧职责
- 可选的 `JobRuntimePersistence` 把 job / thread 绑定持久化到 repository

## 关键模块

- `src/job_manager.rs`：job 跟踪、结果消费、mailbox forwarder
- `src/job_runtime_supervisor.rs`：loaded runtime、child thread 恢复、pool snapshot
- `src/types.rs`：`JobRuntimeRequest`
- `src/bin/smoke-chat.rs`：端到端 smoke chat 工具

## 公开入口

- `JobManager`
- `JobRuntimeSupervisor`
- `JobLookup`
- `JobRuntimeRequest`

## 修改守则

- chat thread 是树根；job thread 必须作为父线程的直接子节点
- job 生命周期要同时对齐 `JobRuntimePoolSnapshot`、`ThreadEvent` 与持久化状态
- runtime 注册与 parent/child thread 关系优先经由 `ThreadRuntime`，不要在 job 侧重建第二套 thread authority
- 不要让上层绕过 `JobManager` 直接操纵 child runtime；job runtime 细节保持在 `JobRuntimeSupervisor` 内部
