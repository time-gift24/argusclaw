# Cascade Agent Delete Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add an explicit cascade delete option for agent templates that removes associated jobs, matching threads, and sessions left empty by that cleanup, while preserving safe default deletion.

**Architecture:** Keep delete orchestration in `argus-template`, SQL in `argus-repository`, and transport/UI pass-through in `argus-wing`, `argus-server`, Tauri, desktop React, and Web Vue. Use an `AgentDeleteReport` and explicit `cascade_associations` option so callers can show accurate cleanup feedback.

**Tech Stack:** Rust workspace with async traits, sqlx SQLite/Postgres repositories, Tauri commands, React desktop frontend, Vue 3 + OpenTiny Web admin, Vitest and Cargo tests.

---

### Task 1: Repository Contract and Report Types

**Files:**
- Modify: `crates/argus-repository/src/traits/agent.rs`
- Modify: `crates/argus-repository/src/types/agent.rs`
- Modify: `crates/argus-repository/src/types/mod.rs`
- Test: `crates/argus-repository/tests/agent_repository.rs` or the closest existing repository test file

**Step 1: Write the failing trait-level test**

Add a SQLite repository test that creates:

- one agent to delete
- one job with `jobs.agent_id = agent_id`
- one session with a thread whose `template_id = agent_id`
- one session with both a matching thread and an unrelated thread

Assert that a new call like this exists and returns counts or IDs:

```rust
let report = AgentRepository::delete_with_associations(&sqlite, &agent_id)
    .await
    .expect("cascade delete should succeed");

assert!(report.agent_deleted);
assert_eq!(report.deleted_job_count, 1);
assert_eq!(report.deleted_thread_count, 2);
assert_eq!(report.deleted_session_count, 1);
```

Also query the database through repository APIs to assert the mixed session still exists and the unrelated thread remains.

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-repository cascade_delete
```

Expected: FAIL because `AgentDeleteReport` and `delete_with_associations` do not exist.

**Step 3: Add minimal types and trait method**

Define a report type in `crates/argus-repository/src/types/agent.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDeleteReport {
    pub agent_deleted: bool,
    pub deleted_job_count: u64,
    pub deleted_thread_count: u64,
    pub deleted_session_count: u64,
}

impl AgentDeleteReport {
    pub fn empty(agent_deleted: bool) -> Self {
        Self {
            agent_deleted,
            deleted_job_count: 0,
            deleted_thread_count: 0,
            deleted_session_count: 0,
        }
    }
}
```

Export it from `types/mod.rs`, then extend `AgentRepository`:

```rust
async fn delete_with_associations(
    &self,
    id: &AgentId,
) -> Result<AgentDeleteReport, DbError>;
```

**Step 4: Run test to verify compile failure moves to implementations**

Run:

```bash
cargo test -p argus-repository cascade_delete
```

Expected: FAIL because SQLite/Postgres implementations are missing the trait method.

**Step 5: Commit**

```bash
git add crates/argus-repository/src/traits/agent.rs crates/argus-repository/src/types/agent.rs crates/argus-repository/src/types/mod.rs crates/argus-repository/tests
git commit -m "test: specify cascade agent delete repository contract"
```

### Task 2: SQLite Cascade Transaction

**Files:**
- Modify: `crates/argus-repository/src/sqlite/agent.rs`
- Test: `crates/argus-repository/tests/agent_repository.rs` or the test file chosen in Task 1

**Step 1: Write missing SQLite coverage**

Add assertions for:

- messages attached to deleted threads are gone
- jobs without a thread but matching `agent_id` are deleted
- MCP bindings for the agent are deleted by FK cascade

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-repository cascade_delete -- --nocapture
```

Expected: FAIL with unimplemented SQLite method.

**Step 3: Implement SQLite transaction**

In `impl AgentRepository for ArgusSqlite`, implement `delete_with_associations` with a transaction:

```rust
let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
    reason: e.to_string(),
})?;
```

Use SQL in this order:

1. Select matching thread IDs and session IDs:

```sql
SELECT id, session_id FROM threads WHERE template_id = ?1
```

2. Delete matching jobs:

```sql
DELETE FROM jobs WHERE agent_id = ?1
```

3. Delete matching threads:

```sql
DELETE FROM threads WHERE template_id = ?1
```

4. Delete touched sessions that have no threads:

```sql
DELETE FROM sessions
WHERE id IN (...)
  AND NOT EXISTS (SELECT 1 FROM threads WHERE threads.session_id = sessions.id)
```

5. Delete the agent:

```sql
DELETE FROM agents WHERE id = ?1
```

Commit and return `AgentDeleteReport`.

