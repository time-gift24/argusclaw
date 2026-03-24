## Why

Agent 在执行过程中需要一种跟踪和更新任务进度的方式。目前没有内置机制让 LLM 在一个 turn 内的多个工具调用之间维护 TODO 列表。`update_plan` 工具让 LLM 能够以结构化的方式声明、更新和报告自己的任务计划——实现更自主、更自驱的 agent 行为。

## What Changes

- **`update_plan` 工具**：位于 `argus-thread`，低风险工具，允许 LLM 在 turn 执行期间更新每个 Thread 的计划状态。
- **Plan 类型**：位于 `argus-protocol`：`StepStatus`、`PlanItemArg`、`UpdatePlanArgs` — 用于 plan 序列化/反序列化的共享类型。
- **Thread 级 plan 状态**：`Thread.plan: Arc<RwLock<Vec<PlanItem>>>` — 在 turn 内的工具调用之间持久化 plan，外部消费者（UI、日志）可访问。
- **Plan 工具实现** `UpdatePlanTool`：位于 `argus-thread/src/plan_tool.rs`，实现 `NamedTool`，操作共享 plan 状态。

## Capabilities

### New Capabilities

- `update-plan-tool`：LLM 可用的工具，管理每个 Thread 的任务计划。LLM 发送完整的计划快照；工具覆盖状态并返回更新后的计划。支持 `pending`、`in_progress`、`completed` 三种状态，以及可选的 `explanation` 字段用于可审计性。

### Modified Capabilities

*（无）*

## Impact

- **新增文件**：
  - `crates/argus-protocol/src/plan.rs` — plan 类型
  - `crates/argus-thread/src/plan_tool.rs` — 工具实现
- **修改文件**：
  - `crates/argus-protocol/src/lib.rs` — 导出 plan 类型
  - `crates/argus-thread/src/lib.rs` — 导出 plan_tool 模块
  - `crates/argus-thread/src/thread.rs` — 添加 `plan` 字段，用 plan clone 构建 `UpdatePlanTool`，传给 `TurnBuilder`
- **无新增 crate 依赖**：所有依赖均已存在（`serde`、`async-trait`、`argus-protocol`）
- **风险等级**：`Low` — 工具仅写入内存中的 `Vec`，无文件系统或网络访问
