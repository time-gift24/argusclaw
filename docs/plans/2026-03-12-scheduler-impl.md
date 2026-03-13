# Scheduler Runtime Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a scheduler runtime that polls for pending jobs, checks dependency readiness, and dispatches jobs to agents for execution.

**Architecture:** Generalize the existing workflow-specific Job into a universal Job model with three types (standalone, workflow, cron). A Scheduler runs as a tokio background task, polling the database at a fixed interval, checking dependency resolution, and dispatching ready jobs to Agent runtimes by creating new Threads.

**Tech Stack:** Rust, tokio (async runtime + CancellationToken via tokio-util), sqlx (SQLite), DashMap (concurrent map), cron (cron expression parsing)

**Design doc:** `docs/plans/2026-03-12-scheduler-design.md`

---

### Task 1: Add new dependencies to Cargo.toml

**Files:**
- Modify: `crates/claw/Cargo.toml`

**Step 1: Add tokio-util and cron dependencies**

In `crates/claw/Cargo.toml`, add to `[dependencies]`:

```toml
tokio-util = "0.7"
cron = "0.15"
```

Update existing tokio entry to include `sync` feature (needed for CancellationToken interop):

```toml
tokio = { version = "1", features = ["time", "macros", "rt", "sync"] }
```

**Step 2: Verify it compiles**

Run: `cd crates/claw && cargo check`
Expected: compiles with no errors

**Step 3: Commit**

```bash
git add crates/claw/Cargo.toml
git commit -m "chore: add tokio-util and cron dependencies for scheduler"
```

---

### Task 2: Database migration - new jobs table schema

**Files:**
- Create: `crates/claw/migrations/<next_timestamp>_generalize_jobs.sql`

**Step 1: Create migration file**

Run from `crates/claw`:
```bash
cd crates/claw && sqlx migrate add generalize_jobs
```

**Step 2: Write migration SQL**

```sql
-- Drop old stage-dependent tables and recreate jobs as universal
DROP TABLE IF EXISTS jobs;
DROP TABLE IF EXISTS stages;

CREATE TABLE IF NOT EXISTS jobs (
    id          TEXT PRIMARY KEY NOT NULL,
    job_type    TEXT NOT NULL DEFAULT 'standalone',
    name        TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending',

    agent_id    TEXT NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    context     TEXT,
    prompt      TEXT NOT NULL,
    thread_id   TEXT,

    group_id    TEXT,
    depends_on  TEXT NOT NULL DEFAULT '[]',

    cron_expr   TEXT,
    scheduled_at TEXT,

    started_at  TEXT,
    finished_at TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);
CREATE INDEX IF NOT EXISTS idx_jobs_group_id ON jobs(group_id);
CREATE INDEX IF NOT EXISTS idx_jobs_agent_id ON jobs(agent_id);
CREATE INDEX IF NOT EXISTS idx_jobs_scheduled_at ON jobs(scheduled_at);
CREATE INDEX IF NOT EXISTS idx_jobs_job_type ON jobs(job_type);
```

**Step 3: Verify migration compiles**

Run: `cargo check`
Expected: compiles (sqlx migrations are embedded at compile time)

**Step 4: Commit**

```bash
git add crates/claw/migrations/
git commit -m "migration: generalize jobs table, drop stages"
```

---

### Task 3: Job domain types module

**Files:**
- Create: `crates/claw/src/job/mod.rs`
- Create: `crates/claw/src/job/types.rs`
- Create: `crates/claw/src/job/error.rs`
- Create: `crates/claw/src/job/repository.rs`
- Modify: `crates/claw/src/lib.rs` (add `pub mod job;`)

**Step 1: Write tests for JobType in `types.rs`**

Create `crates/claw/src/job/types.rs`:

```rust
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::agents::AgentId;
use crate::agents::thread::ThreadId;
use crate::workflow::{JobId, WorkflowStatus};

/// The kind of job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobType {
    Standalone,
    Workflow,
    Cron,
}

impl JobType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::Workflow => "workflow",
            Self::Cron => "cron",
        }
    }

    pub fn parse_str(s: &str) -> Result<Self, String> {
        match s {
            "standalone" => Ok(Self::Standalone),
            "workflow" => Ok(Self::Workflow),
            "cron" => Ok(Self::Cron),
            _ => Err(format!("invalid job type: {s}")),
        }
    }
}

impl fmt::Display for JobType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Job record stored in database.
pub struct JobRecord {
    pub id: JobId,
    pub job_type: JobType,
    pub name: String,
    pub status: WorkflowStatus,
    pub agent_id: AgentId,
    pub context: Option<String>,
    pub prompt: String,
    pub thread_id: Option<ThreadId>,
    pub group_id: Option<String>,
    pub depends_on: Vec<JobId>,
    pub cron_expr: Option<String>,
    pub scheduled_at: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[cfg(test)]
impl JobRecord {
    #[must_use]
    pub fn for_test(id: &str, agent_id: &str, name: &str, prompt: &str) -> Self {
        Self {
            id: JobId::new(id),
            job_type: JobType::Standalone,
            name: name.to_string(),
            status: WorkflowStatus::Pending,
            agent_id: AgentId::new(agent_id),
            context: None,
            prompt: prompt.to_string(),
            thread_id: None,
            group_id: None,
            depends_on: vec![],
            cron_expr: None,
            scheduled_at: None,
            started_at: None,
            finished_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_type_roundtrip() {
        for jt in [JobType::Standalone, JobType::Workflow, JobType::Cron] {
            assert_eq!(JobType::parse_str(jt.as_str()).unwrap(), jt);
        }
    }

    #[test]
    fn job_type_invalid() {
        assert!(JobType::parse_str("unknown").is_err());
    }

    #[test]
    fn job_type_display() {
        assert_eq!(JobType::Standalone.to_string(), "standalone");
        assert_eq!(JobType::Workflow.to_string(), "workflow");
        assert_eq!(JobType::Cron.to_string(), "cron");
    }
}
```

