# Workflow 模块实现计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 构建一个 workflow 工具，采用 Stage 模型（Stage 串行，Job 并行），专注于数据模型和 SQLite 持久化。

**Architecture:** 三表设计（workflows → stages → jobs），Repository trait 模式，async/await。

**Tech Stack:** Rust, SQLx, SQLite, async-trait, uuid, chrono, clap, serde.

---

## Task 1: 创建数据库迁移文件

**Files:**
- Create: `crates/claw/migrations/20260311000001_create_workflows.sql`

**Step 1: 创建迁移文件**

创建包含 workflows、stages、jobs 三张表的 SQL 迁移文件。

```sql
-- crates/claw/migrations/20260311000001_create_workflows.sql
CREATE TABLE IF NOT EXISTS workflows (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS stages (
    id TEXT PRIMARY KEY,
    workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_stages_workflow_id ON stages(workflow_id);
CREATE UNIQUE INDEX idx_stages_workflow_sequence ON stages(workflow_id, sequence);

CREATE TABLE IF NOT EXISTS jobs (
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

CREATE INDEX IF NOT EXISTS idx_jobs_stage_id ON jobs(stage_id);
CREATE INDEX IF NOT EXISTS idx_jobs_agent_id ON jobs(agent_id);
```

**Step 2: 运行格式化**

Run: `cargo fmt -- --path crates/claw/migrations/20260311000001_create_workflows.sql`
Expected: 文件已格式化（SQL 文件通常无需格式化，但保持一致性）

**Step 3: Commit**

```bash
git add crates/claw/migrations/20260311000001_create_workflows.sql
git commit -m "feat(workflow): add database migration for workflows, stages, and jobs tables"
```

---

## Task 2: 创建领域类型 - ID 类型

**Files:**
- Create: `crates/claw/src/workflow/types.rs`

**Step 1: 编写 WorkflowId, StageId, JobId 类型**

```rust
//! crates/claw/src/workflow/types.rs

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Unique identifier for a workflow.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkflowId(String);

impl WorkflowId {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        debug_assert!(!id.is_empty(), "WorkflowId cannot be empty");
        Self(id)
    }
}

impl AsRef<str> for WorkflowId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkflowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for WorkflowId {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

/// Unique identifier for a stage.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StageId(String);

impl StageId {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        debug_assert!(!id.is_empty(), "StageId cannot be empty");
        Self(id)
    }
}

impl AsRef<str> for StageId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for StageId {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

/// Unique identifier for a job.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(String);

impl JobId {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        debug_assert!(!id.is_empty(), "JobId cannot be empty");
        Self(id)
    }
}

impl AsRef<str> for JobId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for JobId {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}
```

**Step 2: 添加测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_id_from_str() {
        let id = WorkflowId::new("wf-123");
        assert_eq!(id.as_ref(), "wf-123");
        assert_eq!(id.to_string(), "wf-123");
    }

    #[test]
    fn stage_id_display() {
        let id = StageId::new("stage-001");
        assert_eq!(format!("{id}"), "stage-001");
    }

    #[test]
    fn job_id_from_string() {
        let id: JobId = "job-abc".parse().unwrap();
        assert_eq!(id.as_ref(), "job-abc");
    }
}
```

**Step 3: 运行测试**

Run: `cargo test --package claw -- workflow::types -- --nocapture`
Expected: PASS（所有 3 个测试通过）

**Step 4: Commit**

```bash
git add crates/claw/src/workflow/types.rs
git commit -m "feat(workflow): add WorkflowId, StageId, JobId newtype wrappers"
```

---

## Task 3: 创建 WorkflowStatus 枚举

**Files:**
- Modify: `crates/claw/src/workflow/types.rs`

**Step 1: 添加 WorkflowStatus 枚举**

在 `types.rs` 文件末尾，测试模块之前添加：

```rust
/// Workflow execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl WorkflowStatus {
    /// Returns the string representation of the status.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parse a status string.
    /// # Errors
    /// Returns an error if the string is not a valid status.
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("invalid workflow status: {s}")),
        }
    }
}

impl fmt::Display for WorkflowStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<&str> for WorkflowStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::from_str(value)
    }
}
```

**Step 2: 添加测试**

在测试模块中添加：

```rust
    #[test]
    fn workflow_status_as_str() {
        assert_eq!(WorkflowStatus::Pending.as_str(), "pending");
        assert_eq!(WorkflowStatus::Running.as_str(), "running");
        assert_eq!(WorkflowStatus::Succeeded.as_str(), "succeeded");
        assert_eq!(WorkflowStatus::Failed.as_str(), "failed");
        assert_eq!(WorkflowStatus::Cancelled.as_str(), "cancelled");
    }

    #[test]
    fn workflow_status_from_str_valid() {
        assert_eq!(WorkflowStatus::from_str("pending"), Ok(WorkflowStatus::Pending));
        assert_eq!(WorkflowStatus::from_str("succeeded"), Ok(WorkflowStatus::Succeeded));
    }

    #[test]
    fn workflow_status_from_str_invalid() {
        assert!(WorkflowStatus::from_str("invalid").is_err());
    }

    #[test]
    fn workflow_status_display() {
        assert_eq!(format!("{}", WorkflowStatus::Failed), "failed");
    }
```

**Step 3: 运行测试**

Run: `cargo test --package claw -- workflow::types::tests::workflow_status -- --nocapture`
Expected: PASS（所有 4 个 status 相关测试通过）

**Step 4: Commit**

```bash
git add crates/claw/src/workflow/types.rs
git commit -m "feat(workflow): add WorkflowStatus enum with string conversion"
```

---

## Task 4: 创建 Record 类型

**Files:**
- Modify: `crates/claw/src/workflow/types.rs`

**Step 1: 添加 Record 结构体**

在 `WorkflowStatus` 实现之后，测试模块之前添加：

```rust
use crate::agents::AgentId;

/// Workflow record stored in database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRecord {
    pub id: WorkflowId,
    pub name: String,
    pub status: WorkflowStatus,
}

/// Stage record stored in database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageRecord {
    pub id: StageId,
    pub workflow_id: WorkflowId,
    pub name: String,
    pub sequence: i32,
    pub status: WorkflowStatus,
}

/// Job record stored in database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: JobId,
    pub stage_id: StageId,
    pub agent_id: AgentId,
    pub name: String,
    pub status: WorkflowStatus,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

