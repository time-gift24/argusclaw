## Context

Argusclaw 的 Agent 在执行过程中需要一种派发后台任务的方式。目前没有机制让 LLM 派发异步 job 并获得完成通知。

参考 ironclaw 的 Job/Worker/JobDelegate 模式，我们需要在 argusclaw 中实现类似的能力。

## Decisions

### 1. 新建 `argus-job` crate，依赖 `argus-turn`

**Decision**：创建独立 `argus-job` crate，复用 `argus-turn` 的执行模式。

**Rationale**：Job 执行与 Turn 执行有相似模式（LLM 调用、工具执行、循环委托），但 Job 是长生命周期任务。独立 crate 保持关注点分离，同时允许复用 turn 相关代码。

### 2. Agent 通过 `dispatch_job` 工具派发任务

**Decision**：在 argus-session 中实现 `dispatch_job` 工具，签名：

```rust
dispatch_job(
    prompt: String,
    agent_id: i64,
    context: Option<JSON>,
    wait_for_result: bool,
) -> Result<JobDispatchResult, DispatchError>
```

**Rationale**：工具调用接口符合现有 argus-tool 模式，LLM 可直接使用。`wait_for_result` 控制同步/异步执行。

### 3. Subagent 通过 `parent_agent_id` + `agent_type` 标识

**Decision**：agents 表增加两列：
- `parent_agent_id: INTEGER REFERENCES agents(id)` — subagent 的父 agent
- `agent_type: TEXT DEFAULT 'standard' CHECK(agent_type IN ('standard', 'subagent'))` — agent 类型

**Rationale**：`parent_agent_id` 建立 parent-child 关系用于 UI 显示。`agent_type` 用于权限检查，subagent 不能派发 job。

### 4. Subagent 不能派发 job（agent_type 检查）

**Decision**：当 `agent_type == 'subagent'` 的 agent 调用 `dispatch_job` 时，工具返回错误。

**Rationale**：防止 subagent 无限递归派发任务。

### 5. Job 完成通过 SSE + Polling 通知

**Decision**：Job 完成时：
1. 通过 SSE 广播 `JobResult` 事件到 session 的 broadcast channel
2. 主 agent 的 turn loop 订阅 SSE 事件
3. Fallback：主 agent 可通过 `get_job_result(job_id)` 轮询状态

**Rationale**：SSE 提供实时通知，polling 作为 fallback 保证可靠性。

### 6. SSE broadcast channel 按 session 作用域

**Decision**：每个 session 有一个 broadcast channel。所有 job 事件发到该 session 的 channel。

**Rationale**：简单一致，ironclaw 已验证的模式。Agent 订阅自己 session 的 channel 即可。

### 7. Job 执行复用 argus-turn 模式

**Decision**：JobExecution 复用 TurnExecution 的模式，但使用 JobDelegate 而非 TurnDelegate。

**Rationale**：最小化重复代码。Job 和 Turn 的执行流程相似（LLM 调用 → 工具执行 → 循环）。

### 8. Job 错误处理：Retry with backoff

**Decision**：dispatch_job 失败时（rate limit、transient error），自动重试最多 3 次。

**Rationale**：处理瞬时故障，提高可靠性。

## Risks / Trade-offs

- **【Job 执行生命周期管理】** → JobManager 需要管理 job 的启动、执行、完成/失败状态，防止 zombie job
- **【SSE channel 资源清理】** → Session 结束时需要清理 broadcast channel，防止泄漏
- **【Subagent 删除时级联】** → 删除 subagent 时需要处理其关联的 job（暂不删除，标记为 orphaned）
- **【Job 结果长期存储】** → Job 结果在 DB 中存储多久需要定义生命周期策略

## Open Questions

- Job 最大执行时间是多少？超时如何处理？
  - **TBD**：考虑添加 `max_duration` 配置
- Subagent 删除时，其关联的 parent_agent_id 如何处理？
  - **TBD**：设置为 NULL 或级联删除