Keep the session `IN` list parameterized. If there are no touched sessions, skip step 4.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p argus-repository cascade_delete -- --nocapture
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-repository/src/sqlite/agent.rs crates/argus-repository/tests
git commit -m "feat: cascade delete agent associations in sqlite"
```

### Task 3: Postgres Cascade Transaction

**Files:**
- Modify: `crates/argus-repository/src/postgres/mod.rs`
- Test: `crates/argus-repository/tests/postgres_repository.rs`

**Step 1: Add Postgres test where harness supports it**

Mirror the SQLite cascade behavior using `ArgusPostgres` if the current Postgres test harness is available. If tests are gated by environment, add the test inside the existing Postgres test pattern.

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-repository postgres cascade_delete -- --nocapture
```

Expected: FAIL until Postgres implementation is added, or SKIP if the existing harness skips without `DATABASE_URL`.

**Step 3: Implement Postgres method**

Use a transaction and PostgreSQL placeholders:

```sql
SELECT id, session_id FROM threads WHERE template_id=$1
DELETE FROM jobs WHERE agent_id=$1
DELETE FROM threads WHERE template_id=$1
DELETE FROM sessions
WHERE id = ANY($1)
  AND NOT EXISTS (SELECT 1 FROM threads WHERE threads.session_id = sessions.id)
DELETE FROM agents WHERE id=$1
```

Return the same `AgentDeleteReport` semantics as SQLite.

**Step 4: Run repository tests**

Run:

```bash
cargo test -p argus-repository cascade_delete -- --nocapture
```

Expected: PASS for SQLite and Postgres where enabled.

**Step 5: Commit**

```bash
git add crates/argus-repository/src/postgres/mod.rs crates/argus-repository/tests/postgres_repository.rs
git commit -m "feat: cascade delete agent associations in postgres"
```

### Task 4: Template Manager Options

**Files:**
- Modify: `crates/argus-template/src/manager.rs`
- Test: `crates/argus-template/src/manager.rs`

**Step 1: Write failing manager tests**

Add tests for:

- `delete(id)` still blocks when references exist
- `delete_with_options(id, cascade_associations: true)` succeeds when only jobs/threads reference the agent
- `delete_with_options(id, cascade_associations: true)` still blocks when another agent has the target display name in `subagent_names`

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-template delete_with_options -- --nocapture
```

Expected: FAIL because options API does not exist.

**Step 3: Implement options API**

Add:

```rust
#[derive(Debug, Clone, Copy, Default)]
pub struct TemplateDeleteOptions {
    pub cascade_associations: bool,
}
```

Update `delete` to call:

```rust
self.delete_with_options(id, TemplateDeleteOptions::default()).await
```

Implement `delete_with_options`:

- load target display name
- count `subagent_names` references
- if `cascade_associations` is false, keep existing thread/job/subagent blocking behavior
- if true, block only on `subagent_names`, then call `repository.delete_with_associations`
- return `AgentDeleteReport`

**Step 4: Run tests**

Run:

```bash
cargo test -p argus-template -- --nocapture
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/argus-template/src/manager.rs
git commit -m "feat: add explicit cascade template delete option"
```

### Task 5: Wing, Server, and Tauri Contracts

**Files:**
- Modify: `crates/argus-wing/src/lib.rs`
- Modify: `crates/argus-server/src/server_core.rs`
- Modify: `crates/argus-server/src/routes/templates.rs`
- Modify: `crates/desktop/src-tauri/src/commands.rs`
- Test: `crates/argus-wing/src/lib.rs`
- Test: `crates/argus-server/tests/management_actions_api.rs`

**Step 1: Write failing contract tests**

Add tests that:

- call `ArgusWing::delete_template_with_options` or updated `delete_template` with cascade options and assert the report
- call server `DELETE /api/v1/agents/templates/{id}?cascade_associations=true` and assert response includes cleanup counts
- verify old `DELETE /api/v1/agents/templates/{id}` still blocks on references

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-wing delete_template
cargo test -p argus-server template_delete -- --nocapture
```

Expected: FAIL because facade/route options are not wired.

**Step 3: Wire facade and server route**

Expose a facade method that accepts `TemplateDeleteOptions` and returns `AgentDeleteReport`.

In `templates.rs`, parse:

```rust
#[derive(Debug, Deserialize)]
pub struct DeleteTemplateQuery {
    #[serde(default)]
    pub cascade_associations: bool,
}
```

Return the report inside the existing mutation envelope.

**Step 4: Wire Tauri command**

Update command signature to accept optional camelCase from JS:

```rust
pub async fn delete_agent_template(
    wing: State<'_, Arc<ArgusWing>>,
    id: i64,
    cascade_associations: Option<bool>,
) -> Result<AgentDeleteReportPayload, String>
```

If Tauri requires camelCase mapping, use the existing serde rename pattern in commands or a request struct.

**Step 5: Run tests**

Run:

```bash
cargo test -p argus-wing delete_template
cargo test -p argus-server template_delete -- --nocapture
```

Expected: PASS.

**Step 6: Commit**

```bash
git add crates/argus-wing/src/lib.rs crates/argus-server/src/server_core.rs crates/argus-server/src/routes/templates.rs crates/desktop/src-tauri/src/commands.rs crates/argus-server/tests/management_actions_api.rs
git commit -m "feat: expose cascade template delete contracts"
```