impl WorkflowRecord {
    /// Create a minimal workflow record for testing.
    #[cfg(test)]
    pub fn for_test(id: &str, name: &str) -> Self {
        Self {
            id: WorkflowId::new(id),
            name: name.to_string(),
            status: WorkflowStatus::Pending,
        }
    }
}

impl StageRecord {
    /// Create a minimal stage record for testing.
    #[cfg(test)]
    pub fn for_test(id: &str, workflow_id: &str, name: &str, sequence: i32) -> Self {
        Self {
            id: StageId::new(id),
            workflow_id: WorkflowId::new(workflow_id),
            name: name.to_string(),
            sequence,
            status: WorkflowStatus::Pending,
        }
    }
}

impl JobRecord {
    /// Create a minimal job record for testing.
    #[cfg(test)]
    pub fn for_test(id: &str, stage_id: &str, agent_id: &str, name: &str) -> Self {
        Self {
            id: JobId::new(id),
            stage_id: StageId::new(stage_id),
            agent_id: AgentId::new(agent_id),
            name: name.to_string(),
            status: WorkflowStatus::Pending,
            started_at: None,
            finished_at: None,
        }
    }
}
```

**Step 2: 运行 clippy 检查**

Run: `cargo clippy --package claw -- -D warnings`
Expected: 无警告

**Step 3: Commit**

```bash
git add crates/claw/src/workflow/types.rs
git commit -m "feat(workflow): add WorkflowRecord, StageRecord, JobRecord types"
```

---

## Task 5: 创建 Repository Trait

**Files:**
- Create: `crates/claw/src/workflow/repository.rs`

**Step 1: 创建 repository.rs 文件**

```rust
//! crates/claw/src/workflow/repository.rs

use async_trait::async_trait;

use crate::db::DbError;
use super::types::{JobId, JobRecord, StageId, StageRecord, WorkflowId, WorkflowRecord, WorkflowStatus};

/// Repository for workflow persistence.
#[async_trait]
pub trait WorkflowRepository: Send + Sync {
    // Workflow CRUD

    /// Create a new workflow.
    async fn create_workflow(&self, workflow: &WorkflowRecord) -> Result<(), DbError>;

    /// Get a workflow by ID.
    async fn get_workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowRecord>, DbError>;

    /// Update workflow status.
    async fn update_workflow_status(&self, id: &WorkflowId, status: WorkflowStatus) -> Result<(), DbError>;

    /// List all workflows.
    async fn list_workflows(&self) -> Result<Vec<WorkflowRecord>, DbError>;

    /// Delete a workflow. Returns true if a row was deleted.
    async fn delete_workflow(&self, id: &WorkflowId) -> Result<bool, DbError>;

    // Stage CRUD

    /// Create a new stage.
    async fn create_stage(&self, stage: &StageRecord) -> Result<(), DbError>;

    /// List all stages for a workflow, ordered by sequence.
    async fn list_stages_by_workflow(&self, workflow_id: &WorkflowId) -> Result<Vec<StageRecord>, DbError>;

    /// Update stage status.
    async fn update_stage_status(&self, id: &StageId, status: WorkflowStatus) -> Result<(), DbError>;

    // Job CRUD

    /// Create a new job.
    async fn create_job(&self, job: &JobRecord) -> Result<(), DbError>;

    /// List all jobs for a stage.
    async fn list_jobs_by_stage(&self, stage_id: &StageId) -> Result<Vec<JobRecord>, DbError>;

    /// Update job status with optional timestamps.
    async fn update_job_status(
        &self,
        id: &JobId,
        status: WorkflowStatus,
        started_at: Option<&str>,
        finished_at: Option<&str>,
    ) -> Result<(), DbError>;
}
```

**Step 2: 运行 clippy 检查**

Run: `cargo clippy --package claw -- -D warnings`
Expected: 无警告

**Step 3: Commit**

```bash
git add crates/claw/src/workflow/repository.rs
git commit -m "feat(workflow): add WorkflowRepository trait"
```

---

## Task 6: 创建 workflow 模块入口

**Files:**
- Create: `crates/claw/src/workflow/mod.rs`

**Step 1: 创建 mod.rs**

```rust
//! crates/claw/src/workflow/mod.rs

mod repository;
mod types;

pub use repository::WorkflowRepository;
pub use types::{JobId, JobRecord, StageId, StageRecord, WorkflowId, WorkflowRecord, WorkflowStatus};
```

**Step 2: 修改 lib.rs 添加模块导出**

在 `crates/claw/src/lib.rs` 末尾添加：

```rust
pub mod workflow;
```

**Step 3: 运行 clippy 检查**

Run: `cargo clippy --package claw -- -D warnings`
Expected: 无警告

**Step 4: Commit**

```bash
git add crates/claw/src/workflow/mod.rs crates/claw/src/lib.rs
git commit -m "feat(workflow): add workflow module entry point"
```

---

## Task 7: 创建 SQLite 实现 - WorkflowRepository 结构体

**Files:**
- Create: `crates/claw/src/db/sqlite/workflow.rs`

**Step 1: 创建基本结构和辅助方法**

```rust
//! crates/claw/src/db/sqlite/workflow.rs

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::agents::AgentId;
use crate::db::DbError;
use crate::workflow::{
    JobId, JobRecord, StageId, StageRecord, WorkflowId, WorkflowRecord, WorkflowStatus,
};

pub struct SqliteWorkflowRepository {
    pool: SqlitePool,
}