**Step 2: Write `error.rs`**

Create `crates/claw/src/job/error.rs`:

```rust
use thiserror::Error;

use crate::db::DbError;

#[derive(Debug, Error)]
pub enum JobError {
    #[error(transparent)]
    Database(#[from] DbError),

    #[error("job `{id}` not found")]
    NotFound { id: String },

    #[error("invalid job type: {value}")]
    InvalidJobType { value: String },
}
```

**Step 3: Write `repository.rs`**

Create `crates/claw/src/job/repository.rs`:

```rust
use async_trait::async_trait;

use crate::agents::thread::ThreadId;
use crate::db::DbError;
use crate::workflow::{JobId, WorkflowStatus};

use super::types::JobRecord;

#[async_trait]
pub trait JobRepository: Send + Sync {
    async fn create(&self, job: &JobRecord) -> Result<(), DbError>;
    async fn get(&self, id: &JobId) -> Result<Option<JobRecord>, DbError>;
    async fn update_status(
        &self,
        id: &JobId,
        status: WorkflowStatus,
        started_at: Option<&str>,
        finished_at: Option<&str>,
    ) -> Result<(), DbError>;
    async fn update_thread_id(&self, id: &JobId, thread_id: &ThreadId) -> Result<(), DbError>;
    async fn find_ready_jobs(&self, limit: usize) -> Result<Vec<JobRecord>, DbError>;
    async fn find_due_cron_jobs(&self, now: &str) -> Result<Vec<JobRecord>, DbError>;
    async fn update_scheduled_at(&self, id: &JobId, next: &str) -> Result<(), DbError>;
    async fn list_by_group(&self, group_id: &str) -> Result<Vec<JobRecord>, DbError>;
    async fn delete(&self, id: &JobId) -> Result<bool, DbError>;
}
```

**Step 4: Write `mod.rs`**

Create `crates/claw/src/job/mod.rs`:

```rust
pub mod error;
pub mod repository;
pub mod types;

pub use error::JobError;
pub use repository::JobRepository;
pub use types::{JobRecord, JobType};
```

**Step 5: Register module in `lib.rs`**

In `crates/claw/src/lib.rs`, add after `pub mod error;`:

```rust
pub mod job;
```

**Step 6: Run tests**

Run: `cargo test -p claw job`
Expected: 3 tests pass (job_type_roundtrip, job_type_invalid, job_type_display)

**Step 7: Commit**

```bash
git add crates/claw/src/job/ crates/claw/src/lib.rs
git commit -m "feat(job): add generalized Job domain types, repository trait, and error types"
```

---

### Task 4: SQLite JobRepository implementation

**Files:**
- Create: `crates/claw/src/db/sqlite/job.rs`
- Modify: `crates/claw/src/db/sqlite/mod.rs` (add `mod job; pub use job::SqliteJobRepository;`)
- Modify: `crates/claw/src/db/mod.rs` (add `pub use sqlite::SqliteJobRepository;` re-export)

**Step 1: Write `SqliteJobRepository`**

Create `crates/claw/src/db/sqlite/job.rs`:

