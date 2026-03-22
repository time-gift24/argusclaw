## Why

Agent 模板需要在执行过程中能够派发后台任务给 subagent。目前 argusclaw 缺少一种机制让主 agent 能够异步派发 job 并获得完成通知。

目标：添加 `dispatch_job` 工具，让 agent 能够派发后台 job 到 subagent，通过 SSE + polling fallback 实现完成检测。

## What Changes

- **新增 `argus-job` crate**：包含 JobManager、SSEBroadcaster、JobExecution（复用 argus-turn 模式）
- **`dispatch_job` 工具**：位于 argus-session，低风险工具，允许 agent 派发后台 job
- **数据库 schema 变更**：agents 表增加 `parent_agent_id`、`agent_type`；jobs 表增加 `parent_job_id`
- **前端变更**：Agent 设置页面嵌套显示 subagent，支持添加/删除 subagent 关联

## Capabilities

### New Capabilities

- **job-dispatch-tool**：LLM 可用的工具，管理每个 Thread 的任务计划。LLM 发送完整的计划快照；工具覆盖状态并返回更新后的计划。支持 `pending`、`in_progress`、`completed` 三种状态，以及可选的 `explanation` 字段用于可审计性。

### Modified Capabilities

- **subagent 管理**：Agent 设置页面支持嵌套 subagent 显示和关联管理

## Impact

- **新增文件**：
  - `crates/argus-job/src/lib.rs` — 新 crate 入口
  - `crates/argus-job/src/job_manager.rs` — JobManager 实现
  - `crates/argus-job/src/dispatch_tool.rs` — dispatch_job 工具
  - `crates/argus-job/src/sse_broadcaster.rs` — SSE 广播
  - `crates/argus-protocol/src/job.rs` — job 相关类型
- **修改文件**：
  - `crates/argus-repository/migrations/` — 新增 migration
  - `crates/argus-repository/src/types/job.rs` — JobRecord
  - `crates/argus-repository/src/sqlite/agent.rs` — agent 查询
  - `crates/argus-session/src/lib.rs` — 集成 JobManager
  - `crates/desktop/app/settings/agents/page.tsx` — 前端 subagent UI
  - `crates/desktop/components/settings/agent-card.tsx` — agent 卡组件
- **风险等级**：`Medium` — 涉及后台任务执行和 SSE，需要正确处理生命周期
