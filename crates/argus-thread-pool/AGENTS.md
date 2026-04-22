# Argus-Thread-Pool

> 特性：共享 thread runtime pool 核心，负责 runtime residency、装载、冷却/驱逐与 generic delivery。

## 作用域

- 本文件适用于 `crates/argus-thread-pool/` 及其子目录。

## 核心职责

- `ThreadPool` 管理 runtime slot、resident admission、attach/detach 与 lifecycle transition
- 通过 `load_runtime_with_builder` 接收 owner-provided builder，负责装载并附着已构建的 `Thread`
- 提供 sessionless 的 `RuntimeSummary` / `PoolState`、lifecycle observer 与 generic `deliver_thread_message`

## 关键模块

- `src/lib.rs`

## 公开入口

- `ThreadPool`
- `ThreadPoolConfig`
- `RuntimeSummary`
- `PoolState`
- `RuntimeLifecycleChange`
- `RuntimeIdleObserver`

## 修改守则

- 这里只承载 pool-core 语义；不要引入 Session / Job / provider / repository / template / tool 依赖
- runtime 构建和 owner-specific orchestration 留在上层 manager，通过 builder closure 注入
- 不在这里维护 `session_id` 兼容层、chat/job 专属 snapshot、或 owner-specific mailbox 语义
- 新增状态前先确认它不能从 runtime store 或 `Thread` 本身直接推导