```rust
use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::agents::AgentId;
use crate::agents::thread::ThreadId;
use crate::db::DbError;
use crate::job::repository::JobRepository;
use crate::job::types::{JobRecord, JobType};
use crate::workflow::{JobId, WorkflowStatus};

pub struct SqliteJobRepository {
    pool: SqlitePool,
}

impl SqliteJobRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn parse_status(s: &str) -> Result<WorkflowStatus, DbError> {
        WorkflowStatus::parse_str(s).map_err(|e| DbError::QueryFailed { reason: e })
    }

    fn parse_job_type(s: &str) -> Result<JobType, DbError> {
        JobType::parse_str(s).map_err(|e| DbError::QueryFailed { reason: e })
    }

    fn parse_depends_on(s: &str) -> Vec<JobId> {
        serde_json::from_str::<Vec<String>>(s)
            .unwrap_or_default()
            .into_iter()
            .map(JobId::new)
            .collect()
    }

    fn serialize_depends_on(deps: &[JobId]) -> String {
        let ids: Vec<&str> = deps.iter().map(AsRef::as_ref).collect();
        serde_json::to_string(&ids).unwrap_or_else(|_| "[]".to_string())
    }

    fn parse_thread_id(s: Option<String>) -> Option<ThreadId> {
        s.and_then(|v| ThreadId::parse(&v).ok())
    }
}

#[async_trait]
impl JobRepository for SqliteJobRepository {
    async fn create(&self, job: &JobRecord) -> Result<(), DbError> {
        let depends_on = Self::serialize_depends_on(&job.depends_on);
        let thread_id = job.thread_id.map(|t| t.to_string());

        sqlx::query(
            "INSERT INTO jobs (id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(job.id.as_ref())
        .bind(job.job_type.as_str())
        .bind(&job.name)
        .bind(job.status.as_str())
        .bind(job.agent_id.as_ref())
        .bind(&job.context)
        .bind(&job.prompt)
        .bind(&thread_id)
        .bind(&job.group_id)
        .bind(&depends_on)
        .bind(&job.cron_expr)
        .bind(&job.scheduled_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(())
    }

    async fn get(&self, id: &JobId) -> Result<Option<JobRecord>, DbError> {
        let row = sqlx::query_as::<_, (String, String, String, String, String, Option<String>, String, Option<String>, Option<String>, String, Option<String>, Option<String>, Option<String>, Option<String>)>(
            "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at FROM jobs WHERE id = ?"
        )
        .bind(id.as_ref())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| Ok(JobRecord {
            id: JobId::new(r.0),
            job_type: Self::parse_job_type(&r.1)?,
            name: r.2,
            status: Self::parse_status(&r.3)?,
            agent_id: AgentId::new(r.4),
            context: r.5,
            prompt: r.6,
            thread_id: Self::parse_thread_id(r.7),
            group_id: r.8,
            depends_on: Self::parse_depends_on(&r.9),
            cron_expr: r.10,
            scheduled_at: r.11,
            started_at: r.12,
            finished_at: r.13,
        })).transpose()
    }

    async fn update_status(
        &self,
        id: &JobId,
        status: WorkflowStatus,
        started_at: Option<&str>,
        finished_at: Option<&str>,
    ) -> Result<(), DbError> {
        let mut query = String::from("UPDATE jobs SET status = ?, updated_at = datetime('now')");
        if started_at.is_some() {
            query.push_str(", started_at = ?");
        }
        if finished_at.is_some() {
            query.push_str(", finished_at = ?");
        }
        query.push_str(" WHERE id = ? AND status NOT IN ('succeeded', 'failed', 'cancelled')");

        let mut q = sqlx::query(&query).bind(status.as_str());
        if let Some(sa) = started_at {
            q = q.bind(sa);
        }
        if let Some(fa) = finished_at {
            q = q.bind(fa);
        }
        q = q.bind(id.as_ref());

        q.execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(())
    }

    async fn update_thread_id(&self, id: &JobId, thread_id: &ThreadId) -> Result<(), DbError> {
        sqlx::query("UPDATE jobs SET thread_id = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(thread_id.to_string())
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(())
    }

    async fn find_ready_jobs(&self, limit: usize) -> Result<Vec<JobRecord>, DbError> {
        let rows = sqlx::query_as::<_, (String, String, String, String, String, Option<String>, String, Option<String>, Option<String>, String, Option<String>, Option<String>, Option<String>, Option<String>)>(
            "SELECT j.id, j.job_type, j.name, j.status, j.agent_id, j.context, j.prompt, j.thread_id, j.group_id, j.depends_on, j.cron_expr, j.scheduled_at, j.started_at, j.finished_at
             FROM jobs j
             WHERE j.status = 'pending'
               AND j.job_type != 'cron'
               AND NOT EXISTS (
                   SELECT 1 FROM jobs dep
                   WHERE dep.id IN (SELECT value FROM json_each(j.depends_on))
                     AND dep.status != 'succeeded'
               )
             ORDER BY j.created_at ASC
             LIMIT ?"
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter()
            .map(|r| Ok(JobRecord {
                id: JobId::new(r.0),
                job_type: Self::parse_job_type(&r.1)?,
                name: r.2,
                status: Self::parse_status(&r.3)?,
                agent_id: AgentId::new(r.4),
                context: r.5,
                prompt: r.6,
                thread_id: Self::parse_thread_id(r.7),
                group_id: r.8,
                depends_on: Self::parse_depends_on(&r.9),
                cron_expr: r.10,
                scheduled_at: r.11,
                started_at: r.12,
                finished_at: r.13,
            }))
            .collect()
    }

    async fn find_due_cron_jobs(&self, now: &str) -> Result<Vec<JobRecord>, DbError> {
        let rows = sqlx::query_as::<_, (String, String, String, String, String, Option<String>, String, Option<String>, Option<String>, String, Option<String>, Option<String>, Option<String>, Option<String>)>(
            "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at
             FROM jobs
             WHERE job_type = 'cron'
               AND scheduled_at IS NOT NULL
               AND scheduled_at <= ?
             ORDER BY scheduled_at ASC"
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter()
            .map(|r| Ok(JobRecord {
                id: JobId::new(r.0),
                job_type: Self::parse_job_type(&r.1)?,
                name: r.2,
                status: Self::parse_status(&r.3)?,
                agent_id: AgentId::new(r.4),
                context: r.5,
                prompt: r.6,
                thread_id: Self::parse_thread_id(r.7),
                group_id: r.8,
                depends_on: Self::parse_depends_on(&r.9),
                cron_expr: r.10,
                scheduled_at: r.11,
                started_at: r.12,
                finished_at: r.13,
            }))
            .collect()
    }

    async fn update_scheduled_at(&self, id: &JobId, next: &str) -> Result<(), DbError> {
        sqlx::query("UPDATE jobs SET scheduled_at = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(next)
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(())
    }

    async fn list_by_group(&self, group_id: &str) -> Result<Vec<JobRecord>, DbError> {
        let rows = sqlx::query_as::<_, (String, String, String, String, String, Option<String>, String, Option<String>, Option<String>, String, Option<String>, Option<String>, Option<String>, Option<String>)>(
            "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at
             FROM jobs WHERE group_id = ? ORDER BY created_at ASC"
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter()
            .map(|r| Ok(JobRecord {
                id: JobId::new(r.0),
                job_type: Self::parse_job_type(&r.1)?,
                name: r.2,
                status: Self::parse_status(&r.3)?,
                agent_id: AgentId::new(r.4),
                context: r.5,
                prompt: r.6,
                thread_id: Self::parse_thread_id(r.7),
                group_id: r.8,
                depends_on: Self::parse_depends_on(&r.9),
                cron_expr: r.10,
                scheduled_at: r.11,
                started_at: r.12,
                finished_at: r.13,
            }))
            .collect()
    }

    async fn delete(&self, id: &JobId) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM jobs WHERE id = ?")
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(result.rows_affected() > 0)
    }
}
```

