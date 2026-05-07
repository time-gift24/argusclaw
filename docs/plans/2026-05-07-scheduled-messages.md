# Scheduled Messages Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build scheduled messages that deliver normal `ThreadMessage::UserInput` into a target session/thread at a future time or recurring cron schedule.

**Architecture:** Add a process-level `CronScheduler` owned by `argus-session`. Scheduled message records live in the existing `jobs` table as `job_type = cron`; the scheduler claims due records, calls `SessionManager::send_message`, then advances or disables the schedule. Server routes are thin wrappers over `SessionManager`.

**Tech Stack:** Rust workspace, Tokio timers/tasks, `argus-repository` traits, SQLite/PostgreSQL SQLx implementations, `argus-session` orchestration, axum server routes, TypeScript API client types. Use the Rust `croner` crate for cron expression parsing and `chrono-tz` for timezone-aware next-run calculation.

---

### Task 1: Add Scheduled Message Types

**Files:**
- Modify: `crates/argus-repository/src/types/job.rs`
- Modify: `crates/argus-repository/src/types/mod.rs`

**Step 1: Write failing unit tests**

Add tests in `crates/argus-repository/src/types/job.rs`:

```rust
#[test]
fn job_status_parses_paused() {
    assert_eq!(JobStatus::parse_str("paused").unwrap(), JobStatus::Paused);
    assert_eq!(JobStatus::Paused.as_str(), "paused");
}

#[test]
fn scheduled_message_context_round_trips() {
    let context = ScheduledMessageContext {
        target_session_id: "session-1".to_string(),
        enabled: true,
        timezone: Some("Asia/Shanghai".to_string()),
        last_error: None,
    };

    let json = serde_json::to_string(&context).unwrap();
    let restored: ScheduledMessageContext = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.target_session_id, "session-1");
    assert!(restored.enabled);
    assert_eq!(restored.timezone.as_deref(), Some("Asia/Shanghai"));
}
```

**Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p argus-repository job_status_parses_paused scheduled_message_context_round_trips
```

Expected: FAIL because `Paused` and `ScheduledMessageContext` do not exist.

**Step 3: Implement types**

In `crates/argus-repository/src/types/job.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Paused,
}
```

Update `as_str` and `parse_str` with `"paused"`.

Add:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledMessageContext {
    pub target_session_id: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

fn default_enabled() -> bool {
    true
}

impl ScheduledMessageContext {
    pub fn new(target_session_id: impl Into<String>) -> Self {
        Self {
            target_session_id: target_session_id.into(),
            enabled: true,
            timezone: None,
            last_error: None,
        }
    }
}
```

Export it from `crates/argus-repository/src/types/mod.rs`:

```rust
pub use job::{JobId, JobRecord, JobResult, JobStatus, JobType, ScheduledMessageContext};
```

**Step 4: Run tests**

Run:

```bash
cargo test -p argus-repository job_status_parses_paused scheduled_message_context_round_trips
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-repository/src/types/job.rs crates/argus-repository/src/types/mod.rs
git commit -m "feat: add scheduled message job types"
```

### Task 2: Extend Job Repository Contract

**Files:**
- Modify: `crates/argus-repository/src/traits/job.rs`
- Modify: `crates/argus-repository/src/sqlite/job.rs`
- Modify: `crates/argus-repository/src/postgres/mod.rs`
- Test: `crates/argus-repository/tests/scheduled_message_repository.rs`

**Step 1: Write failing repository tests**

Create `crates/argus-repository/tests/scheduled_message_repository.rs`:

