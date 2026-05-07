# Scheduled Messages Design

## Goal

Add a first version of scheduled tasks that wakes an existing chat thread by
delivering a normal `ThreadMessage::UserInput` at a scheduled time.

This is intentionally not an isolated/background subagent job. The scheduled
record binds to a target session and thread; a process-level scheduler owns the
timer and dispatches the message.

## Context

ArgusClaw already has the main building blocks:

- `argus-repository` has a `jobs` table with `job_type`, `prompt`, `thread_id`,
  `cron_expr`, and `scheduled_at`.
- `JobRepository` already exposes `find_due_cron_jobs` and
  `update_scheduled_at`.
- `SessionManager::send_message` already validates the target session/thread,
  rehydrates the thread runtime, emits runtime events, and delivers
  `ThreadMessage::UserInput`.
- `argus-job` owns child job execution through `JobManager` and `ThreadPool`,
  but the first scheduled-message use case is chat wakeup, not child job
  dispatch.

OpenClaw's cron implementation informed the design: a scheduler service owns the
timers and persistent records, then triggers either a main session wakeup or an
isolated run. Hermes Agent informed the failure and safety model: scheduled
jobs should be self-contained, persistent, visible, and guarded against repeated
surprise delivery.

## Chosen Approach

Add a `CronScheduler` held by `argus-session`.

The scheduler is a process-level component. It is not owned by any one chat
session. A scheduled message record names its target session/thread, and the
scheduler calls:

```rust
SessionManager::send_message(target_session_id, &target_thread_id, prompt)
```

This keeps the wakeup path aligned with ordinary user messages. Runtime
recovery, MCP resolver wiring, thread pool admission, events, and UI updates all
remain behind the existing session boundary.

## Alternatives Considered

### Put cron execution in `argus-job`

This matches the existing `jobs` table shape, but it blurs the boundary between
child job execution and chat-thread wakeups. A scheduled message does not need a
child job runtime, so `JobManager` would have to call back into session-level
message routing. That inversion makes ownership harder to reason about.

### Put cron execution in server or desktop

This would be faster to wire, but it would make scheduler behavior depend on the
entrypoint. Desktop, server, and future runners could drift. It would also move
core runtime behavior outside the workspace's facade and orchestration layers.

## Data Model

Use the existing `jobs` table for the first version.

Recommended first-version interpretation:

- `job_type = cron`
- `name`: human-readable schedule name
- `status`: current lifecycle state
- `prompt`: message text to deliver as `UserInput`
- `thread_id`: target thread ID
- `cron_expr`: recurring schedule expression when recurring
- `scheduled_at`: next fire time
- `context`: JSON object for small scheduled-message metadata

The `context` JSON should include at least:

```json
{
  "target_session_id": "...",
  "enabled": true,
  "timezone": "Asia/Shanghai"
}
```

Using `context` avoids a first-version schema expansion. If management queries
become session-centric, a later migration can add an explicit `session_id`
column.

## Execution Flow

1. `SessionManager` starts one `CronScheduler` during runtime initialization.
2. The scheduler loads due cron jobs where `scheduled_at <= now`.
3. For each due job, it reads `target_session_id`, `thread_id`, and `prompt`.
4. It calls `SessionManager::send_message`.
5. On success, it computes the next `scheduled_at`.
6. For one-shot jobs, it disables or completes the job.
7. The scheduler arms the next Tokio timer for the nearest upcoming
   `scheduled_at`.

The target session/thread is the delivery target, not the scheduler owner.

## Schedule Semantics

First version should support:

- one-shot scheduled timestamps via `scheduled_at`
- recurring cron expressions via `cron_expr`

The scheduler should validate prompt and schedule at create/update time:

- empty prompt is rejected
- invalid cron expression is rejected
- missing target session/thread is rejected

Missed runs are not replayed. On startup, if a recurring job is overdue, the
scheduler runs it once and advances to the next future time.

## Failure Semantics

Use conservative delivery semantics:

- Successful send: advance `scheduled_at` to the next run.
- Missing target session/thread: pause the scheduled message and record the
  failure reason.
- Empty prompt or invalid schedule: reject at create/update time.
- Busy runtime: rely on existing `send_message` and `ThreadPool` queueing.
- Restart with overdue recurring jobs: run once, then advance; do not backfill
  every missed historical occurrence.

To avoid duplicate execution, the first version can use a single-process
scheduler lock. Before delivery, it should mark a due job as running or set
`started_at`. For multi-process deployments, add a repository-level atomic
claim method before enabling multiple scheduler instances.

## Public Entry Points

Keep first-version entrypoints explicit and thin:

- create scheduled message
- list scheduled messages, optionally filtered by session/thread
- pause scheduled message
- delete scheduled message
- trigger scheduled message now

These can be exposed through server routes and/or Tauri commands. The execution
core remains in `argus-session`.

Do not add model-driven self-scheduling to the existing `scheduler` tool in the
first version. That can be added later after loop prevention and user-visible
approval semantics are clear.

## Testing

Repository tests:

- create a cron job and find it with `find_due_cron_jobs`
- update `scheduled_at`
- paused/deleted jobs do not appear as due

Session scheduler tests:

- due job delivers `UserInput` to the target thread
- successful run advances `scheduled_at`
- missing session/thread pauses the job and records an error
- startup with an overdue recurring job executes once and advances

Integration tests:

- create a session/thread, add a scheduled message, trigger it, and verify the
  thread receives a normal user-input turn
- manual trigger follows the same path as timer execution

## Open Decisions

- Whether to add a dedicated `session_id` column after the first version.
- Whether to support human-readable intervals such as `every 30m`.
- Whether future agent/tool-created schedules require explicit user approval.