**Step 2: Register in `db/sqlite/mod.rs`**

Add after `mod thread;`:
```rust
mod job;
```

Add after `pub use thread::SqliteThreadRepository;`:
```rust
pub use job::SqliteJobRepository;
```

**Step 3: Re-export in `db/mod.rs`**

Add after existing re-exports:
```rust
pub use sqlite::SqliteJobRepository;
```

**Step 4: Verify compilation**

Run: `cargo check -p claw`
Expected: compiles

**Step 5: Commit**

```bash
git add crates/claw/src/db/sqlite/job.rs crates/claw/src/db/sqlite/mod.rs crates/claw/src/db/mod.rs
git commit -m "feat(db): implement SqliteJobRepository for generalized Job model"
```

---

### Task 5: Job repository integration tests

**Files:**
- Create: `crates/claw/tests/job_repository_test.rs`

**Step 1: Write integration tests**

```rust
use claw::db::sqlite::{SqliteJobRepository, connect, migrate};
use claw::job::{JobRepository, JobRecord, JobType};
use claw::workflow::{JobId, WorkflowStatus};

async fn setup() -> SqliteJobRepository {
    let pool = connect("sqlite::memory:").await.unwrap();
    migrate(&pool).await.unwrap();
    // Insert a dummy agent + provider for FK constraints
    sqlx::query("INSERT INTO llm_providers (id, kind, display_name, base_url, model, encrypted_api_key, api_key_nonce) VALUES ('prov-1', 'openai', 'Test', 'http://localhost', 'gpt-4', X'00', X'00')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO agents (id, display_name, provider_id, system_prompt) VALUES ('agent-1', 'Test Agent', 'prov-1', 'You are a test agent')")
        .execute(&pool).await.unwrap();
    SqliteJobRepository::new(pool)
}

#[tokio::test]
async fn create_and_get_standalone_job() {
    let repo = setup().await;
    let job = JobRecord::for_test("job-1", "agent-1", "Test Job", "Do something");
    repo.create(&job).await.unwrap();

    let fetched = repo.get(&JobId::new("job-1")).await.unwrap().unwrap();
    assert_eq!(fetched.name, "Test Job");
    assert_eq!(fetched.job_type, JobType::Standalone);
    assert_eq!(fetched.status, WorkflowStatus::Pending);
    assert_eq!(fetched.prompt, "Do something");
}

#[tokio::test]
async fn find_ready_jobs_no_dependencies() {
    let repo = setup().await;
    let job = JobRecord::for_test("job-1", "agent-1", "Ready", "Go");
    repo.create(&job).await.unwrap();

    let ready = repo.find_ready_jobs(10).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id.as_ref(), "job-1");
}

#[tokio::test]
async fn find_ready_jobs_respects_dependencies() {
    let repo = setup().await;

    // Job A: no dependencies
    let mut job_a = JobRecord::for_test("job-a", "agent-1", "Step A", "First");
    repo.create(&job_a).await.unwrap();

    // Job B: depends on A
    let mut job_b = JobRecord::for_test("job-b", "agent-1", "Step B", "Second");
    job_b.depends_on = vec![JobId::new("job-a")];
    job_b.job_type = JobType::Workflow;
    repo.create(&job_b).await.unwrap();

    // Only A should be ready
    let ready = repo.find_ready_jobs(10).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id.as_ref(), "job-a");

    // Complete A
    repo.update_status(&JobId::new("job-a"), WorkflowStatus::Succeeded, None, None).await.unwrap();

    // Now B should also be ready
    let ready = repo.find_ready_jobs(10).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id.as_ref(), "job-b");
}

#[tokio::test]
async fn find_ready_jobs_skips_cron_templates() {
    let repo = setup().await;
    let mut cron_job = JobRecord::for_test("cron-1", "agent-1", "Cron", "Run daily");
    cron_job.job_type = JobType::Cron;
    cron_job.cron_expr = Some("0 0 * * *".to_string());
    repo.create(&cron_job).await.unwrap();

    let ready = repo.find_ready_jobs(10).await.unwrap();
    assert!(ready.is_empty());
}

#[tokio::test]
async fn update_status_prevents_terminal_transition() {
    let repo = setup().await;
    let job = JobRecord::for_test("job-1", "agent-1", "Done", "Task");
    repo.create(&job).await.unwrap();

    repo.update_status(&JobId::new("job-1"), WorkflowStatus::Succeeded, None, None).await.unwrap();
    // Try to update again - should be no-op (terminal state guard)
    repo.update_status(&JobId::new("job-1"), WorkflowStatus::Running, None, None).await.unwrap();

    let fetched = repo.get(&JobId::new("job-1")).await.unwrap().unwrap();
    assert_eq!(fetched.status, WorkflowStatus::Succeeded);
}

#[tokio::test]
async fn list_by_group() {
    let repo = setup().await;
    let mut job1 = JobRecord::for_test("j1", "agent-1", "W1", "p1");
    job1.group_id = Some("wf-1".to_string());
    let mut job2 = JobRecord::for_test("j2", "agent-1", "W2", "p2");
    job2.group_id = Some("wf-1".to_string());
    let job3 = JobRecord::for_test("j3", "agent-1", "Other", "p3");

    repo.create(&job1).await.unwrap();
    repo.create(&job2).await.unwrap();
    repo.create(&job3).await.unwrap();

    let group = repo.list_by_group("wf-1").await.unwrap();
    assert_eq!(group.len(), 2);
}

#[tokio::test]
async fn delete_job() {
    let repo = setup().await;
    let job = JobRecord::for_test("job-1", "agent-1", "Del", "p");
    repo.create(&job).await.unwrap();

    assert!(repo.delete(&JobId::new("job-1")).await.unwrap());
    assert!(repo.get(&JobId::new("job-1")).await.unwrap().is_none());
    assert!(!repo.delete(&JobId::new("job-1")).await.unwrap());
}
```