```rust
use std::sync::Arc;

use argus_protocol::ThreadId;
use argus_repository::traits::JobRepository;
use argus_repository::types::{
    AgentId, JobId, JobRecord, JobStatus, JobType, ScheduledMessageContext,
};

async fn sqlite_repo() -> Arc<argus_repository::ArgusSqlite> {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    argus_repository::migrate(&pool).await.unwrap();
    Arc::new(argus_repository::ArgusSqlite::new(pool))
}

fn scheduled_job(id: &str, session_id: &str, scheduled_at: &str) -> JobRecord {
    JobRecord {
        id: JobId::new(id),
        job_type: JobType::Cron,
        name: "Morning ping".to_string(),
        status: JobStatus::Pending,
        agent_id: AgentId::new(1),
        context: Some(serde_json::to_string(&ScheduledMessageContext::new(session_id)).unwrap()),
        prompt: "Wake up and summarize state".to_string(),
        thread_id: Some(ThreadId::parse("018f0000-0000-7000-8000-000000000001").unwrap()),
        group_id: None,
        depends_on: vec![],
        cron_expr: Some("0 9 * * *".to_string()),
        scheduled_at: Some(scheduled_at.to_string()),
        started_at: None,
        finished_at: None,
        parent_job_id: None,
        result: None,
    }
}

#[tokio::test]
async fn due_cron_jobs_exclude_paused_records() {
    let repo = sqlite_repo().await;
    repo.create(&scheduled_job("job-active", "session-1", "2026-05-07T01:00:00Z")).await.unwrap();
    repo.create(&scheduled_job("job-paused", "session-1", "2026-05-07T01:00:00Z")).await.unwrap();
    repo.update_status(&JobId::new("job-paused"), JobStatus::Paused, None, None).await.unwrap();

    let due = repo.find_due_cron_jobs("2026-05-07T02:00:00Z").await.unwrap();

    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id.as_ref(), "job-active");
}

#[tokio::test]
async fn cron_job_can_be_claimed_once() {
    let repo = sqlite_repo().await;
    repo.create(&scheduled_job("job-claim", "session-1", "2026-05-07T01:00:00Z")).await.unwrap();

    assert!(
        repo.claim_cron_job(&JobId::new("job-claim"), "2026-05-07T02:00:00Z")
            .await
            .unwrap()
    );
    assert!(
        !repo.claim_cron_job(&JobId::new("job-claim"), "2026-05-07T02:00:01Z")
            .await
            .unwrap()
    );
}
```

**Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p argus-repository --test scheduled_message_repository
```

Expected: FAIL because `claim_cron_job` does not exist and `find_due_cron_jobs` still returns paused records.

**Step 3: Extend trait**

In `crates/argus-repository/src/traits/job.rs`, add:

```rust
async fn claim_cron_job(&self, id: &JobId, started_at: &str) -> Result<bool, DbError>;

async fn update_cron_after_run(
    &self,
    id: &JobId,
    status: JobStatus,
    scheduled_at: Option<&str>,
    finished_at: &str,
    context: Option<&str>,
) -> Result<(), DbError>;

async fn list_cron_jobs(
    &self,
    include_paused: bool,
    thread_id: Option<&ThreadId>,
) -> Result<Vec<JobRecord>, DbError>;
```

**Step 4: Implement SQLite**

In `crates/argus-repository/src/sqlite/job.rs`:

- Change `find_due_cron_jobs` SQL to include `status = 'pending'`.
- Add atomic claim:

```rust
async fn claim_cron_job(&self, id: &JobId, started_at: &str) -> DbResult<bool> {
    let result = sqlx::query(
        "UPDATE jobs SET status = 'running', started_at = ?1, updated_at = datetime('now')
         WHERE id = ?2 AND job_type = 'cron' AND status = 'pending'",
    )
    .bind(started_at)
    .bind(id.to_string())
    .execute(&self.pool)
    .await
    .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

    Ok(result.rows_affected() == 1)
}
```

- Add `update_cron_after_run`:

```rust
async fn update_cron_after_run(
    &self,
    id: &JobId,
    status: JobStatus,
    scheduled_at: Option<&str>,
    finished_at: &str,
    context: Option<&str>,
) -> DbResult<()> {
    sqlx::query(
        "UPDATE jobs
         SET status = ?1, scheduled_at = ?2, finished_at = ?3, context = COALESCE(?4, context),
             updated_at = datetime('now')
         WHERE id = ?5 AND job_type = 'cron'",
    )
    .bind(status.as_str())
    .bind(scheduled_at)
    .bind(finished_at)
    .bind(context)
    .bind(id.to_string())
    .execute(&self.pool)
    .await
    .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
    Ok(())
}
```

- Add `list_cron_jobs` using only repository SQL:

```rust
let status_clause = if include_paused {
    "status IN ('pending', 'paused', 'running', 'failed')"
} else {
    "status = 'pending'"
};
```

Use two explicit queries rather than string-concatenating untrusted input: one branch with `thread_id`, one without.

**Step 5: Implement PostgreSQL**

Mirror the SQLite implementation in `crates/argus-repository/src/postgres/mod.rs` with `$1`, `$2`, etc. Keep all SQL inside this repository crate.

**Step 6: Run tests**

Run:

```bash
cargo test -p argus-repository --test scheduled_message_repository
```

Expected: PASS.

**Step 7: Commit**

```bash
git add crates/argus-repository/src/traits/job.rs crates/argus-repository/src/sqlite/job.rs crates/argus-repository/src/postgres/mod.rs crates/argus-repository/tests/scheduled_message_repository.rs
git commit -m "feat: add cron job repository operations"
```

### Task 3: Add Schedule Calculation

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/argus-session/Cargo.toml`
- Create: `crates/argus-session/src/scheduled_messages.rs`
- Modify: `crates/argus-session/src/lib.rs`

