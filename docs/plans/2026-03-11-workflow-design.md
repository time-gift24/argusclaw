# Workflow 模块设计

## 概述

构建一个 workflow 工具，采用 Stage 模型（Stage 串行，Job 并行），每个 Job 最终派发给一个 Agent 执行。本阶段专注于数据模型和状态管理，不涉及 LLM 调用。

## 架构

```
workflows ─┬─< stages ─┬─< jobs
           │           │
           │           └── agent_id, status
           └── status (derived)
```

- **Workflow**: 一次工作流执行实例
- **Stage**: 工作流阶段，Stage 之间串行执行
- **Job**: 具体执行单元，同一 Stage 内的 Job 并行执行，绑定一个 Agent

## 数据库 Schema

### workflows 表

```sql
CREATE TABLE workflows (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

### stages 表

```sql
CREATE TABLE stages (
    id TEXT PRIMARY KEY,
    workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_stages_workflow_id ON stages(workflow_id);
CREATE UNIQUE INDEX idx_stages_workflow_sequence ON stages(workflow_id, sequence);
```

### jobs 表

```sql
CREATE TABLE jobs (
    id TEXT PRIMARY KEY,
    stage_id TEXT NOT NULL REFERENCES stages(id) ON DELETE CASCADE,
    agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    started_at TEXT,
    finished_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_jobs_stage_id ON jobs(stage_id);
CREATE INDEX idx_jobs_agent_id ON jobs(agent_id);
```

## 状态模型

### 状态枚举

Workflow/Stage/Job 共享相同状态：

| 状态 | 说明 |
|------|------|
| `pending` | 等待执行 |
| `running` | 执行中 |
| `succeeded` | 成功完成 |
| `failed` | 执行失败 |
| `cancelled` | 被取消 |

### 状态转换规则

**Job 状态转换：**
```
pending → running → succeeded
                  ↘ failed
                  ↘ cancelled
```

**Stage 状态（由 Job 聚合推导）：**
- `pending`: 所有 Job 都是 pending
- `running`: 至少一个 Job 是 running，且没有 failed/cancelled
- `succeeded`: 所有 Job 都是 succeeded
- `failed`: 任一 Job 是 failed
- `cancelled`: 任一 Job 是 cancelled

**Workflow 状态（由 Stage 聚合推导）：**
- 规则同 Stage，基于 Stage 状态

## 模块结构

```
crates/claw/src/
├── workflow/
│   ├── mod.rs           # 模块入口和导出
│   ├── types.rs         # 领域类型 (WorkflowId, StageId, JobId, *Record, WorkflowStatus)
│   └── repository.rs    # WorkflowRepository trait
├── db/
│   ├── mod.rs           # 添加 workflow 模块导出
│   └── sqlite/
│       └── workflow.rs  # SqliteWorkflowRepository 实现
└── migrations/
    └── 20260311000001_create_workflows.sql
```

## 领域类型

```rust
/// 工作流执行状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

/// Workflow 记录
pub struct WorkflowRecord {
    pub id: WorkflowId,
    pub name: String,
    pub status: WorkflowStatus,
}

/// Stage 记录
pub struct StageRecord {
    pub id: StageId,
    pub workflow_id: WorkflowId,
    pub name: String,
    pub sequence: i32,
    pub status: WorkflowStatus,
}

/// Job 记录
pub struct JobRecord {
    pub id: JobId,
    pub stage_id: StageId,
    pub agent_id: AgentId,
    pub name: String,
    pub status: WorkflowStatus,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}
```

## Repository Trait

```rust
#[async_trait]
pub trait WorkflowRepository: Send + Sync {
    // Workflow CRUD
    async fn create_workflow(&self, workflow: &WorkflowRecord) -> Result<(), DbError>;
    async fn get_workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowRecord>, DbError>;
    async fn update_workflow_status(&self, id: &WorkflowId, status: WorkflowStatus) -> Result<(), DbError>;
    async fn list_workflows(&self) -> Result<Vec<WorkflowRecord>, DbError>;
    async fn delete_workflow(&self, id: &WorkflowId) -> Result<bool, DbError>;

    // Stage CRUD
    async fn create_stage(&self, stage: &StageRecord) -> Result<(), DbError>;
    async fn list_stages_by_workflow(&self, workflow_id: &WorkflowId) -> Result<Vec<StageRecord>, DbError>;
    async fn update_stage_status(&self, id: &StageId, status: WorkflowStatus) -> Result<(), DbError>;

    // Job CRUD
    async fn create_job(&self, job: &JobRecord) -> Result<(), DbError>;
    async fn list_jobs_by_stage(&self, stage_id: &StageId) -> Result<Vec<JobRecord>, DbError>;
    async fn update_job_status(
        &self,
        id: &JobId,
        status: WorkflowStatus,
        started_at: Option<&str>,
        finished_at: Option<&str>,
    ) -> Result<(), DbError>;
}
```

## Dev CLI 命令

```bash
# Workflow 管理
cargo run --features dev -- workflow create <name>
cargo run --features dev -- workflow list
cargo run --features dev -- workflow show <id>
cargo run --features dev -- workflow delete <id>

# Stage 管理
cargo run --features dev -- workflow add-stage <workflow_id> <name> <sequence>

# Job 管理
cargo run --features dev -- workflow add-job <stage_id> <agent_id> <name>
cargo run --features dev -- workflow job-status <job_id> <status>

# 状态查询
cargo run --features dev -- workflow status <id>
```

**示例输出：**
```
$ cargo run --features dev -- workflow status wf-001

Workflow: data-pipeline (running)
├── Stage 0: fetch (succeeded)
│   ├── Job: fetch-a (agent: fetcher-a) [succeeded]
│   └── Job: fetch-b (agent: fetcher-b) [succeeded]
├── Stage 1: process (running)
│   ├── Job: transform (agent: processor) [running]
│   └── Job: validate (agent: validator) [pending]
└── Stage 2: export (pending)
    └── Job: upload (agent: uploader) [pending]
```

## 测试策略

### 单元测试（内联）

- `WorkflowStatus` 的 `FromStr`/`Display` 实现
- 状态聚合逻辑

### 集成测试

`crates/claw/tests/workflow_integration_test.rs`:

- 创建 Workflow + Stage + Job 的完整流程
- 状态更新与聚合验证
- 级联删除验证
- 外键约束验证（不存在的 agent_id）

## 约束

1. Stage 的 `sequence` 在同一 Workflow 内唯一
2. Job 只能关联已存在的 `agent_id`
3. 删除 Workflow 时级联删除所有 Stage 和 Job
4. 删除 Agent 时阻止如果有关联 Job

## 不在本阶段范围

- LLM 调用逻辑
- Workflow 执行引擎（调度、状态轮询）
- Job 输入/输出传递
- 重试机制