**Step 2: Run tests**

Run: `cargo test -p claw --test job_repository_test`
Expected: all 7 tests pass

**Step 3: Commit**

```bash
git add crates/claw/tests/job_repository_test.rs
git commit -m "test(job): add JobRepository integration tests"
```

---

### Task 6: Scheduler config and error types

**Files:**
- Create: `crates/claw/src/scheduler/mod.rs`
- Create: `crates/claw/src/scheduler/config.rs`
- Create: `crates/claw/src/scheduler/error.rs`
- Modify: `crates/claw/src/lib.rs` (add `pub mod scheduler;`)

**Step 1: Write `config.rs`**

Create `crates/claw/src/scheduler/config.rs`:

```rust
use std::time::Duration;

pub struct SchedulerConfig {
    pub poll_interval: Duration,
    pub max_concurrent_jobs: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5),
            max_concurrent_jobs: 5,
        }
    }
}
```

**Step 2: Write `error.rs`**

Create `crates/claw/src/scheduler/error.rs`:

```rust
use thiserror::Error;

use crate::db::DbError;
use crate::error::AgentError;

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error(transparent)]
    Database(#[from] DbError),

    #[error(transparent)]
    Agent(#[from] AgentError),

    #[error("failed to dispatch job `{job_id}`: {reason}")]
    DispatchFailed { job_id: String, reason: String },

    #[error("cron expression parse failed for job `{job_id}`: {reason}")]
    CronParseFailed { job_id: String, reason: String },
}
```

**Step 3: Write `mod.rs`**

Create `crates/claw/src/scheduler/mod.rs`:

```rust
pub mod config;
pub mod error;
mod scheduler;

pub use config::SchedulerConfig;
pub use error::SchedulerError;
pub use scheduler::Scheduler;
```

**Step 4: Create placeholder `scheduler.rs`**

Create `crates/claw/src/scheduler/scheduler.rs`:

```rust
use std::sync::Arc;

use dashmap::DashMap;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::agents::AgentManager;
use crate::job::JobRepository;
use crate::workflow::JobId;

use super::config::SchedulerConfig;
use super::error::SchedulerError;

pub struct Scheduler {
    config: SchedulerConfig,
    job_repository: Arc<dyn JobRepository>,
    agent_manager: Arc<AgentManager>,
    running_jobs: DashMap<JobId, JoinHandle<()>>,
    shutdown: CancellationToken,
}

impl Scheduler {
    #[must_use]
    pub fn new(
        config: SchedulerConfig,
        job_repository: Arc<dyn JobRepository>,
        agent_manager: Arc<AgentManager>,
    ) -> Self {
        Self {
            config,
            job_repository,
            agent_manager,
            running_jobs: DashMap::new(),
            shutdown: CancellationToken::new(),
        }
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub fn running_count(&self) -> usize {
        self.running_jobs.len()
    }
}
```

**Step 5: Register in `lib.rs`**

Add `pub mod scheduler;` in `crates/claw/src/lib.rs`.

**Step 6: Verify compilation**

Run: `cargo check -p claw`
Expected: compiles

**Step 7: Commit**

```bash
git add crates/claw/src/scheduler/ crates/claw/src/lib.rs
git commit -m "feat(scheduler): add config, error types, and Scheduler skeleton"
```

---

### Task 7: Scheduler core loop implementation

**Files:**
- Modify: `crates/claw/src/scheduler/scheduler.rs`

**Step 1: Implement the core loop and tick**

Replace the placeholder `scheduler.rs` with the full implementation:

