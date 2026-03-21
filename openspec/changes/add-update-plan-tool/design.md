## Context

Argus-claw 的工具系统（`argus-tool`）向 LLM 提供文件系统、Shell 和网络工具。Thread 执行（`argus-thread`）协调 turn 和工具调用。目前没有机制让 LLM 在一个 turn 内的多个工具调用之间跟踪自己的任务进度。

目标是添加一个最小化的 `update_plan` 工具，赋予 LLM 结构化的 TODO/checklist 能力，作用域限定在单个 Thread 内。

## Goals / Non-Goals

**Goals：**
- LLM 可以通过 `update_plan` 工具更新每个 Thread 的计划
- Plan 状态在 turn 内的工具调用之间持久化
- 外部消费者（UI、日志）可以随时从 Thread 读取当前计划
- 最小化实现：仅内存，无新增依赖，无新增 crate

**Non-Goals：**
- Plan 在 Thread 重启后持久化（文件存储）
- Plan 在 turn 失败后持久化
- 跨 Thread 共享 plan
- 工具发现或动态工具注册
- OpenSpec 集成

## Decisions

### 1. Plan 类型放在 `argus-protocol`（无新增依赖）

**Decision**：在 `crates/argus-protocol/src/plan.rs` 中定义 `StepStatus`、`PlanItemArg`、`UpdatePlanArgs`。

**Rationale**：这些类型在工具实现（argus-thread）和外部消费者（UI 从 Thread 读取 plan）之间共享。放在 argus-protocol 可避免重复。不使用 `schemars` 或 `ts_rs` — 仅用 `serde` + 基础 derives。argus-protocol 是叶子模块，不得增加内部 argus-* 依赖。

### 2. 内存 plan store，无 trait 抽象

**Decision**：Thread 持有 `plan: Arc<RwLock<Vec<PlanItem>>>`，`UpdatePlanTool` 获得 clone。无 `PlanStore` trait。

**Rationale**：纯值语义 — `Arc<RwLock<Vec>>` 是简单的零开销句柄。当前需求是单 turn 内存方案。Trait 会增加间接层，当前阶段没有收益。未来扩展（文件、SQLite）可届时添加 `trait PlanStore`。

```
Thread
  └─ plan: Arc<RwLock<Vec<PlanItem>>>        ← owned here
       │
       ├── Turn N → tools → UpdatePlanTool    ← clone #1
       └── Turn N → tools → UpdatePlanTool    ← clone #2 (same turn, same clone)
```

### 3. Plan 状态属于 Thread，不属于 Turn

**Decision**：Plan 由 Thread 持有，通过 `TurnBuilder` 传给 Turn。

**Rationale**：Turn 实例每个 turn 创建一次，执行后丢弃。Thread 是长生命周期的容器。Plan 应该在未来能跨 turn 存活，并随时可被外部消费者读取。

**对比**：Turn 级 plan 在 turn 结束时丢失状态。

### 4. UpdatePlanTool 放在 `argus-thread`，不在 `argus-tool`

**Decision**：`UpdatePlanTool` 位于 `crates/argus-thread/src/plan_tool.rs`。

**Rationale**：`UpdatePlanTool` 与 `Thread.plan` 紧耦合（Thread 级关注点）。`argus-tool` 包含通用可复用工具（read、shell、grep）。Thread 专属工具属于 argus-thread。

### 5. 全量覆盖语义

**Decision**：LLM 每次调用都发送完整计划。工具完全覆盖状态。

**Rationale**：最简单的语义 — 无读写合并写入的竞态，无部分更新。LLM 负责每次发送完整更新的计划。这符合 TODO 工具的典型做法。

### 6. 内部存储为 `Vec<serde_json::Value>`

**Decision**：Thread.plan 为 `Arc<RwLock<Vec<serde_json::Value>>>`。

**Rationale**：避免在 protocol 中定义单独的 `PlanItem` 结构。JSON schema 定义在工具的 `definition()` 中，工具按需序列化/反序列化。`serde_json::Value` 已通过 argus-protocol 的依赖进入作用域。

### 7. 风险等级：Low

**Decision**：`UpdatePlanTool::risk_level()` 返回 `RiskLevel::Low`。

**Rationale**：工具仅写入内存 Vec。无文件系统、网络或 Shell 访问。不需要审批。

## Risks / Trade-offs

- **【Turn 失败时 plan 丢失】** → 当前范围可接受。未来：添加 `FilePlanStore` 实现。
- **【Thread 重启后 plan 不持久化】** → 可接受。未来：添加持久化层。
- **【无冲突解决】** → 工具无条件接受任何 plan 更新。格式错误的 plan 返回错误。LLM 负责提供有效的 plan 状态。
- **【无 plan 验证】** → 工具不验证标记为 `completed` 的步骤是否真正完成。信任交由 LLM。

## Open Questions

- `explanation` 是否应该存储在 plan 条目中（作为每个条目的字段），还是仅记录日志？
  - **Decision**：仅记录日志。不存入 plan。
- Plan 最大大小是多少？是否需要限制？
  - **Decision**：暂不限制。根据实际使用监控。