impl SqliteWorkflowRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn get<T>(row: &sqlx::sqlite::SqliteRow, col: &str) -> Result<T, DbError>
    where
        T: for<'r> sqlx::decode::Decode<'r, sqlx::Sqlite> + sqlx::types::Type<sqlx::Sqlite>,
    {
        row.try_get(col).map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })
    }

    fn parse_status(s: String) -> Result<WorkflowStatus, DbError> {
        WorkflowStatus::from_str(&s).map_err(|_| DbError::QueryFailed {
            reason: format!("invalid workflow status: {s}"),
        })
    }

    fn map_workflow(row: sqlx::sqlite::SqliteRow) -> Result<WorkflowRecord, DbError> {
        let status_str: String = Self::get(&row, "status")?;
        Ok(WorkflowRecord {
            id: WorkflowId::new(Self::get(&row, "id")?),
            name: Self::get(&row, "name")?,
            status: Self::parse_status(status_str)?,
        })
    }

    fn map_stage(row: sqlx::sqlite::SqliteRow) -> Result<StageRecord, DbError> {
        let status_str: String = Self::get(&row, "status")?;
        Ok(StageRecord {
            id: StageId::new(Self::get(&row, "id")?),
            workflow_id: WorkflowId::new(Self::get(&row, "workflow_id")?),
            name: Self::get(&row, "name")?,
            sequence: Self::get::<i32, _>(&row, "sequence")?,
            status: Self::parse_status(status_str)?,
        })
    }

    fn map_job(row: sqlx::sqlite::SqliteRow) -> Result<JobRecord, DbError> {
        let status_str: String = Self::get(&row, "status")?;
        Ok(JobRecord {
            id: JobId::new(Self::get(&row, "id")?),
            stage_id: StageId::new(Self::get(&row, "stage_id")?),
            agent_id: AgentId::new(Self::get(&row, "agent_id")?),
            name: Self::get(&row, "name")?,
            status: Self::parse_status(status_str)?,
            started_at: Self::get::<Option<String>, _>(&row, "started_at")?,
            finished_at: Self::get::<Option<String>, _>(&row, "finished_at")?,
        })
    }

    fn status_as_str(status: WorkflowStatus) -> &'static str {
        status.as_str()
    }
}
```

**Step 2: 运行 clippy 检查**

Run: `cargo clippy --package claw -- -D warnings`
Expected: 无警告

**Step 3: Commit**

```bash
git add crates/claw/src/db/sqlite/workflow.rs
git commit -m "feat(workflow): add SqliteWorkflowRepository struct with helper methods"
```

---

## Task 8: 实现 Workflow CRUD 方法

**Files:**
- Modify: `crates/claw/src/db/sqlite/workflow.rs`

**Step 1: 实现 Workflow CRUD**

在 `SqliteWorkflowRepository` impl 块中，`status_as_str` 方法之后添加：

```rust
    async fn create_workflow(&self, workflow: &WorkflowRecord) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO workflows (id, name, status)
             VALUES (?1, ?2, ?3)",
        )
        .bind(workflow.id.as_ref())
        .bind(&workflow.name)
        .bind(Self::status_as_str(workflow.status))
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(())
    }

    async fn get_workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowRecord>, DbError> {
        let row = sqlx::query(
            "SELECT id, name, status FROM workflows WHERE id = ?1",
        )
        .bind(id.as_ref())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        row.map(Self::map_workflow).transpose()
    }

    async fn update_workflow_status(&self, id: &WorkflowId, status: WorkflowStatus) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE workflows SET status = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
        )
        .bind(Self::status_as_str(status))
        .bind(id.as_ref())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(())
    }

    async fn list_workflows(&self) -> Result<Vec<WorkflowRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT id, name, status FROM workflows ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        rows.into_iter().map(Self::map_workflow).collect()
    }

    async fn delete_workflow(&self, id: &WorkflowId) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM workflows WHERE id = ?1")
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(result.rows_affected() > 0)
    }
```

**注意：** 需要添加 `#[async_trait]` 到 impl 块。将整个 impl 块改为：

```rust
#[async_trait]
impl SqliteWorkflowRepository {
    // ... 所有方法
}
```

不对，应该将 trait 实现单独的 impl 块。修正如下：

保留当前的结构体 impl 块（包含 `new`, 辅助方法），在文件末尾添加新的 impl 块：

```rust
#[async_trait]
impl crate::workflow::WorkflowRepository for SqliteWorkflowRepository {
    // ... trait 方法实现
}
```

**Step 2: 修正结构 - 将 CRUD 方法移到 trait 实现**

删除刚才添加在 struct impl 中的方法，改为在文件末尾添加：

```rust
#[async_trait]
impl crate::workflow::WorkflowRepository for SqliteWorkflowRepository {
    async fn create_workflow(&self, workflow: &WorkflowRecord) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO workflows (id, name, status)
             VALUES (?1, ?2, ?3)",
        )
        .bind(workflow.id.as_ref())
        .bind(&workflow.name)
        .bind(Self::status_as_str(workflow.status))
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(())
    }

    async fn get_workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowRecord>, DbError> {
        let row = sqlx::query(
            "SELECT id, name, status FROM workflows WHERE id = ?1",
        )
        .bind(id.as_ref())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        row.map(Self::map_workflow).transpose()
    }

    async fn update_workflow_status(&self, id: &WorkflowId, status: WorkflowStatus) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE workflows SET status = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
        )
        .bind(Self::status_as_str(status))
        .bind(id.as_ref())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(())
    }

    async fn list_workflows(&self) -> Result<Vec<WorkflowRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT id, name, status FROM workflows ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        rows.into_iter().map(Self::map_workflow).collect()
    }

    async fn delete_workflow(&self, id: &WorkflowId) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM workflows WHERE id = ?1")
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(result.rows_affected() > 0)
    }
```

**Step 3: 运行 clippy 检查**

Run: `cargo clippy --package claw -- -D warnings`
Expected: 无警告

**Step 4: Commit**

```bash
git add crates/claw/src/db/sqlite/workflow.rs
git commit -m "feat(workflow): implement Workflow CRUD in SqliteWorkflowRepository"
```

---

## Task 9: 实现 Stage CRUD 方法

**Files:**
- Modify: `crates/claw/src/db/sqlite/workflow.rs`

**Step 1: 在 trait impl 块中添加 Stage 方法**

在 `delete_workflow` 方法之后添加：

```rust
    async fn create_stage(&self, stage: &StageRecord) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO stages (id, workflow_id, name, sequence, status)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(stage.id.as_ref())
        .bind(stage.workflow_id.as_ref())
        .bind(&stage.name)
        .bind(stage.sequence)
        .bind(Self::status_as_str(stage.status))
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(())
    }

    async fn list_stages_by_workflow(&self, workflow_id: &WorkflowId) -> Result<Vec<StageRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT id, workflow_id, name, sequence, status FROM stages
             WHERE workflow_id = ?1 ORDER BY sequence ASC",
        )
        .bind(workflow_id.as_ref())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        rows.into_iter().map(Self::map_stage).collect()
    }

    async fn update_stage_status(&self, id: &StageId, status: WorkflowStatus) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE stages SET status = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
        )
        .bind(Self::status_as_str(status))
        .bind(id.as_ref())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(())
    }
```