```rust
use std::str::FromStr;
use std::sync::Arc;

use chrono::Utc;
use cron::Schedule;
use dashmap::DashMap;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::agents::AgentManager;
use crate::agents::thread::ThreadConfig;
use crate::agents::types::AgentId;
use crate::db::DbError;
use crate::job::repository::JobRepository;
use crate::job::types::{JobRecord, JobType};
use crate::workflow::{JobId, WorkflowStatus};

use super::config::SchedulerConfig;
use super::error::SchedulerError;

pub struct Scheduler {
    config: SchedulerConfig,
    job_repository: Arc<dyn JobRepository>,
    agent_manager: Arc<AgentManager>,
    running_jobs: DashMap<JobId, JoinHandle<()>>,
    shutdown: CancellationToken,
}

impl Scheduler {
    #[must_use]
    pub fn new(
        config: SchedulerConfig,
        job_repository: Arc<dyn JobRepository>,
        agent_manager: Arc<AgentManager>,
    ) -> Self {
        Self {
            config,
            job_repository,
            agent_manager,
            running_jobs: DashMap::new(),
            shutdown: CancellationToken::new(),
        }
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub fn running_count(&self) -> usize {
        self.running_jobs.len()
    }

    /// Start the scheduler loop. Runs until the shutdown token is cancelled.
    pub async fn run(&self) {
        tracing::info!(
            poll_interval = ?self.config.poll_interval,
            max_concurrent = self.config.max_concurrent_jobs,
            "Scheduler started"
        );

        loop {
            tokio::select! {
                () = self.shutdown.cancelled() => {
                    tracing::info!("Scheduler shutting down");
                    break;
                }
                () = tokio::time::sleep(self.config.poll_interval) => {
                    if let Err(e) = self.tick().await {
                        tracing::error!("Scheduler tick failed: {e}");
                    }
                }
            }
        }

        self.wait_for_running_jobs().await;
        tracing::info!("Scheduler stopped");
    }

    async fn tick(&self) -> Result<(), SchedulerError> {
        self.cleanup_finished();
        self.check_cron_jobs().await?;

        let running = self.running_jobs.len();
        let available = self.config.max_concurrent_jobs.saturating_sub(running);
        if available == 0 {
            return Ok(());
        }

        let ready_jobs = self.job_repository.find_ready_jobs(available).await?;

        for job in ready_jobs {
            if let Err(e) = self.dispatch(job).await {
                tracing::error!("Failed to dispatch job: {e}");
            }
        }

        Ok(())
    }

    fn cleanup_finished(&self) {
        let finished: Vec<JobId> = self
            .running_jobs
            .iter()
            .filter(|entry| entry.value().is_finished())
            .map(|entry| entry.key().clone())
            .collect();

        for id in finished {
            self.running_jobs.remove(&id);
        }
    }

    async fn check_cron_jobs(&self) -> Result<(), SchedulerError> {
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let due = self.job_repository.find_due_cron_jobs(&now).await?;

        for template in due {
            let new_job = JobRecord {
                id: JobId::new(Uuid::new_v4().to_string()),
                job_type: JobType::Standalone,
                name: format!("{} (cron)", template.name),
                status: WorkflowStatus::Pending,
                agent_id: template.agent_id.clone(),
                context: template.context.clone(),
                prompt: template.prompt.clone(),
                thread_id: None,
                group_id: Some(template.id.to_string()),
                depends_on: vec![],
                cron_expr: None,
                scheduled_at: None,
                started_at: None,
                finished_at: None,
            };

            self.job_repository.create(&new_job).await?;

            if let Some(expr) = &template.cron_expr {
                match self.next_cron_time(expr) {
                    Ok(next) => {
                        self.job_repository
                            .update_scheduled_at(&template.id, &next)
                            .await?;
                    }
                    Err(e) => {
                        tracing::error!(
                            job_id = %template.id,
                            "Failed to compute next cron time: {e}"
                        );
                    }
                }
            }
        }

        Ok(())
    }

    fn next_cron_time(&self, expr: &str) -> Result<String, SchedulerError> {
        let schedule = Schedule::from_str(expr).map_err(|e| SchedulerError::CronParseFailed {
            job_id: String::new(),
            reason: e.to_string(),
        })?;

        schedule
            .upcoming(Utc)
            .next()
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .ok_or_else(|| SchedulerError::CronParseFailed {
                job_id: String::new(),
                reason: "no upcoming time".to_string(),
            })
    }

    async fn dispatch(&self, job: JobRecord) -> Result<(), SchedulerError> {
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        self.job_repository
            .update_status(&job.id, WorkflowStatus::Running, Some(&now), None)
            .await?;

        let job_id = job.id.clone();
        let agent_id = job.agent_id.clone();
        let repo = self.job_repository.clone();
        let agent_manager = self.agent_manager.clone();

        let handle = tokio::spawn(async move {
            match execute_job(job, agent_manager, repo.clone()).await {
                Ok(()) => {
                    tracing::info!(job_id = %job_id, "Job completed successfully");
                }
                Err(e) => {
                    tracing::error!(job_id = %job_id, "Job failed: {e}");
                    let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    let _ = repo
                        .update_status(&job_id, WorkflowStatus::Failed, None, Some(&now))
                        .await;
                }
            }
        });

        self.running_jobs.insert(job_id, handle);
        Ok(())
    }

    async fn wait_for_running_jobs(&self) {
        let handles: Vec<(JobId, JoinHandle<()>)> = self
            .running_jobs
            .iter()
            .map(|entry| (entry.key().clone(), ()))
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|(id, _)| self.running_jobs.remove(&id).map(|(id, h)| (id, h)))
            .collect();

        for (id, handle) in handles {
            tracing::info!(job_id = %id, "Waiting for running job to complete");
            let _ = handle.await;
        }
    }
}

async fn execute_job(
    job: JobRecord,
    agent_manager: Arc<AgentManager>,
    repo: Arc<dyn JobRepository>,
) -> Result<(), SchedulerError> {
    let template = agent_manager
        .get_template(&job.agent_id)
        .await
        .map_err(|e| SchedulerError::DispatchFailed {
            job_id: job.id.to_string(),
            reason: e.to_string(),
        })?
        .ok_or_else(|| SchedulerError::DispatchFailed {
            job_id: job.id.to_string(),
            reason: format!("agent template `{}` not found", job.agent_id),
        })?;

    let runtime_id = agent_manager
        .create_agent(&template)
        .await
        .map_err(|e| SchedulerError::DispatchFailed {
            job_id: job.id.to_string(),
            reason: e.to_string(),
        })?;

    let agent = agent_manager.get(runtime_id).ok_or_else(|| {
        SchedulerError::DispatchFailed {
            job_id: job.id.to_string(),
            reason: "agent runtime vanished after creation".to_string(),
        }
    })?;

    let thread_id = agent.create_thread(ThreadConfig::default());

    repo.update_thread_id(&job.id, &thread_id)
        .await
        .map_err(|e| SchedulerError::DispatchFailed {
            job_id: job.id.to_string(),
            reason: e.to_string(),
        })?;

    // Send the prompt to the thread
    let mut thread_ref = agent.get_thread_mut(&thread_id).ok_or_else(|| {
        SchedulerError::DispatchFailed {
            job_id: job.id.to_string(),
            reason: "thread not found after creation".to_string(),
        }
    })?;

    let handle = thread_ref.send_message(job.prompt.clone()).await;
    drop(thread_ref);

    handle
        .wait_for_result()
        .await
        .map_err(|e| SchedulerError::DispatchFailed {
            job_id: job.id.to_string(),
            reason: e.to_string(),
        })?;

    let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    repo.update_status(&job.id, WorkflowStatus::Succeeded, None, Some(&now))
        .await?;

    // Cleanup the agent runtime
    agent_manager.delete(runtime_id);

    Ok(())
}
```