**Step 1: Add failing schedule tests**

Create `crates/argus-session/src/scheduled_messages.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_cron_run_uses_timezone() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-05-07T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        let next = next_cron_run("0 9 * * *", Some("Asia/Shanghai"), now).unwrap();

        assert_eq!(next.to_rfc3339(), "2026-05-07T01:00:00+00:00");
    }

    #[test]
    fn invalid_cron_expression_is_rejected() {
        let now = chrono::Utc::now();
        assert!(next_cron_run("not cron", None, now).is_err());
    }
}
```

**Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p argus-session scheduled_messages::tests::next_cron_run_uses_timezone
```

Expected: FAIL because the module/function/dependency do not exist.

**Step 3: Add dependency**

In workspace `Cargo.toml`, add:

```toml
croner = "3"
chrono-tz = "0.10"
```

In `crates/argus-session/Cargo.toml`, add:

```toml
croner = { workspace = true }
chrono-tz = { workspace = true }
```

**Step 4: Implement schedule helper**

In `crates/argus-session/src/scheduled_messages.rs`:

```rust
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use croner::Cron;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScheduledMessageError {
    #[error("invalid cron expression: {0}")]
    InvalidCron(String),
    #[error("invalid timezone: {0}")]
    InvalidTimezone(String),
    #[error("cron expression has no next run")]
    NoNextRun,
}