**Step 2: 运行 clippy 检查**

Run: `cargo clippy --package claw -- -D warnings`
Expected: 无警告

**Step 3: Commit**

```bash
git add crates/claw/src/db/sqlite/workflow.rs
git commit -m "feat(workflow): implement Stage CRUD in SqliteWorkflowRepository"
```

---

## Task 10: 实现 Job CRUD 方法

**Files:**
- Modify: `crates/claw/src/db/sqlite/workflow.rs`

**Step 1: 在 trait impl 块中添加 Job 方法**

在 `update_stage_status` 方法之后添加：

```rust
    async fn create_job(&self, job: &JobRecord) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO jobs (id, stage_id, agent_id, name, status, started_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(job.id.as_ref())
        .bind(job.stage_id.as_ref())
        .bind(job.agent_id.as_ref())
        .bind(&job.name)
        .bind(Self::status_as_str(job.status))
        .bind(&job.started_at)
        .bind(&job.finished_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(())
    }

    async fn list_jobs_by_stage(&self, stage_id: &StageId) -> Result<Vec<JobRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT id, stage_id, agent_id, name, status, started_at, finished_at FROM jobs
             WHERE stage_id = ?1 ORDER BY created_at ASC",
        )
        .bind(stage_id.as_ref())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        rows.into_iter().map(Self::map_job).collect()
    }

    async fn update_job_status(
        &self,
        id: &JobId,
        status: WorkflowStatus,
        started_at: Option<&str>,
        finished_at: Option<&str>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE jobs SET status = ?1, started_at = COALESCE(?2, started_at),
             finished_at = COALESCE(?3, finished_at), updated_at = CURRENT_TIMESTAMP
             WHERE id = ?4",
        )
        .bind(Self::status_as_str(status))
        .bind(started_at)
        .bind(finished_at)
        .bind(id.as_ref())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(())
    }
```

**Step 2: 运行 clippy 检查**

Run: `cargo clippy --package claw -- -D warnings`
Expected: 无警告

**Step 3: Commit**

```bash
git add crates/claw/src/db/sqlite/workflow.rs
git commit -m "feat(workflow): implement Job CRUD in SqliteWorkflowRepository"
```

---

## Task 11: 导出 SqliteWorkflowRepository

**Files:**
- Modify: `crates/claw/src/db/sqlite/mod.rs`
- Modify: `crates/claw/src/db/mod.rs`

**Step 1: 修改 sqlite/mod.rs**

添加 `mod workflow;` 并导出：

```rust
mod agent;
#[cfg(feature = "dev")]
mod approval;
mod llm;
mod workflow;  // 添加这一行

use std::path::Path;
// ... 其余代码不变

pub use agent::SqliteAgentRepository;
#[cfg(feature = "dev")]
pub use approval::SqliteApprovalRepository;
pub use llm::SqliteLlmProviderRepository;
pub use workflow::SqliteWorkflowRepository;  // 添加这一行
```

**Step 2: 修改 db/mod.rs**

在 `pub use approval::ApprovalRepository;` 之后添加：

```rust
#[cfg(feature = "dev")]
pub use approval::ApprovalRepository;

// 添加 workflow 模块
pub mod workflow;

pub use workflow::WorkflowRepository;
```

等等，检查一下 `workflow.rs` 需要放在哪里。查看代码结构，`workflow.rs` 应该在 `crates/claw/src/db/` 目录下，类似于 `approval.rs`。

**修正：** 将 `crates/claw/src/db/sqlite/workflow.rs` 移动到 `crates/claw/src/db/workflow.rs`。

重新执行 **Task 7-10**，但将文件创建在 `crates/claw/src/db/workflow.rs` 而不是 `crates/claw/src/db/sqlite/workflow.rs`。

**Step 3: 在 db/mod.rs 中添加**

```rust
pub mod llm;
pub mod sqlite;
pub mod workflow;  // 添加这一行

#[cfg(feature = "dev")]
pub use approval::ApprovalRepository;
pub use workflow::WorkflowRepository;  // 添加这一行
```

**Step 4: 运行 clippy 检查**

Run: `cargo clippy --package claw -- -D warnings`
Expected: 无警告

**Step 5: Commit**

```bash
git add crates/claw/src/db/mod.rs
git commit -m "feat(workflow): export WorkflowRepository from db module"
```

---

## Task 12: 添加 SqliteWorkflowRepository 的单元测试

**Files:**
- Modify: `crates/claw/src/db/workflow.rs`

**Step 1: 添加测试模块**