**Step 2: Verify compilation**

Run: `cargo check -p claw`
Expected: compiles

**Step 3: Commit**

```bash
git add crates/claw/src/scheduler/scheduler.rs
git commit -m "feat(scheduler): implement core loop, cron checking, and job dispatch"
```

---

### Task 8: Wire Scheduler into AppContext

**Files:**
- Modify: `crates/claw/src/claw.rs`

**Step 1: Add Scheduler to AppContext**

In `crates/claw/src/claw.rs`, add imports:

```rust
use crate::db::sqlite::SqliteJobRepository;
use crate::job::JobRepository;
use crate::scheduler::{Scheduler, SchedulerConfig};
use tokio_util::sync::CancellationToken;
```

Add fields to `AppContext`:

```rust
#[derive(Clone)]
pub struct AppContext {
    db_pool: SqlitePool,
    llm_manager: Arc<LLMManager>,
    agent_manager: Arc<AgentManager>,
    tool_manager: Arc<ToolManager>,
    job_repository: Arc<dyn JobRepository>,
    shutdown: CancellationToken,
}
```

Update `init()` to create job_repository, Scheduler, and spawn background task:

```rust
pub async fn init(database_target: Option<String>) -> Result<Self, AgentError> {
    // ... existing pool + migration code ...

    let job_repository: Arc<dyn JobRepository> = Arc::new(SqliteJobRepository::new(pool.clone()));

    let scheduler = Arc::new(Scheduler::new(
        SchedulerConfig::default(),
        job_repository.clone(),
        agent_manager.clone(),
    ));
    let shutdown = scheduler.shutdown_token();

    // Spawn scheduler background task
    tokio::spawn({
        let scheduler = scheduler.clone();
        async move { scheduler.run().await }
    });

    Ok(Self {
        db_pool: pool,
        llm_manager,
        agent_manager,
        tool_manager,
        job_repository,
        shutdown,
    })
}
```

Add accessor and shutdown methods:

```rust
pub fn job_repository(&self) -> Arc<dyn JobRepository> {
    Arc::clone(&self.job_repository)
}

pub fn shutdown(&self) {
    self.shutdown.cancel();
}
```

Update `new()` and `with_pool()` constructors to include the new fields (using a no-op in-memory job repository or the SQLite one).

**Step 2: Verify compilation**

Run: `cargo check -p claw`
Expected: compiles

**Step 3: Commit**

```bash
git add crates/claw/src/claw.rs
git commit -m "feat(claw): wire Scheduler and JobRepository into AppContext"
```

---

### Task 9: Update workflow module - remove Stage, simplify

**Files:**
- Modify: `crates/claw/src/workflow/types.rs` (remove StageId, StageRecord, old JobRecord)
- Modify: `crates/claw/src/workflow/mod.rs` (remove Stage re-exports)
- Modify: `crates/claw/src/workflow/repository.rs` (remove stage/job methods)
- Modify: `crates/claw/src/db/sqlite/workflow.rs` (remove stage/job SQL, simplify)
- Modify: `crates/claw/src/db/mod.rs` (if needed)

