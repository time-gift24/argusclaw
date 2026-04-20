# Argus-Thread-Pool

> 特性：统一线程池运行时边界的占位 crate，预留 chat/runtime 生命周期与路由能力的独立归属。

## 核心职责

- 为后续从 `argus-job` 抽离的 thread pool 逻辑提供独立 crate 边界
- 先稳定 crate 名称、文档约束和最小公开入口，再逐步迁移运行时代码
- 保持与并行中的线程路由重构兼容，不在这里提前复制现有实现

## 公开入口

- `ThreadPool`
- `ThreadPoolConfig`
- `scaffold_status()`

## 修改守则

- 抽离前不要复制 `argus-job` 的 thread pool 实现，避免形成双份运行时逻辑
- 新增公开 API 时优先服务迁移边界，不要在骨架阶段引入额外行为承诺
- 后续迁移应保持对现有上游 crate 的最小扰动，避免无关的依赖扩散