pub fn next_cron_run(
    expr: &str,
    timezone: Option<&str>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, ScheduledMessageError> {
    let cron = Cron::from_str(expr)
        .map_err(|error| ScheduledMessageError::InvalidCron(error.to_string()))?;
    if let Some(timezone) = timezone.filter(|value| !value.trim().is_empty()) {
        let tz: Tz = timezone
            .parse()
            .map_err(|_| ScheduledMessageError::InvalidTimezone(timezone.to_string()))?;
        let local_now = now.with_timezone(&tz);
        return cron
            .find_next_occurrence(&local_now, false)
            .map(|next| next.with_timezone(&Utc))
            .map_err(|error| ScheduledMessageError::InvalidCron(error.to_string()));
    }
    cron.find_next_occurrence(&now, false)
        .map(|next| next.with_timezone(&Utc))
        .map_err(|error| ScheduledMessageError::InvalidCron(error.to_string()))
}
```

Export the module from `crates/argus-session/src/lib.rs`:

```rust
pub mod scheduled_messages;
```

**Step 5: Run tests**

Run:

```bash
cargo test -p argus-session scheduled_messages::tests::next_cron_run_uses_timezone
```

Expected: PASS.

**Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/argus-session/Cargo.toml crates/argus-session/src/scheduled_messages.rs crates/argus-session/src/lib.rs
git commit -m "feat: add scheduled message schedule calculation"
```

### Task 4: Implement CronScheduler Core

**Files:**
- Modify: `crates/argus-session/src/scheduled_messages.rs`
- Modify: `crates/argus-session/src/manager.rs`

**Step 1: Write failing scheduler unit test**

In `crates/argus-session/src/scheduled_messages.rs`, add a mock dispatcher and repository-backed test:

```rust
#[cfg(test)]
mod scheduler_tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingDispatcher {
        messages: Mutex<Vec<(SessionId, ThreadId, String)>>,
    }

    #[async_trait]
    impl ScheduledMessageDispatcher for RecordingDispatcher {
        async fn deliver_scheduled_message(
            &self,
            session_id: SessionId,
            thread_id: ThreadId,
            prompt: String,
        ) -> Result<(), ScheduledMessageError> {
            self.messages.lock().unwrap().push((session_id, thread_id, prompt));
            Ok(())
        }
    }

    #[tokio::test]
    async fn run_due_once_delivers_user_input_and_advances_recurring_job() {
        // Build an in-memory repository, insert one due cron job, run scheduler.run_due_once,
        // assert the dispatcher saw exactly one prompt and the job is pending with a future scheduled_at.
    }
}
```

Use the helper from Task 2 to build the SQLite repository and insert a due job.

**Step 2: Run test to verify failure**

Run:

```bash
cargo test -p argus-session run_due_once_delivers_user_input_and_advances_recurring_job
```

Expected: FAIL because `CronScheduler` and `ScheduledMessageDispatcher` do not exist.

**Step 3: Implement dispatcher trait and scheduler**

In `crates/argus-session/src/scheduled_messages.rs`, add:

```rust
use argus_protocol::{SessionId, ThreadId};
use argus_repository::traits::JobRepository;
use argus_repository::types::{JobId, JobRecord, JobStatus, ScheduledMessageContext};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Notify;

#[async_trait]
pub trait ScheduledMessageDispatcher: Send + Sync {
    async fn deliver_scheduled_message(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        prompt: String,
    ) -> Result<(), ScheduledMessageError>;
}

pub struct CronScheduler {
    job_repository: Arc<dyn JobRepository>,
    dispatcher: Arc<dyn ScheduledMessageDispatcher>,
    notify: Arc<Notify>,
}
```

Implement:

```rust
impl CronScheduler {
    pub fn new(
        job_repository: Arc<dyn JobRepository>,
        dispatcher: Arc<dyn ScheduledMessageDispatcher>,
    ) -> Self {
        Self {
            job_repository,
            dispatcher,
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn notify_changed(&self) {
        self.notify.notify_waiters();
    }

    pub async fn run_due_once(&self, now: chrono::DateTime<Utc>) -> Result<usize, ScheduledMessageError> {
        let jobs = self.job_repository.find_due_cron_jobs(&now.to_rfc3339()).await?;
        let mut delivered = 0;
        for job in jobs {
            if self.run_one_due_job(job, now).await? {
                delivered += 1;
            }
        }
        Ok(delivered)
    }
}
```

`run_one_due_job` should:

- parse `ScheduledMessageContext` from `job.context`
- require `context.enabled`
- require `job.thread_id`
- call `claim_cron_job`
- deliver via dispatcher
- compute next run from `job.cron_expr`
- call `update_cron_after_run`
- on missing target, update context `last_error`, status `Paused`, and no `scheduled_at`

Use `JobStatus::Pending` for active recurring jobs after successful delivery and `JobStatus::Succeeded` for completed one-shot jobs.

**Step 4: Make `SessionManager` a dispatcher**

In `crates/argus-session/src/manager.rs`, implement:

```rust
#[async_trait]
impl scheduled_messages::ScheduledMessageDispatcher for SessionManager {
    async fn deliver_scheduled_message(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        prompt: String,
    ) -> std::result::Result<(), scheduled_messages::ScheduledMessageError> {
        self.send_message(session_id, &thread_id, prompt)
            .await
            .map_err(|error| scheduled_messages::ScheduledMessageError::Dispatch(error.to_string()))
    }
}
```

Add `Dispatch(String)` and `Repository(String)` variants to `ScheduledMessageError`.

**Step 5: Run tests**

Run:

```bash
cargo test -p argus-session run_due_once_delivers_user_input_and_advances_recurring_job
```

Expected: PASS.

**Step 6: Commit**

```bash
git add crates/argus-session/src/scheduled_messages.rs crates/argus-session/src/manager.rs
git commit -m "feat: add scheduled message scheduler core"
```

### Task 5: Wire Scheduler into SessionManager

**Files:**
- Modify: `crates/argus-session/src/manager.rs`
- Modify: `crates/argus-server/src/server_core.rs`
- Modify: `crates/argus-wing/src/lib.rs`

**Step 1: Write failing manager construction test**

In `crates/argus-session/src/manager.rs` tests, add:

```rust
#[tokio::test]
async fn session_manager_installs_cron_scheduler_when_job_repo_is_available() {
    let fixture = make_persistent_manager().await;
    assert!(fixture.session_manager.scheduled_message_scheduler().is_some());
}
```

Use the existing test fixture style near current persistent manager tests.

**Step 2: Run test to verify failure**

Run:

```bash
cargo test -p argus-session session_manager_installs_cron_scheduler_when_job_repo_is_available
```

Expected: FAIL because `scheduled_message_scheduler` does not exist.

**Step 3: Extend SessionManager**

Add field:

```rust
scheduled_message_scheduler: Arc<std::sync::Mutex<Option<Arc<CronScheduler>>>>,
```

Change `SessionManager::new` signature to accept:

```rust
job_repository: Option<Arc<dyn JobRepository>>,
```

After `manager.install_thread_pool_lifecycle_bridge();`, install scheduler:

```rust
if let Some(job_repository) = job_repository {
    let scheduler = Arc::new(CronScheduler::new(job_repository, Arc::new(manager.clone())));
    scheduler.start_background_loop();
    *manager.scheduled_message_scheduler.lock().expect("scheduler mutex poisoned") =
        Some(Arc::clone(&scheduler));
}
```

Add accessor:

```rust
pub fn scheduled_message_scheduler(&self) -> Option<Arc<CronScheduler>> {
    self.scheduled_message_scheduler
        .lock()
        .expect("scheduler mutex poisoned")
        .clone()
}
```

`start_background_loop` may be a no-op in tests if the implementation exposes `run_due_once`; if it spawns, guard tests by using direct calls and make the loop idempotent.

**Step 4: Update call sites**

In `crates/argus-server/src/server_core.rs`, pass:

```rust
Some(Arc::clone(&job_repository))
```

to `SessionManager::new`.

In `crates/argus-wing/src/lib.rs`, pass:

```rust
Some(arc_sqlite.clone() as Arc<dyn JobRepository>)
```

to each `SessionManager::new`.

In tests that construct `SessionManager::new`, pass the existing SQLite job repo or `None` where persistence is intentionally absent.

**Step 5: Run tests**

Run:

```bash
cargo test -p argus-session session_manager_installs_cron_scheduler_when_job_repo_is_available
cargo check -p argus-server -p argus-wing
```

Expected: PASS.

**Step 6: Commit**

```bash
git add crates/argus-session/src/manager.rs crates/argus-server/src/server_core.rs crates/argus-wing/src/lib.rs
git commit -m "feat: wire scheduled message scheduler"
```

### Task 6: Add SessionManager Scheduled Message API

**Files:**
- Modify: `crates/argus-session/src/scheduled_messages.rs`
- Modify: `crates/argus-session/src/manager.rs`

**Step 1: Write failing API tests**

In `crates/argus-session/src/manager.rs` tests:

```rust
#[tokio::test]
async fn create_scheduled_message_rejects_empty_prompt() {
    let fixture = make_persistent_manager().await;
    let err = fixture.session_manager
        .create_scheduled_message(CreateScheduledMessageRequest {
            session_id: fixture.session_id,
            thread_id: fixture.thread_id,
            name: "empty".to_string(),
            prompt: "   ".to_string(),
            cron_expr: Some("0 9 * * *".to_string()),
            scheduled_at: None,
            timezone: None,
        })
        .await
        .expect_err("empty prompt should fail");

    assert!(err.to_string().contains("prompt"));
}

#[tokio::test]
async fn create_scheduled_message_persists_target_session_context() {
    // Create schedule, load job through repository, assert job_type=cron and context.target_session_id.
}
```

**Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p argus-session create_scheduled_message_rejects_empty_prompt create_scheduled_message_persists_target_session_context
```

Expected: FAIL because API types/methods do not exist.

**Step 3: Add request/response types**

In `crates/argus-session/src/scheduled_messages.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScheduledMessageRequest {
    pub session_id: SessionId,
    pub thread_id: ThreadId,
    pub name: String,
    pub prompt: String,
    pub cron_expr: Option<String>,
    pub scheduled_at: Option<String>,
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledMessageSummary {
    pub id: String,
    pub name: String,
    pub status: JobStatus,
    pub session_id: SessionId,
    pub thread_id: ThreadId,
    pub prompt: String,
    pub cron_expr: Option<String>,
    pub scheduled_at: Option<String>,
    pub timezone: Option<String>,
    pub last_error: Option<String>,
}
```

**Step 4: Implement manager methods**

In `crates/argus-session/src/manager.rs`, implement these public method signatures:

```rust
pub async fn create_scheduled_message(
    &self,
    request: CreateScheduledMessageRequest,
) -> Result<ScheduledMessageSummary>;

pub async fn list_scheduled_messages(
    &self,
    thread_id: Option<&ThreadId>,
) -> Result<Vec<ScheduledMessageSummary>>;

pub async fn pause_scheduled_message(&self, job_id: &str) -> Result<ScheduledMessageSummary>;

pub async fn delete_scheduled_message(&self, job_id: &str) -> Result<bool>;

pub async fn trigger_scheduled_message_now(&self, job_id: &str) -> Result<bool>;
```

Implementation notes:

- Validate thread belongs to session with `ensure_thread_in_session`.
- Validate non-empty prompt.
- Require exactly one of `cron_expr` or `scheduled_at`.
- Validate `cron_expr` using `next_cron_run`.
- For one-shot `scheduled_at`, parse RFC3339 and store it.
- Create a `JobRecord` with `job_type = JobType::Cron`, `status = JobStatus::Pending`, `thread_id = Some(request.thread_id)`, `context = ScheduledMessageContext`.
- Use default or target thread agent/template for `agent_id` if needed; keep it stable and documented in code.
- Notify scheduler after create/pause/delete/trigger.

**Step 5: Run tests**

Run:

```bash
cargo test -p argus-session create_scheduled_message
```

Expected: PASS.

**Step 6: Commit**

```bash
git add crates/argus-session/src/scheduled_messages.rs crates/argus-session/src/manager.rs
git commit -m "feat: add scheduled message manager api"
```

### Task 7: Add Server Routes

**Files:**
- Create: `crates/argus-server/src/routes/scheduled_messages.rs`
- Modify: `crates/argus-server/src/routes/mod.rs`
- Modify: `crates/argus-server/src/server_core.rs`
- Test: `crates/argus-server/tests/scheduled_messages_api.rs`

**Step 1: Write failing route tests**

Create `crates/argus-server/tests/scheduled_messages_api.rs`:

```rust
mod support;

use axum::http::StatusCode;
use serde_json::json;

#[tokio::test]
async fn scheduled_message_routes_create_list_and_pause() {
    let ctx = support::TestContext::new().await;
    let created_thread = support::create_chat_session_with_thread(&ctx).await;

    let create = ctx
        .post_json(
            "/api/v1/scheduled-messages",
            &json!({
                "session_id": created_thread.session_id,
                "thread_id": created_thread.thread_id,
                "name": "Daily check",
                "prompt": "Run the daily check",
                "cron_expr": "0 9 * * *",
                "timezone": "Asia/Shanghai"
            }),
        )
        .await;
    assert_eq!(create.status(), StatusCode::CREATED);

    let list = ctx.get("/api/v1/scheduled-messages").await;
    assert_eq!(list.status(), StatusCode::OK);

    let body: serde_json::Value = support::json_body(list).await;
    let id = body.as_array().unwrap()[0]["id"].as_str().unwrap();

    let pause = ctx.post_json(&format!("/api/v1/scheduled-messages/{id}/pause"), &json!({})).await;
    assert_eq!(pause.status(), StatusCode::OK);
}
```

If `support::create_chat_session_with_thread` does not exist, add it to `crates/argus-server/tests/support/mod.rs` by composing existing chat routes.

**Step 2: Run test to verify failure**

Run:

```bash
cargo test -p argus-server --test scheduled_messages_api
```

Expected: FAIL because routes do not exist.

**Step 3: Implement ServerCore wrappers**

In `crates/argus-server/src/server_core.rs`, add narrow methods:

```rust
pub async fn create_scheduled_message(
    &self,
    request: CreateScheduledMessageRequest,
) -> Result<ScheduledMessageSummary> {
    self.session_manager.create_scheduled_message(request).await
}
```

Add list/pause/delete/trigger wrappers.

**Step 4: Implement route module**

In `crates/argus-server/src/routes/scheduled_messages.rs`, follow `chat.rs` style:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScheduledMessageBody {
    pub session_id: String,
    pub thread_id: String,
    pub name: String,
    pub prompt: String,
    pub cron_expr: Option<String>,
    pub scheduled_at: Option<String>,
    pub timezone: Option<String>,
}
```

Parse IDs, call `state.core()`, return `MutationResponse<ScheduledMessageSummary>` for mutations and plain `Vec<ScheduledMessageSummary>` for list.

Routes:

- `GET|POST /api/v1/scheduled-messages`
- `POST /api/v1/scheduled-messages/{job_id}/pause`
- `POST /api/v1/scheduled-messages/{job_id}/trigger`
- `DELETE /api/v1/scheduled-messages/{job_id}`

**Step 5: Register routes**

In `crates/argus-server/src/routes/mod.rs`:

```rust
pub mod scheduled_messages;
```

Register the routes in `router()`.

**Step 6: Run tests**

Run:

```bash
cargo test -p argus-server --test scheduled_messages_api
```

Expected: PASS.

**Step 7: Commit**

```bash
git add crates/argus-server/src/routes/scheduled_messages.rs crates/argus-server/src/routes/mod.rs crates/argus-server/src/server_core.rs crates/argus-server/tests/scheduled_messages_api.rs crates/argus-server/tests/support/mod.rs
git commit -m "feat: expose scheduled message routes"
```

### Task 8: Add TypeScript API Client Types

**Files:**
- Modify: `apps/web/src/lib/api.ts`

**Step 1: Write failing TypeScript test or type usage**

If there is an API client test file, add a compile-time usage for:

```ts
const schedule: CreateScheduledMessageRequest = {
  session_id: "session",
  thread_id: "thread",
  name: "Daily",
  prompt: "Run daily check",
  cron_expr: "0 9 * * *",
  timezone: "Asia/Shanghai",
};
```

If no existing type test exists, rely on `pnpm test`/`pnpm typecheck` after adding the client methods.

**Step 2: Add types**

In `apps/web/src/lib/api.ts`:

```ts
export interface ScheduledMessageSummary {
  id: string;
  name: string;
  status: "pending" | "queued" | "running" | "succeeded" | "failed" | "cancelled" | "paused";
  session_id: string;
  thread_id: string;
  prompt: string;
  cron_expr: string | null;
  scheduled_at: string | null;
  timezone: string | null;
  last_error: string | null;
}

export interface CreateScheduledMessageRequest {
  session_id: string;
  thread_id: string;
  name: string;
  prompt: string;
  cron_expr?: string | null;
  scheduled_at?: string | null;
  timezone?: string | null;
}
```

Add `ApiClient` methods:

```ts
listScheduledMessages(): Promise<ScheduledMessageSummary[]>;
createScheduledMessage(input: CreateScheduledMessageRequest): Promise<ScheduledMessageSummary>;
pauseScheduledMessage(id: string): Promise<ScheduledMessageSummary>;
triggerScheduledMessage(id: string): Promise<ScheduledMessageSummary>;
deleteScheduledMessage(id: string): Promise<void>;
```

Implement them in the concrete API client using the existing `requestJson`/`requestVoid` helpers.

**Step 3: Run frontend checks**

Run:

```bash
cd apps/web
pnpm test -- api
```

If the repo uses a different frontend command, run the existing web test command from `package.json`.

Expected: PASS.

**Step 4: Commit**

```bash
git add apps/web/src/lib/api.ts
git commit -m "feat: add scheduled message web api client"
```

### Task 9: End-to-End Verification

**Files:**
- No new files unless a failing test reveals a bug.

**Step 1: Run focused Rust tests**

Run:

```bash
cargo test -p argus-repository --test scheduled_message_repository
cargo test -p argus-session scheduled_message
cargo test -p argus-server --test scheduled_messages_api
```

Expected: PASS.

**Step 2: Run package checks**

Run:

```bash
cargo check -p argus-repository -p argus-session -p argus-server -p argus-wing
```

Expected: PASS.

**Step 3: Run pre-commit**

Run:

```bash
prek
```

Expected: PASS.

**Step 4: Review diff**

Run:

```bash
git diff --stat main...HEAD
git diff main...HEAD -- crates/argus-repository crates/argus-session crates/argus-server apps/web
```

Expected: diff is scoped to repository traits/implementations, session scheduler, server route wrappers, and web API types.

**Step 5: Final commit if needed**

If verification required any fix:

```bash
git add <changed-files>
git commit -m "fix: stabilize scheduled message verification"
```

## Notes for Implementation

- Keep SQL only in `argus-repository`.
- Keep scheduled message execution in `argus-session`; do not route through `argus-job` for this first version.
- Do not add model-driven schedule creation to `argus-tool::scheduler` in this pass.
- Prefer `context` JSON for `target_session_id` in the first version; add a schema migration only if implementation proves it is necessary.
- `send_message` should remain the only delivery path so scheduled messages behave like ordinary user input.