在文件末尾添加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::sqlite::{connect, migrate};

    async fn create_test_pool() -> SqlitePool {
        let pool = connect("sqlite::memory:").await.expect("failed to connect");
        migrate(&pool).await.expect("failed to migrate");
        pool
    }

    #[tokio::test]
    async fn create_and_get_workflow() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        let workflow = WorkflowRecord::for_test("wf-1", "test-workflow");
        repo.create_workflow(&workflow).await.unwrap();

        let retrieved = repo.get_workflow(&workflow.id).await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, workflow.id);
        assert_eq!(retrieved.name, "test-workflow");
        assert_eq!(retrieved.status, WorkflowStatus::Pending);
    }

    #[tokio::test]
    async fn list_workflows() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        repo.create_workflow(&WorkflowRecord::for_test("wf-1", "workflow-a")).await.unwrap();
        repo.create_workflow(&WorkflowRecord::for_test("wf-2", "workflow-b")).await.unwrap();

        let workflows = repo.list_workflows().await.unwrap();
        assert_eq!(workflows.len(), 2);
    }

    #[tokio::test]
    async fn update_workflow_status() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        let workflow = WorkflowRecord::for_test("wf-1", "test");
        repo.create_workflow(&workflow).await.unwrap();

        repo.update_workflow_status(&workflow.id, WorkflowStatus::Running).await.unwrap();

        let retrieved = repo.get_workflow(&workflow.id).await.unwrap().unwrap();
        assert_eq!(retrieved.status, WorkflowStatus::Running);
    }

    #[tokio::test]
    async fn delete_workflow() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        let workflow = WorkflowRecord::for_test("wf-1", "test");
        repo.create_workflow(&workflow).await.unwrap();

        let deleted = repo.delete_workflow(&workflow.id).await.unwrap();
        assert!(deleted);

        let retrieved = repo.get_workflow(&workflow.id).await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn create_and_list_stages() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        let workflow = WorkflowRecord::for_test("wf-1", "test");
        repo.create_workflow(&workflow).await.unwrap();

        repo.create_stage(&StageRecord::for_test("stage-1", "wf-1", "fetch", 0)).await.unwrap();
        repo.create_stage(&StageRecord::for_test("stage-2", "wf-1", "process", 1)).await.unwrap();

        let stages = repo.list_stages_by_workflow(&workflow.id).await.unwrap();
        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].sequence, 0);
        assert_eq!(stages[1].sequence, 1);
    }

    #[tokio::test]
    async fn create_and_list_jobs() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        let workflow = WorkflowRecord::for_test("wf-1", "test");
        repo.create_workflow(&workflow).await.unwrap();

        let stage = StageRecord::for_test("stage-1", "wf-1", "fetch", 0);
        repo.create_stage(&stage).await.unwrap();

        repo.create_job(&JobRecord::for_test("job-1", "stage-1", "agent-1", "fetch-a")).await.unwrap();
        repo.create_job(&JobRecord::for_test("job-2", "stage-1", "agent-2", "fetch-b")).await.unwrap();

        let jobs = repo.list_jobs_by_stage(&stage.id).await.unwrap();
        assert_eq!(jobs.len(), 2);
    }

    #[tokio::test]
    async fn update_job_status_with_timestamps() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        let workflow = WorkflowRecord::for_test("wf-1", "test");
        repo.create_workflow(&workflow).await.unwrap();

        let stage = StageRecord::for_test("stage-1", "wf-1", "fetch", 0);
        repo.create_stage(&stage).await.unwrap();

        let job = JobRecord::for_test("job-1", "stage-1", "agent-1", "fetch");
        repo.create_job(&job).await.unwrap();

        let started = "2025-03-11T10:00:00Z";
        repo.update_job_status(&job.id, WorkflowStatus::Running, Some(started), None).await.unwrap();

        let jobs = repo.list_jobs_by_stage(&stage.id).await.unwrap();
        assert_eq!(jobs[0].status, WorkflowStatus::Running);
        assert_eq!(jobs[0].started_at.as_deref(), Some(started));
    }
}
```

**Step 2: 运行测试**

Run: `cargo test --package claw -- --nocapture workflow`
Expected: 所有测试通过

**Step 3: Commit**

```bash
git add crates/claw/src/db/workflow.rs
git commit -m "test(workflow): add unit tests for SqliteWorkflowRepository"
```

---

## Task 13: 创建 Dev CLI - WorkflowCommand 枚举

**Files:**
- Modify: `crates/cli/src/dev.rs`

**Step 1: 添加 WorkflowCommand 枚举**

在 `DevCommand` 枚举中添加 `Workflow` 变体。在 `Approval(ApprovalCommand),` 之后添加：

```rust
    #[command(subcommand)]
    Workflow(WorkflowCommand),
```

在同一文件中，`ApprovalCommand` 定义之后添加：

```rust
/// Workflow commands for testing workflow execution.
#[derive(Debug, Subcommand)]
pub enum WorkflowCommand {
    /// Create a new workflow.
    Create {
        /// Workflow name.
        name: String,
    },

    /// List all workflows.
    List,

    /// Show workflow details with stages and jobs.
    Show {
        /// Workflow ID.
        id: String,
    },

    /// Delete a workflow.
    Delete {
        /// Workflow ID.
        id: String,
    },

    /// Add a stage to a workflow.
    AddStage {
        /// Workflow ID.
        #[arg(long)]
        workflow: String,
        /// Stage name.
        name: String,
        /// Stage sequence (order).
        sequence: i32,
    },

    /// Add a job to a stage.
    AddJob {
        /// Stage ID.
        #[arg(long)]
        stage: String,
        /// Agent ID.
        #[arg(long)]
        agent: String,
        /// Job name.
        name: String,
    },

    /// Update job status.
    JobStatus {
        /// Job ID.
        #[arg(long)]
        id: String,
        /// New status.
        status: String,
    },

    /// Show workflow status tree.
    Status {
        /// Workflow ID.
        id: String,
    },
}
```

**Step 2: 修改 try_run 函数**

在 `matches!(first_arg.as_str(), "provider" | "llm" | "turn" | "approval")` 改为：

```rust
    if !matches!(first_arg.as_str(), "provider" | "llm" | "turn" | "approval" | "workflow") {
        return Ok(false);
    }
```

**Step 3: 修改 run 函数**

在 `DevCommand::Approval(command) => run_approval_command(ctx, command).await,` 之后添加：

```rust
        DevCommand::Workflow(command) => run_workflow_command(ctx, command).await,
```

**Step 4: 运行 clippy 检查**

Run: `cargo clippy --package cli -- -D warnings`
Expected: 有 `run_workflow_command` 未定义的警告（预期，下一步实现）

**Step 5: Commit**

```bash
git add crates/cli/src/dev.rs
git commit -m "feat(cli): add WorkflowCommand enum for dev CLI"
```

---

## Task 14: 实现 Workflow CLI 命令处理函数

**文件:**
- Modify: `crates/cli/src/dev.rs`

**Step 1: 添加必要的导入**

在文件顶部的导入区域添加：

```rust
use claw::workflow::{
    JobId, JobRecord, StageId, StageRecord, WorkflowId, WorkflowRecord, WorkflowStatus,
    WorkflowRepository,
};
use claw::db::sqlite::SqliteWorkflowRepository;
```

**Step 2: 在 try_run 函数之前添加 workflow 数据库辅助函数**

在 `try_run` 函数之前添加：

```rust
// ---------------------------------------------------------------------------
// Workflow SQLite helpers
// ---------------------------------------------------------------------------

fn resolve_workflow_dev_database_url(
    explicit_database_url: Option<&str>,
    cwd: Option<&Path>,
) -> Result<String> {
    if let Some(database_url) = explicit_database_url.filter(|value| !value.trim().is_empty()) {
        return Ok(database_url.to_string());
    }

    let cwd = match cwd {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir().context("failed to resolve current working directory")?,
    };
    let tmp_dir = cwd.join("tmp");
    std::fs::create_dir_all(&tmp_dir).with_context(|| {
        format!(
            "failed to create dev workflow tmp directory at {}",
            tmp_dir.display()
        )
    })?;

    let db_path = tmp_dir.join("workflow-dev.sqlite");
    Ok(format!("sqlite:{}", db_path.display()))
}