### Task 6: Desktop React UI

**Files:**
- Modify: `crates/desktop/lib/tauri.ts`
- Modify: `crates/desktop/app/settings/agents/page.tsx`
- Test: `crates/desktop/tests` or the nearest existing frontend binding test

**Step 1: Write failing frontend test**

Add or update a binding/page test that verifies:

- first delete calls `agents.delete(id)` without cascade
- reference-blocked error shows a second confirmation
- confirming calls `agents.delete(id, { cascadeAssociations: true })`
- success message includes report counts

**Step 2: Run test to verify it fails**

Run:

```bash
cd crates/desktop && pnpm test -- --runInBand
```

If there is no generic test script, run the nearest existing desktop test command from `package.json`.

Expected: FAIL because cascade binding/UI does not exist.

**Step 3: Update Tauri binding**

Add `AgentDeleteReport` type and update:

```ts
delete: (id: number, options?: { cascadeAssociations?: boolean }) =>
  invoke<AgentDeleteReport>("delete_agent_template", {
    id,
    cascadeAssociations: options?.cascadeAssociations ?? false,
  })
```

**Step 4: Update agents page**

Track:

- `deleteId`
- `cascadeConfirmAgent`
- `deleteLoading`
- `actionMessage`

On normal delete error, detect the existing reference-blocked Chinese message and open cascade confirmation. On cascade confirm, call with `{ cascadeAssociations: true }`.

**Step 5: Run desktop tests**

Run:

```bash
cd crates/desktop && pnpm test -- --runInBand
```

Expected: PASS.

**Step 6: Commit**

```bash
git add crates/desktop/lib/tauri.ts crates/desktop/app/settings/agents/page.tsx crates/desktop/tests
git commit -m "feat: add desktop cascade agent delete confirmation"
```

### Task 7: Web Vue UI

**Files:**
- Modify: `apps/web/src/lib/api.ts`
- Modify: `apps/web/src/features/templates/TemplatesPage.vue`
- Modify: `apps/web/src/features/templates/templates-page.test.ts`

**Step 1: Write failing API and page tests**

In `templates-page.test.ts`, assert:

- default delete calls `deleteTemplate(8)` or `deleteTemplate(8, { cascadeAssociations: false })`
- blocked delete renders cascade confirmation text
- confirm calls `deleteTemplate(8, { cascadeAssociations: true })`
- success text includes counts

In `api.test.ts`, assert:

```ts
await client.deleteTemplate(8, { cascadeAssociations: true });
expect(fetch).toHaveBeenCalledWith(
  expect.stringContaining("/agents/templates/8?cascade_associations=true"),
  expect.anything(),
);
```

**Step 2: Run tests to verify failure**

Run:

```bash
cd apps/web && pnpm exec vitest run src/lib/api.test.ts src/features/templates/templates-page.test.ts
```

Expected: FAIL because delete options are not implemented.

**Step 3: Update API client**

Add report type and update the interface:

```ts
export interface AgentDeleteReport {
  agent_deleted: boolean;
  deleted_job_count: number;
  deleted_thread_count: number;
  deleted_session_count: number;
}
```

Update `deleteTemplate` to append `?cascade_associations=true` only when requested.

**Step 4: Update TemplatesPage**

Use OpenTiny dialog/confirm if already available in `apps/web/src/lib/opentiny`; otherwise use the existing page-level pattern with inline confirmation controls.

Keep text Chinese:

- `该智能体仍有关联任务或会话线程。`
- `同时删除关联数据`
- `已删除模板，并清理 X 个任务、Y 个线程、Z 个空会话。`

**Step 5: Run targeted tests**

Run:

```bash
cd apps/web && pnpm exec vitest run src/lib/api.test.ts src/features/templates/templates-page.test.ts
```

Expected: PASS.

**Step 6: Commit**

```bash
git add apps/web/src/lib/api.ts apps/web/src/lib/api.test.ts apps/web/src/features/templates/TemplatesPage.vue apps/web/src/features/templates/templates-page.test.ts
git commit -m "feat: add web cascade template delete confirmation"
```

### Task 8: Full Verification

**Files:**
- No source edits expected unless verification finds bugs.

**Step 1: Run Rust checks**

Run:

```bash
cargo test -p argus-repository
cargo test -p argus-template
cargo test -p argus-wing
cargo test -p argus-server
```

Expected: PASS.

**Step 2: Run Web checks**

Run:

```bash
cd apps/web && pnpm exec vitest run
cd apps/web && pnpm build
```

Expected: PASS.

**Step 3: Run Desktop checks**

Run the available desktop test/build commands from `crates/desktop/package.json`, at minimum the targeted tests touched by this change.

Expected: PASS.

**Step 4: Run pre-commit if available**

Run:

```bash
prek
```

Expected: PASS. If `prek` hangs in this environment, capture the process status and note it in the final handoff.

**Step 5: Final commit if fixes were needed**

```bash
git add <fixed files>
git commit -m "fix: stabilize cascade agent delete"
```
