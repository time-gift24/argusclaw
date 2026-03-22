# job-dispatch-tool

## ADDED Requirements

### Requirement: dispatch_job 工具对 LLM 可用

系统 SHALL 提供 `dispatch_job` 工具，注册到 Thread 的工具集合中，在 turn 执行期间可供 LLM 调用。

### Requirement: dispatch_job 接受任务派发参数

`dispatch_job` 工具 SHALL 接受以下参数：
- `prompt: String` (必填) — 任务的 prompt
- `agent_id: i64` (必填) — subagent 模板 ID
- `context: Optional<JSON>` — 额外的上下文数据
- `wait_for_result: bool` (默认 false) — 是否等待结果

### Requirement: dispatch_job 返回派发结果

成功执行时，工具 SHALL 返回包含以下字段的 JSON 对象：
- `job_id: String` — 任务的唯一 ID
- `status: String` — "submitted" 或 "completed"
- `result: Optional<JobResult>` — 当 wait_for_result=true 时的结果

### Requirement: Subagent 不能派发任务

若调用 `dispatch_job` 的 agent 类型为 `'subagent'`，工具 SHALL 返回错误。

### Requirement: dispatch_job 验证 agent_id

若 `agent_id` 对应的 agent 不存在，工具 SHALL 返回错误。

### Requirement: dispatch_job 失败时重试

若 dispatch 失败是由于 rate limit 或其他瞬时错误，工具 SHALL 自动重试最多 3 次。

### Requirement: Job 完成时发送 SSE 事件

当 Job 完成（成功/失败/超时）时，系统 SHALL 发送 SSE 事件到 session 的 broadcast channel。

SSE 事件格式：
```json
{
  "type": "job_result",
  "job_id": "uuid",
  "status": "completed|failed|stuck",
  "session_id": "optional-session-id"
}
```

### Requirement: 主 Agent 可轮询 Job 结果

Agent SHALL 能够通过 `get_job_result(job_id)` 查询 job 状态和获取结果。

### Requirement: Job 结果可被存储

Job 的执行结果 SHALL 持久化到数据库，供后续查询。

### Requirement: dispatch_job 风险等级为 Medium

`dispatch_job` 工具 SHALL 具有 `RiskLevel::Medium`，可能需要审批（取决于 context 内容）。

## Scenario: Agent 派发后台任务

- **WHEN** 主 agent 调用 `dispatch_job` 且 `wait_for_result: false`
- **THEN** 工具立即返回 `job_id` 和 `status: "submitted"`
- **AND** Job 在后台异步执行

## Scenario: Agent 派发并等待结果

- **WHEN** 主 agent 调用 `dispatch_job` 且 `wait_for_result: true`
- **THEN** 工具阻塞直到 Job 完成
- **AND** 返回 `status: "completed"` 和 `result`

## Scenario: Subagent 尝试派发任务

- **WHEN** agent_type 为 `'subagent'` 的 agent 调用 `dispatch_job`
- **THEN** 工具返回错误，包含 "subagent cannot dispatch jobs"
- **AND** 无 job 被创建

## Scenario: 派发到不存在的 agent

- **WHEN** LLM 调用 `dispatch_job` 且 `agent_id` 不存在
- **THEN** 工具返回错误，包含 "agent not found"
- **AND** 无 job 被创建

## Scenario: Job 完成通知

- **WHEN** Job 执行完成（成功/失败/超时）
- **THEN** SSE 事件发送到 session broadcast channel
- **AND** Job 状态更新到数据库

## Scenario: Job 派发重试

- **WHEN** dispatch_job 首次调用因 rate limit 失败
- **THEN** 系统自动等待后重试
- **AND** 最多重试 3 次
- **AND** 若全部失败，返回最终错误
