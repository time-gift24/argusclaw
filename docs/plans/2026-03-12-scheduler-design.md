# Scheduler Runtime Design

## Context

ArgusClaw needs a scheduler runtime that monitors the database for pending jobs, checks if they are ready to run (respecting pipeline dependencies), and dispatches them to the appropriate Agent for execution.

## Requirements

- **Job types:** standalone (one-off), workflow (pipeline with dependencies), cron (recurring)
- **Discovery:** polling mode with configurable interval
- **Concurrency:** fixed `max_concurrent_jobs` limit
- **Execution:** each Job creates a new Thread on its assigned Agent, prompt as user message, context injected as system_prompt
- **Result storage:** Job records the associated `thread_id`; results live in Thread message history
- **Graceful shutdown:** via `CancellationToken`

## Data Model

### Jobs table (replaces old jobs + stages)

```sql
CREATE TABLE jobs (
    id          TEXT PRIMARY KEY,
    job_type    TEXT NOT NULL DEFAULT 'standalone',  -- standalone | workflow | cron
    name        TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending',     -- pending | running | succeeded | failed | cancelled

    agent_id    TEXT NOT NULL REFERENCES agents(id),
    context     TEXT,          -- injected into system_prompt
    prompt      TEXT NOT NULL, -- sent as user message to Agent
    thread_id   TEXT,          -- associated Thread (populated after dispatch)

    group_id    TEXT,          -- workflow_id or cron template id (logical grouping)
    depends_on  TEXT,          -- JSON array of job_ids: ["job-1", "job-2"]

    cron_expr   TEXT,          -- cron expression (only for job_type='cron')
    scheduled_at TEXT,         -- next trigger time (cron only)

    started_at  TEXT,
    finished_at TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_jobs_status ON jobs(status);
CREATE INDEX idx_jobs_group_id ON jobs(group_id);
CREATE INDEX idx_jobs_agent_id ON jobs(agent_id);
CREATE INDEX idx_jobs_scheduled_at ON jobs(scheduled_at);
```

### Workflows table (retained as lightweight grouping)

The existing `workflows` table stays but only for logical grouping. The `stages` table is removed — stage ordering is replaced by `depends_on` relationships between jobs.

### Type system

```rust
pub enum JobType {
    Standalone,
    Workflow,
    Cron,
}

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
```

## Scheduler Runtime

### Config

```rust
pub struct SchedulerConfig {
    pub poll_interval: Duration,       // default 5s
    pub max_concurrent_jobs: usize,    // default 5
}
```

### Structure

```rust
pub struct Scheduler {
    config: SchedulerConfig,
    job_repository: Arc<dyn JobRepository>,
    agent_manager: Arc<AgentManager>,
    running_jobs: Arc<DashMap<JobId, JoinHandle<()>>>,
    shutdown: CancellationToken,
}
```

### Core loop

```rust
async fn run(&self) {
    loop {
        select! {
            _ = self.shutdown.cancelled() => break,
            _ = tokio::time::sleep(self.config.poll_interval) => {
                self.tick().await;
            }
        }
    }
}

async fn tick(&self) {
    self.cleanup_finished();
    self.check_cron_jobs().await;

    let available = self.config.max_concurrent_jobs - self.running_jobs.len();
    if available == 0 { return; }

    let ready_jobs = self.job_repository.find_ready_jobs(available).await;
    for job in ready_jobs {
        self.dispatch(job).await;
    }
}
```

### Ready-job query

```sql
SELECT j.* FROM jobs j
WHERE j.status = 'pending'
  AND j.job_type != 'cron'
  AND NOT EXISTS (
      SELECT 1 FROM jobs dep
      WHERE dep.id IN (SELECT value FROM json_each(j.depends_on))
        AND dep.status NOT IN ('succeeded')
  )
ORDER BY j.created_at ASC
LIMIT ?
```

Jobs whose dependencies include a `failed` or `cancelled` job will never become ready. Future work can add failure propagation.

### Job dispatch

1. Update job status to `running`, set `started_at`
2. Get or create Agent runtime from `AgentManager` using `job.agent_id`
3. Create new Thread with `job.context` as system_prompt context
4. Record `thread_id` on the job record
5. Spawn tokio task: send `job.prompt` as user message, await completion
6. On success → update status to `succeeded`; on error → `failed`
7. Track `JoinHandle` in `running_jobs` DashMap

### Cron mechanism

Cron-type jobs are templates that don't execute directly. Each tick:

1. Query cron jobs where `scheduled_at <= now`
2. For each, create a new `standalone` job (inheriting agent_id, context, prompt, group_id = template id)
3. Update template's `scheduled_at` to next trigger time

### Graceful shutdown

`CancellationToken` signals the loop to stop. On shutdown:
- Stop accepting new jobs
- Wait for running jobs to complete (with timeout, then cancel)

## Module Structure

```text
crates/claw/src/
├── scheduler/
│   ├── mod.rs         -- module entry, exports Scheduler
│   ├── config.rs      -- SchedulerConfig
│   ├── error.rs       -- SchedulerError
│   └── scheduler.rs   -- Scheduler core implementation
├── job/
│   ├── mod.rs         -- module entry
│   ├── types.rs       -- JobId, JobType, JobRecord
│   ├── error.rs       -- JobError
│   └── repository.rs  -- JobRepository trait
├── db/sqlite/
│   └── job.rs         -- SqliteJobRepository
```

## JobRepository Trait

```rust
#[async_trait]
pub trait JobRepository: Send + Sync {
    async fn create(&self, job: &JobRecord) -> Result<(), DbError>;
    async fn get(&self, id: &JobId) -> Result<Option<JobRecord>, DbError>;
    async fn update_status(&self, id: &JobId, status: WorkflowStatus, started_at: Option<&str>, finished_at: Option<&str>) -> Result<(), DbError>;
    async fn update_thread_id(&self, id: &JobId, thread_id: &ThreadId) -> Result<(), DbError>;
    async fn find_ready_jobs(&self, limit: usize) -> Result<Vec<JobRecord>, DbError>;
    async fn find_due_cron_jobs(&self, now: &str) -> Result<Vec<JobRecord>, DbError>;
    async fn update_scheduled_at(&self, id: &JobId, next: &str) -> Result<(), DbError>;
    async fn list_by_group(&self, group_id: &str) -> Result<Vec<JobRecord>, DbError>;
    async fn delete(&self, id: &JobId) -> Result<bool, DbError>;
}
```

## AppContext Changes

```rust
pub struct AppContext {
    // existing fields...
    scheduler: Arc<Scheduler>,
    job_repository: Arc<dyn JobRepository>,
}
```

Scheduler starts as a background tokio task during `AppContext::init()`. `AppContext` exposes `shutdown()` for graceful termination.

## API Changes

### Removed
- `addStage` mutation
- Stage-related queries

### Modified
- `addJob` → accepts `job_type`, `context`, `prompt`, `depends_on`, `cron_expr`
- `updateJobStatus` → works with new job structure

### Added
- `jobs(group_id, status, job_type)` query with filters
- `job(id)` query
- `cancelJob(id)` mutation

## Workflow Simplification

Creating a workflow pipeline:
1. Create a Workflow record (logical grouping, just id + name)
2. Create multiple `job_type=workflow` jobs with `group_id = workflow.id`
3. Define dependencies via `depends_on` arrays
4. Scheduler automatically picks up and executes in dependency order

## Migration Strategy

A new migration will:
1. Drop the `stages` table
2. Recreate the `jobs` table with the new schema (or ALTER TABLE to add columns and remove the stage_id FK)
3. Existing data migration if needed (unlikely in current stage)