async fn create_dev_workflow_repository() -> Result<(SqliteWorkflowRepository, String)> {
    let env_database_url = std::env::var("WORKFLOW_DATABASE_URL").ok();
    let database_url = resolve_workflow_dev_database_url(env_database_url.as_deref(), None)?;
    let pool = claw::db::sqlite::connect(&database_url)
        .await
        .with_context(|| {
            format!(
                "failed to connect workflow dev database at `{}`",
                database_url
            )
        })?;

    // Run main migrations (includes workflow tables)
    claw::db::sqlite::migrate(&pool).await.with_context(|| {
        format!(
            "failed to run migrations for workflow dev database at `{}`",
            database_url
        )
    })?;

    Ok((SqliteWorkflowRepository::new(pool), database_url))
}
```

**Step 3: 添加 run_workflow_command 函数**

在 `run_approval_command` 函数之后添加：

```rust
/// Run a workflow command.
async fn run_workflow_command(_ctx: AppContext, command: WorkflowCommand) -> Result<()> {
    use uuid::Uuid;

    let (repo, database_url) = create_dev_workflow_repository().await;

    match command {
        WorkflowCommand::Create { name } => {
            let id = WorkflowId::new(format!("wf-{}", Uuid::new_v4()));
            let workflow = WorkflowRecord {
                id: id.clone(),
                name: name.clone(),
                status: WorkflowStatus::Pending,
            };

            repo.create_workflow(&workflow).await?;

            println!("Workflow created:");
            println!("  ID:     {id}");
            println!("  Name:   {name}");
            println!("  Status: pending");
            println!();
            println!("Storage: {database_url}");
        }

        WorkflowCommand::List => {
            let workflows = repo.list_workflows().await?;
            if workflows.is_empty() {
                println!("No workflows found.");
            } else {
                println!("Workflows ({}):", workflows.len());
                for wf in workflows {
                    println!();
                    println!("  ID:     {}", wf.id);
                    println!("  Name:   {}", wf.name);
                    println!("  Status: {}", wf.status);
                }
            }
        }

        WorkflowCommand::Show { id } => {
            let workflow_id = WorkflowId::new(&id);
            let Some(workflow) = repo.get_workflow(&workflow_id).await? else {
                println!("Workflow not found: {id}");
                return Ok(());
            };

            println!("Workflow: {}", workflow.name);
            println!("ID:     {}", workflow.id);
            println!("Status: {}", workflow.status);
            println!();

            let stages = repo.list_stages_by_workflow(&workflow_id).await?;
            if stages.is_empty() {
                println!("No stages.");
            } else {
                for stage in stages {
                    println!("Stage [{}]: {}", stage.sequence, stage.name);
                    println!("  ID:     {}", stage.id);
                    println!("  Status: {}", stage.status);
                    println!();

                    let jobs = repo.list_jobs_by_stage(&stage.id).await?;
                    if jobs.is_empty() {
                        println!("  No jobs.");
                    } else {
                        for job in jobs {
                            println!("  Job: {}", job.name);
                            println!("    ID:      {}", job.id);
                            println!("    Agent:   {}", job.agent_id);
                            println!("    Status:  {}", job.status);
                            println!();
                        }
                    }
                }
            }
        }

        WorkflowCommand::Delete { id } => {
            let workflow_id = WorkflowId::new(&id);
            let deleted = repo.delete_workflow(&workflow_id).await?;
            if deleted {
                println!("Workflow deleted: {id}");
            } else {
                println!("Workflow not found: {id}");
            }
        }

        WorkflowCommand::AddStage { workflow, name, sequence } => {
            let workflow_id = WorkflowId::new(&workflow);
            let stage_id = StageId::new(format!("stage-{}", Uuid::new_v4()));
            let stage = StageRecord {
                id: stage_id.clone(),
                workflow_id: workflow_id.clone(),
                name,
                sequence,
                status: WorkflowStatus::Pending,
            };

            repo.create_stage(&stage).await?;

            println!("Stage created:");
            println!("  ID:       {stage_id}");
            println!("  Workflow: {workflow}");
            println!("  Name:     {}", stage.name);
            println!("  Sequence: {sequence}");
        }

        WorkflowCommand::AddJob { stage, agent, name } => {
            let stage_id = StageId::new(&stage);
            let job_id = JobId::new(format!("job-{}", Uuid::new_v4()));
            let job = JobRecord {
                id: job_id.clone(),
                stage_id,
                agent_id: crate::agents::AgentId::new(&agent),
                name: name.clone(),
                status: WorkflowStatus::Pending,
                started_at: None,
                finished_at: None,
            };

            repo.create_job(&job).await?;

            println!("Job created:");
            println!("  ID:    {job_id}");
            println!("  Stage: {stage}");
            println!("  Agent: {agent}");
            println!("  Name:  {name}");
        }

        WorkflowCommand::JobStatus { id, status } => {
            let job_id = JobId::new(&id);
            let new_status = WorkflowStatus::from_str(&status)
                .map_err(|e| anyhow!("invalid status: {e}"))?;

            repo.update_job_status(&job_id, new_status, None, None).await?;

            println!("Job {id} status updated to: {status}");
        }

        WorkflowCommand::Status { id } => {
            let workflow_id = WorkflowId::new(&id);
            let Some(workflow) = repo.get_workflow(&workflow_id).await? else {
                println!("Workflow not found: {id}");
                return Ok(());
            };

            println!("Workflow: {} ({})", workflow.name, workflow.status);
            println!();

            let stages = repo.list_stages_by_workflow(&workflow_id).await?;
            for stage in stages {
                let connector = match stage.status {
                    WorkflowStatus::Succeeded => "├",
                    WorkflowStatus::Running => "├",
                    WorkflowStatus::Failed => "├",
                    WorkflowStatus::Pending => "├",
                    WorkflowStatus::Cancelled => "├",
                };
                let prefix = if stages.iter().position(|s| s.id == stage.id) == Some(stages.len() - 1) {
                    "└"
                } else {
                    "├"
                };

                println!("{prefix}─ Stage [{}]: {} ({})", stage.sequence, stage.name, stage.status);

                let jobs = repo.list_jobs_by_stage(&stage.id).await?;
                for job in jobs {
                    let is_last = jobs.iter().position(|j| j.id == job.id) == Some(jobs.len() - 1);
                    let job_prefix = if is_last { "└" } else { "│" };
                    println!("  {job_prefix}── Job: {} (agent: {}) [{}]", job.name, job.agent_id, job.status);
                }
            }
        }
    }

    Ok(())
}
```

**Step 4: 运行 clippy 检查**

Run: `cargo clippy --package cli -- -D warnings`
Expected: 无警告

**Step 5: Commit**

```bash
git add crates/cli/src/dev.rs
git commit -m "feat(cli): implement workflow CLI commands"
```

---

## Task 15: 添加 Workflow CLI 测试

**文件:**
- Modify: `crates/cli/src/dev.rs`

**Step 1: 在 tests 模块添加测试**

在 `#[cfg(test)] mod tests` 块中，`approval_database_url_defaults_to_tmp_under_current_directory` 测试之后添加：

