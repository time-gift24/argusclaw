# plan-persistence — Plan 持久化

## 职责

将 LLM 的 plan 状态持久化到文件系统，支持 Thread 重启后恢复 plan。

## 实现细节

### FilePlanStore

```
{trace_dir}/{thread_id}/plan.json
```

- trace_dir 由 SessionManager 管理，每个 Thread 的 trace 目录为 `{trace_dir}/{thread_id}/`
- plan.json 在首次 `update_plan` 调用时创建，后续每次调用覆盖
- 文件格式为 JSON，存储 `Vec<serde_json::Value>`
- 持久化失败（IO 错误）不影响内存中的 plan 状态，错误仅记录日志

### Thread 集成

- Thread 持有 `FilePlanStore` 而非直接持有 `Arc<RwLock<Vec<Value>>>`
- FilePlanStore 内部包含 `store: Arc<RwLock<Vec<Value>>>` 字段
- UpdatePlanTool 从 Thread 获取 store 引用，写入时同时更新内存和文件
- Thread 重启时，从文件恢复 plan 到内存

## ADDED Requirements

### Requirement: Plan 持久化到文件系统

FilePlanStore SHALL 将 plan 内容持久化到 `{trace_dir}/{thread_id}/plan.json`，每次 `update_plan` 调用时覆盖文件。

#### Scenario: 首次 update_plan 创建文件

- **WHEN** LLM 调用 `update_plan` 且 plan.json 不存在
- **THEN** FilePlanStore 创建 plan.json 并写入当前 plan 内容

#### Scenario: 后续 update_plan 覆盖文件

- **WHEN** LLM 调用 `update_plan` 且 plan.json 已存在
- **THEN** FilePlanStore 覆盖 plan.json 为新的 plan 内容

#### Scenario: Thread 重启后从文件恢复

- **WHEN** Thread 被创建且对应的 plan.json 存在
- **THEN** FilePlanStore 从文件读取 plan 内容到内存

#### Scenario: 持久化失败不影响内存

- **WHEN** 写入 plan.json 时发生 IO 错误
- **THEN** 内存中的 plan 状态正常更新，仅记录错误日志

### Requirement: plan_item_count 包含在 ThreadInfo

ThreadInfo SHALL 包含 `plan_item_count: usize`，反映当前 plan 中的步骤数量。

#### Scenario: 空 plan 时 count 为零

- **WHEN** plan 为空
- **THEN** ThreadInfo.plan_item_count 等于 0

#### Scenario: plan 有内容时返回实际数量

- **WHEN** plan 包含 N 个步骤
- **THEN** ThreadInfo.plan_item_count 等于 N
