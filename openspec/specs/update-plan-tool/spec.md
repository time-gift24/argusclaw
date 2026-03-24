# update-plan-tool Specification

## Purpose
TBD - created by archiving change add-update-plan-tool. Update Purpose after archive.
## Requirements
### Requirement: update_plan 工具对 LLM 可用

系统 SHALL 提供 `update_plan` 工具，注册到 Thread 的工具集合中，在 turn 执行期间可供 LLM 调用。

### Requirement: update_plan 接受计划更新

`update_plan` 工具 SHALL 接受 `plan` 数组（必填）和可选的 `explanation` 字符串。LLM 每次调用发送完整计划；工具完全覆盖 Thread 的计划状态。

### Requirement: update_plan 返回更新后的计划

成功执行时，工具 SHALL 返回包含完整更新计划和元数据的 JSON 对象：
- `plan`：完整更新后的计划数组
- `updated`：更新的条目数
- `total`：计划中的总条目数

### Requirement: update_plan 拒绝空计划

若 `plan` 数组为空，工具 SHALL 返回错误。

### Requirement: update_plan 校验步骤状态

工具 SHALL 仅接受 `pending`、`in_progress`、`completed` 作为有效的步骤状态。未知状态 SHALL 返回错误。

### Requirement: 计划状态在 turn 内的工具调用之间持久化

计划状态 SHALL 在同一 turn 执行内的多个工具调用之间持久化。同一 turn 内所有对 `update_plan` 的调用共享同一计划状态。

### Requirement: 计划可被外部读取

外部消费者 SHALL 能够随时从 `Thread.plan` 读取当前计划。

### Requirement: update_plan 风险等级为 Low

`update_plan` 工具 SHALL 具有 `RiskLevel::Low`，SHALL NOT 需要审批。

### Requirement: explanation 仅记录日志，不存储

可选的 `explanation` 参数 SHALL 以 `debug` 级别记录日志，但 SHALL NOT 持久化到计划状态中。

#### Scenario: LLM 更新单个条目的计划
- **WHEN** LLM 调用 `update_plan` 且 `plan: [{step: "实现功能 X", status: "completed"}]`
- **THEN** Thread 的计划状态恰好包含一个状态为 `completed` 的条目
- **AND** 工具返回 `{plan: [...], updated: 1, total: 1}`

#### Scenario: LLM 更新包含多个条目的计划
- **WHEN** LLM 调用 `update_plan` 且计划包含 5 个混合状态的条目
- **THEN** Thread 的计划状态包含全部 5 个条目及其各自的状态
- **AND** 工具返回 `total: 5`

#### Scenario: LLM 发送空计划
- **WHEN** LLM 调用 `update_plan` 且 `plan: []`
- **THEN** 工具返回包含 "empty" 的错误消息
- **AND** Thread 的计划状态保持不变

#### Scenario: 计划在多次工具调用之间持久化
- **WHEN** LLM 在同一 turn 内两次调用 `update_plan`
- **THEN** 第二次调用能看见第一次调用更新的计划状态
- **AND** 返回的计划反映累积更新

#### Scenario: LLM 提供 explanation
- **WHEN** LLM 调用 `update_plan` 且 `explanation: "完成了第 1 步"`
- **THEN** explanation 以 debug 级别记录日志
- **AND** 返回的计划不包含 explanation

#### Scenario: 无效的步骤状态
- **WHEN** LLM 调用 `update_plan` 且 `status: "invalid_status"`
- **THEN** 工具返回错误
- **AND** Thread 的计划状态保持不变