```rust
    #[test]
    fn parses_workflow_create_command() {
        let cli = DevCli::parse_from(["cli", "workflow", "create", "test-workflow"]);

        match cli.command {
            DevCommand::Workflow(WorkflowCommand::Create { name }) => {
                assert_eq!(name, "test-workflow");
            }
            _ => panic!("workflow create command should parse"),
        }
    }

    #[test]
    fn parses_workflow_add_stage_command() {
        let cli = DevCli::parse_from([
            "cli",
            "workflow",
            "add-stage",
            "--workflow",
            "wf-123",
            "fetch",
            "0",
        ]);

        match cli.command {
            DevCommand::Workflow(WorkflowCommand::AddStage {
                workflow,
                name,
                sequence,
            }) => {
                assert_eq!(workflow, "wf-123");
                assert_eq!(name, "fetch");
                assert_eq!(sequence, 0);
            }
            _ => panic!("workflow add-stage command should parse"),
        }
    }

    #[test]
    fn parses_workflow_add_job_command() {
        let cli = DevCli::parse_from([
            "cli",
            "workflow",
            "add-job",
            "--stage",
            "stage-1",
            "--agent",
            "agent-1",
            "job-name",
        ]);

        match cli.command {
            DevCommand::Workflow(WorkflowCommand::AddJob {
                stage,
                agent,
                name,
            }) => {
                assert_eq!(stage, "stage-1");
                assert_eq!(agent, "agent-1");
                assert_eq!(name, "job-name");
            }
            _ => panic!("workflow add-job command should parse"),
        }
    }

    #[test]
    fn parses_workflow_status_command() {
        let cli = DevCli::parse_from(["cli", "workflow", "status", "wf-001"]);

        match cli.command {
            DevCommand::Workflow(WorkflowCommand::Status { id }) => {
                assert_eq!(id, "wf-001");
            }
            _ => panic!("workflow status command should parse"),
        }
    }

    #[test]
    fn parses_workflow_job_status_command() {
        let cli = DevCli::parse_from([
            "cli",
            "workflow",
            "job-status",
            "--id",
            "job-1",
            "--status",
            "running",
        ]);

        match cli.command {
            DevCommand::Workflow(WorkflowCommand::JobStatus { id, status }) => {
                assert_eq!(id, "job-1");
                assert_eq!(status, "running");
            }
            _ => panic!("workflow job-status command should parse"),
        }
    }

    #[test]
    fn workflow_database_url_prefers_explicit_override() {
        let resolved =
            resolve_workflow_dev_database_url(Some("sqlite:./custom.db"), None).expect("db url should resolve");
        assert_eq!(resolved, "sqlite:./custom.db");
    }
```

**Step 2: 运行测试**

Run: `cargo test --package cli -- dev::tests::parses_workflow -- --nocapture`
Expected: 所有 6 个测试通过

**Step 3: Commit**

```bash
git add crates/cli/src/dev.rs
git commit -m "test(cli): add workflow CLI command tests"
```

---

## Task 16: 创建集成测试

**文件:**
- Create: `crates/claw/tests/workflow_integration_test.rs`

**Step 1: 创建集成测试文件**

