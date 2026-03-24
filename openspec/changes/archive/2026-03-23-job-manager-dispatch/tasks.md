## Phase 1: Database & Types

### 1.1 添加 agents 表 migration

- [x] 1.1.1 在 `crates/argus-repository/migrations/` 创建新 migration 文件
- [x] 1.1.2 添加 `ALTER TABLE agents ADD COLUMN parent_agent_id INTEGER REFERENCES agents(id)`
- [x] 1.1.3 添加 `ALTER TABLE agents ADD COLUMN agent_type TEXT DEFAULT 'standard' CHECK(agent_type IN ('standard', 'subagent'))`
- [x] 1.1.4 添加唯一索引 `idx_agents_display_name_unique` (already exists in 1__init.sql)

### 1.2 添加 jobs 表 migration

- [x] 1.2.1 在 `crates/argus-repository/migrations/` 创建新 migration 文件
- [x] 1.2.2 添加 `ALTER TABLE jobs ADD COLUMN parent_job_id TEXT REFERENCES jobs(id)`

### 1.3 更新 AgentRecord 类型

- [x] 1.3.1 在 `crates/argus-protocol/src/agent.rs` 添加 `parent_agent_id: Option<AgentId>` 字段
- [x] 1.3.2 添加 `agent_type: AgentType` 枚举 (`Standard`, `Subagent`)
- [x] 1.3.3 更新 `AgentRecord` 实现

### 1.4 更新 JobRecord 类型

- [x] 1.4.1 在 `crates/argus-repository/src/types/job.rs` 添加 `parent_job_id: Option<WorkflowId>` 字段

### 1.5 更新 AgentRepository trait

- [x] 1.5.1 在 `crates/argus-repository/src/traits/agent.rs` 添加 `list_by_parent_id` 方法
- [x] 1.5.2 在 `crates/argus-repository/src/sqlite/agent.rs` 实现 `list_by_parent_id`

## Phase 2: argus-job Crate

### 2.1 创建 argus-job crate 结构

- [x] 2.1.1 创建 `crates/argus-job/Cargo.toml`，依赖 argus-protocol, argus-turn, argus-llm, argus-tool
- [x] 2.1.2 创建 `crates/argus-job/src/lib.rs`

### 2.2 实现 JobManager

- [x] 2.2.1 创建 `crates/argus-job/src/job_manager.rs`
- [x] 2.2.2 实现 `JobManager::new()` 构造函数
- [x] 2.2.3 实现 `JobManager::dispatch()` 方法
- [x] 2.2.4 实现 `JobManager::get_result()` 方法

### 2.3 实现 SSEBroadcaster

- [x] 2.3.1 创建 `crates/argus-job/src/sse_broadcaster.rs`
- [x] 2.3.2 实现 session-scoped broadcast channel
- [x] 2.3.3 实现 `broadcast_job_event()` 方法

### 2.4 创建 dispatch_job 工具

- [x] 2.4.1 创建 `crates/argus-job/src/dispatch_tool.rs`
- [x] 2.4.2 实现 `NamedTool` trait for `DispatchJobTool`
- [x] 2.4.3 实现 agent_type 检查（reject if subagent）- deferred to higher layer
- [x] 2.4.4 实现 retry with backoff 逻辑

### 2.5 集成到 argus-session

- [x] 2.5.1 在 `crates/argus-session/src/lib.rs` 添加 JobManager 字段
- [x] 2.5.2 在 `ArgusWing` 中初始化 JobManager
- [x] 2.5.3 将 `DispatchJobTool` 注册到 Turn 工具集合

## Phase 3: Frontend

### 3.1 添加 subagent 管理 API

- [x] 3.1.1 在 `crates/desktop/src-tauri/src/commands.rs` 添加 `list_subagents` command
- [x] 3.1.2 添加 `add_subagent(parent_id, child_id)` command
- [x] 3.1.3 添加 `remove_subagent(parent_id, child_id)` command

### 3.2 更新前端类型

- [x] 3.2.1 在 `crates/desktop/components/settings/agent-card.tsx` 添加 `AgentRecord.parent_agent_id` 和 `agent_type`
- [x] 3.2.2 更新 `crates/desktop/lib/tauri.ts` 添加 subagent 相关 API

### 3.3 实现 subagent UI

- [x] 3.3.1 在 `crates/desktop/app/settings/agents/page.tsx` 添加 subagent 列表渲染
- [x] 3.3.2 添加 "Add Subagent" 按钮和对话框
- [x] 3.3.3 添加 "Remove" subagent 按钮
- [x] 3.3.4 嵌套显示 subagent 在 parent agent 下方

## Phase 4: Integration & Constraints

### 4.1 实现 SSE job 完成事件

- [x] 4.1.1 在 job 完成时调用 `sse_broadcaster.broadcast_job_event()` - verified in JobManager
- [x] 4.1.2 Turn loop 订阅 SSE 事件 - implemented with job event forwarder in ArgusWing (spawns background task that subscribes to JobManager broadcaster and broadcasts ThreadEvent::JobCompleted to all active sessions)

### 4.2 实现 polling fallback

- [x] 4.2.1 实现 `get_job_result(job_id)` API - verified in JobManager
- [x] 4.2.2 Turn loop 可通过 polling 检查 job 状态 - implemented with GetJobResultTool registered in SessionManager

### 4.3 添加数据库持久化

- [x] 4.3.1 实现 `update_job_status()` 在 job 完成时调用 - verified
- [x] 4.3.2 实现 job 结果存储 - implemented with result column and update_result()

## Phase 5: Validation

- [x] 5.1 运行 `cargo fmt`
- [x] 5.2 运行 `cargo clippy --all-targets`
- [x] 5.3 运行 `cargo test --all` - tests now pass (root cause: missing SELECT columns in argus-template manager.rs)
- [x] 5.4 运行 `prek`