**Step 1: Clean up `workflow/types.rs`**

Remove: `StageId` (lines 50-89), `StageRecord` (lines 216-238), old `JobRecord` (lines 240-266), and all associated tests.

Keep: `WorkflowId`, `WorkflowStatus`, `WorkflowRecord`, `JobId` (still used by `job` module).

**Step 2: Clean up `workflow/repository.rs`**

Remove stage and job methods. Keep only:
- `create_workflow`
- `get_workflow`
- `update_workflow_status`
- `list_workflows`
- `delete_workflow`

**Step 3: Clean up `workflow/mod.rs`**

Remove re-exports: `StageId`, `StageRecord`, `JobRecord` (old one).

**Step 4: Clean up `db/sqlite/workflow.rs`**

Remove: `map_stage()`, `map_job()` helpers, stage/job SQL methods, and related tests.

**Step 5: Verify compilation**

Run: `cargo check -p claw`
Expected: compiles (may need to fix api/ references too — see Task 10)

**Step 6: Commit**

```bash
git add crates/claw/src/workflow/ crates/claw/src/db/sqlite/workflow.rs
git commit -m "refactor(workflow): remove Stage and old Job types, simplify to grouping-only"
```

---

### Task 10: Update GraphQL API layer

**Files:**
- Modify: `crates/claw/src/api/types.rs`
- Modify: `crates/claw/src/api/query.rs`
- Modify: `crates/claw/src/api/mutation.rs`
- Modify: `crates/claw/src/api/mod.rs`

**Step 1: Update `api/types.rs`**

Replace with:

```rust
#[derive(Clone, async_graphql::SimpleObject)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub status: String,
    pub jobs: Vec<Job>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, async_graphql::SimpleObject)]
pub struct Job {
    pub id: String,
    pub job_type: String,
    pub name: String,
    pub status: String,
    pub agent_id: String,
    pub context: Option<String>,
    pub prompt: String,
    pub thread_id: Option<String>,
    pub group_id: Option<String>,
    pub depends_on: Vec<String>,
    pub cron_expr: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}
```

**Step 2: Update `api/mutation.rs`**

Remove `AddStageInput`. Update `AddJobInput` to accept `job_type`, `context`, `prompt`, `depends_on`, `cron_expr`, `group_id`. Replace `stage_id` with `group_id`. Update to use `JobRepository` from context data instead of `WorkflowRepository` for job operations. Add `cancelJob` mutation.

**Step 3: Update `api/query.rs`**

Update `workflow()` to load jobs via `job_repository.list_by_group(workflow_id)` instead of loading stages then jobs. Add `job(id)` and `jobs(group_id, status, job_type)` queries.

**Step 4: Update `api/mod.rs`**

Update `create_schema` to accept both `WorkflowRepository` and `JobRepository`:

```rust
pub fn create_schema(
    workflow_repo: Box<dyn WorkflowRepository>,
    job_repo: Box<dyn JobRepository>,
) -> WorkflowSchema {
    Schema::build(QueryRoot, MutationRoot, async_graphql::EmptySubscription)
        .data(workflow_repo)
        .data(job_repo)
        .finish()
}
```

**Step 5: Verify compilation**

Run: `cargo check -p claw`
Expected: compiles

**Step 6: Commit**

```bash
git add crates/claw/src/api/
git commit -m "feat(api): update GraphQL schema for generalized Job model"
```

---

### Task 11: Scheduler integration test

**Files:**
- Create: `crates/claw/tests/scheduler_integration_test.rs`

**Step 1: Write test for scheduler dispatching a standalone job**

This test verifies the full flow: create job in DB → scheduler picks it up → job transitions to succeeded. Since it requires a real LLM provider (or mock), use `mockall` to mock `LlmProvider` for the agent.

```rust
// Test: scheduler picks up a pending standalone job and dispatches it
// Test: scheduler respects dependency ordering for workflow jobs
// Test: scheduler does not exceed max_concurrent_jobs
// Test: cron job creates a new standalone job when due
```

The exact test code depends on how `AgentManager::create_agent` resolves the provider. If `LlmProvider` is mockable, mock it to return a simple response. Otherwise, test at the repository level only (covered in Task 5).

**Step 2: Run tests**

Run: `cargo test -p claw --test scheduler_integration_test`
Expected: tests pass

**Step 3: Commit**

```bash
git add crates/claw/tests/scheduler_integration_test.rs
git commit -m "test(scheduler): add integration tests for job dispatch and cron"
```

---

### Task 12: Update CLAUDE.md project structure docs

**Files:**
- Modify: `CLAUDE.md`

**Step 1: Update the project structure section**

Add `job/` and `scheduler/` modules to the directory tree. Remove `stages` references. Update the Workflow module section. Add Scheduler module and Job module documentation sections.

**Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md with scheduler and job module structure"
```

---

### Task 13: Final verification

**Step 1: Run full test suite**

Run: `cargo test -p claw`
Expected: all tests pass

**Step 2: Run clippy**

Run: `cargo clippy -p claw -- -D warnings`
Expected: no warnings

**Step 3: Run format check**

Run: `cargo fmt -p claw -- --check`
Expected: no formatting issues

**Step 4: Run `prek`**

Run: `prek`
Expected: all checks pass