```rust
//! crates/claw/tests/workflow_integration_test.rs

use claw::agents::{AgentId, AgentRepository};
use claw::db::sqlite::{connect, migrate, SqliteAgentRepository};
use claw::workflow::{
    JobId, JobRecord, StageId, StageRecord, WorkflowId, WorkflowRecord, WorkflowRepository,
    WorkflowStatus,
};
use uuid::Uuid;

async fn create_test_pool() -> sqlx::SqlitePool {
    let pool = connect("sqlite::memory:").await.expect("failed to connect");
    migrate(&pool).await.expect("failed to migrate");
    pool
}

#[tokio::test]
async fn full_workflow_lifecycle() {
    let pool = create_test_pool().await;
    let repo = claw::db::workflow::SqliteWorkflowRepository::new(pool.clone());
    let agent_repo = SqliteAgentRepository::new(pool);

    // Create an agent first
    let agent_id = AgentId::new("test-agent");
    let agent = claw::agents::AgentRecord::for_test(&agent_id, "provider-1");
    agent_repo.upsert(&agent).await.unwrap();

    // Create workflow
    let workflow_id = WorkflowId::new(format!("wf-{}", Uuid::new_v4()));
    let workflow = WorkflowRecord {
        id: workflow_id.clone(),
        name: "integration-test".to_string(),
        status: WorkflowStatus::Pending,
    };
    repo.create_workflow(&workflow).await.unwrap();

    // Create stages
    let stage1_id = StageId::new(format!("stage-{}", Uuid::new_v4()));
    let stage1 = StageRecord {
        id: stage1_id.clone(),
        workflow_id: workflow_id.clone(),
        name: "fetch".to_string(),
        sequence: 0,
        status: WorkflowStatus::Pending,
    };
    repo.create_stage(&stage1).await.unwrap();

    let stage2_id = StageId::new(format!("stage-{}", Uuid::new_v4()));
    let stage2 = StageRecord {
        id: stage2_id.clone(),
        workflow_id: workflow_id.clone(),
        name: "process".to_string(),
        sequence: 1,
        status: WorkflowStatus::Pending,
    };
    repo.create_stage(&stage2).await.unwrap();

    // Create jobs
    let job1_id = JobId::new(format!("job-{}", Uuid::new_v4()));
    let job1 = JobRecord {
        id: job1_id.clone(),
        stage_id: stage1_id.clone(),
        agent_id: agent_id.clone(),
        name: "fetch-a".to_string(),
        status: WorkflowStatus::Pending,
        started_at: None,
        finished_at: None,
    };
    repo.create_job(&job1).await.unwrap();

    let job2_id = JobId::new(format!("job-{}", Uuid::new_v4()));
    let job2 = JobRecord {
        id: job2_id.clone(),
        stage_id: stage1_id.clone(),
        agent_id: agent_id.clone(),
        name: "fetch-b".to_string(),
        status: WorkflowStatus::Pending,
        started_at: None,
        finished_at: None,
    };
    repo.create_job(&job2).await.unwrap();

    // Verify stages are ordered by sequence
    let stages = repo.list_stages_by_workflow(&workflow_id).await.unwrap();
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0].sequence, 0);
    assert_eq!(stages[1].sequence, 1);

    // Update job status
    repo.update_job_status(&job1_id, WorkflowStatus::Running, Some("2025-03-11T10:00:00Z"), None)
        .await
        .unwrap();

    let jobs = repo.list_jobs_by_stage(&stage1_id).await.unwrap();
    assert_eq!(jobs[0].status, WorkflowStatus::Running);
    assert_eq!(jobs[0].started_at.as_deref(), Some("2025-03-11T10:00:00Z"));

    // Verify cascade delete
    repo.delete_workflow(&workflow_id).await.unwrap();

    let workflow = repo.get_workflow(&workflow_id).await.unwrap();
    assert!(workflow.is_none());

    let stages = repo.list_stages_by_workflow(&workflow_id).await.unwrap();
    assert!(stages.is_empty());
}

#[tokio::test]
async fn foreign_key_constraint_agent() {
    let pool = create_test_pool().await;
    let repo = claw::db::workflow::SqliteWorkflowRepository::new(pool);

    // Create workflow and stage
    let workflow_id = WorkflowId::new(format!("wf-{}", Uuid::new_v4()));
    repo.create_workflow(&WorkflowRecord {
        id: workflow_id.clone(),
        name: "test".to_string(),
        status: WorkflowStatus::Pending,
    })
    .await
    .unwrap();

    let stage_id = StageId::new(format!("stage-{}", Uuid::new_v4()));
    repo.create_stage(&StageRecord {
        id: stage_id.clone(),
        workflow_id,
        name: "stage".to_string(),
        sequence: 0,
        status: WorkflowStatus::Pending,
    })
    .await
    .unwrap();

    // Try to create job with non-existent agent
    let job_id = JobId::new(format!("job-{}", Uuid::new_v4()));
    let result = repo
        .create_job(&JobRecord {
            id: job_id,
            stage_id,
            agent_id: AgentId::new("non-existent-agent"),
            name: "job".to_string(),
            status: WorkflowStatus::Pending,
            started_at: None,
            finished_at: None,
        })
        .await;

    // Should fail due to foreign key constraint
    assert!(result.is_err());
}
```

**Step 2: 创建 db/workflow.rs 的 SqliteWorkflowRepository 导出**

确保 `crates/claw/src/db/workflow.rs` 的结构体是 pub 的：

```rust
pub struct SqliteWorkflowRepository {
    // ...
}
```

并确保 `SqliteWorkflowRepository::new` 是 pub 的。

**Step 3: 运行集成测试**

Run: `cargo test --package claw --features integration --test workflow_integration_test -- --nocapture`
Expected: 所有测试通过

**Step 4: Commit**

```bash
git add crates/claw/tests/workflow_integration_test.rs
git commit -m "test(workflow): add integration tests for workflow lifecycle"
```

---

## Task 17: 运行完整测试套件

**Step 1: 运行所有测试**

Run: `cargo test --package claw --all-features`
Expected: 所有测试通过

Run: `cargo test --package cli --all-features`
Expected: 所有测试通过

**Step 2: 运行 clippy**

Run: `cargo clippy --all --benches --tests --examples --all-features`
Expected: 零警告

**Step 3: 运行格式化**

Run: `cargo fmt`
Expected: 无变化

**Step 4: Commit**

```bash
git add -A
git commit -m "chore: pass all tests and clippy checks for workflow module"
```

---

## Task 18: 手动测试 CLI

**Step 1: 创建测试工作流**

Run: `cargo run --features dev -- workflow create "test-pipeline"`
Expected: 输出 workflow ID 和状态

**Step 2: 添加 Stage**

Run: `cargo run --features dev -- workflow add-stage --workflow <id> fetch 0`
Expected: Stage 创建成功

Run: `cargo run --features dev -- workflow add-stage --workflow <id> process 1`
Expected: Stage 创建成功

**Step 3: 添加 Job**

Run: `cargo run --features dev -- workflow add-job --stage <stage_id> <agent_id> fetch-a`
Expected: Job 创建成功

**Step 4: 查看状态**

Run: `cargo run --features dev -- workflow status <workflow_id>`
Expected: 输出状态树

**Step 5: 清理**

Run: `cargo run --features dev -- workflow delete <workflow_id>`
Expected: Workflow 删除成功

---

## 实现完成检查清单

- [ ] 数据库迁移文件创建
- [ ] 领域类型创建（ID, Status, Records）
- [ ] Repository trait 定义
- [ ] SQLite 实现
- [ ] 单元测试通过
- [ ] 集成测试通过
- [ ] Dev CLI 命令实现
- [ ] CLI 测试通过
- [ ] clippy 零警告
- [ ] 手动测试成功

---

## 相关文件清单

| 文件 | 操作 |
|------|------|
| `crates/claw/migrations/20260311000001_create_workflows.sql` | 创建 |
| `crates/claw/src/workflow/types.rs` | 创建 |
| `crates/claw/src/workflow/repository.rs` | 创建 |
| `crates/claw/src/workflow/mod.rs` | 创建 |
| `crates/claw/src/db/workflow.rs` | 创建 |
| `crates/claw/src/db/mod.rs` | 修改 |
| `crates/claw/src/lib.rs` | 修改 |
| `crates/claw/tests/workflow_integration_test.rs` | 创建 |
| `crates/cli/src/dev.rs` | 修改 |
