# Flatten Subagent Persistence Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove hierarchical parent-child distinction from agent persistence, replacing it with a flat `subagent_names` field on `AgentRecord`.

**Architecture:** All agents are stored equally in the `agents` table. The `AgentType` enum and `parent_agent_id` column are removed. Subagent configuration becomes a `subagent_names: Vec<String>` property on the parent agent. Scheduling capability is derived from whether this field is non-empty. Recursion protection uses a runtime depth counter instead of a type-level guard.

**Tech Stack:** Rust, SQLite, Tauri, React/TypeScript

**Spec:** `docs/superpowers/specs/2026-04-11-flatten-subagent-persistence-design.md`

---

## Chunk 1: Core Data Model (argus-protocol)

### Task 1: Remove AgentType and update AgentRecord

**Files:**
- Modify: `crates/argus-protocol/src/agent.rs:16-60`
- Modify: `crates/argus-protocol/src/lib.rs:20`

- [ ] **Step 1: Remove the `AgentType` enum from `crates/argus-protocol/src/agent.rs`**

Delete lines 16-24 (the entire `AgentType` enum definition including derives and serde attributes).

- [ ] **Step 2: Remove `parent_agent_id` and `agent_type` fields from `AgentRecord`, add `subagent_names`**

In `AgentRecord` struct, remove:
```rust
pub parent_agent_id: Option<AgentId>,  // lines 55-56
pub agent_type: AgentType,              // lines 58-59
```

Add after `tool_names`:
```rust
#[serde(default)]
pub subagent_names: Vec<String>,
```

- [ ] **Step 3: Update `Default` impl and `for_test()` helper**

The file has a `Default` impl for `AgentRecord` (around lines 62-80) containing `parent_agent_id: None` and `agent_type: AgentType::Standard`. Replace those with `subagent_names: vec![]`.

Similarly, the `for_test()` method (around lines 82-101) sets these same fields. Update it too.

- [ ] **Step 4: Update `lib.rs` re-export**

In `crates/argus-protocol/src/lib.rs` line 20, change:
```rust
pub use agent::{AgentRecord, AgentType};
```
to:
```rust
pub use agent::AgentRecord;
```

- [ ] **Step 5: Commit**

```bash
git add crates/argus-protocol/src/agent.rs crates/argus-protocol/src/lib.rs
git commit -m "feat(protocol): remove AgentType, replace parent_agent_id with subagent_names"
```

> Note: This will break compilation across all downstream crates. That is expected. Subsequent tasks fix each crate.

---

## Chunk 2: Database Migration (argus-repository)

### Task 2: Add SQL migration

**Files:**
- Create: `crates/argus-repository/migrations/20260411000000_flatten_subagent_persistence.sql`

- [ ] **Step 1: Write the migration file**

```sql
-- Flatten subagent persistence: replace parent_agent_id/agent_type with subagent_names

-- Step 1: Add subagent_names column
ALTER TABLE agents ADD COLUMN subagent_names TEXT NOT NULL DEFAULT '[]';

-- Step 2: Migrate existing parent-child relationships into subagent_names
-- For each parent agent, collect its children's display_names into a JSON array.
INSERT INTO agents (id, display_name, subagent_names)
SELECT
    parent.id,
    parent.display_name,
    COALESCE(
        (SELECT json_group_array(child.display_name)
         FROM agents child
         WHERE child.parent_agent_id = parent.id),
        '[]'
    )
FROM agents parent
WHERE EXISTS (SELECT 1 FROM agents child WHERE child.parent_agent_id = parent.id)
ON CONFLICT(id) DO UPDATE SET subagent_names = excluded.subagent_names;

-- Step 3: Rebuild table without parent_agent_id and agent_type (SQLite column drop requires table recreation)
CREATE TABLE agents_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    display_name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '1.0.0',
    provider_id INTEGER REFERENCES llm_providers(id) ON DELETE RESTRICT,
    model_id TEXT,
    system_prompt TEXT NOT NULL,
    tool_names TEXT NOT NULL DEFAULT '[]',
    subagent_names TEXT NOT NULL DEFAULT '[]',
    max_tokens INTEGER,
    temperature INTEGER,
    thinking_config TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO agents_new (id, display_name, description, version, provider_id, model_id,
    system_prompt, tool_names, subagent_names, max_tokens, temperature, thinking_config,
    created_at, updated_at)
SELECT id, display_name, description, version, provider_id, model_id,
    system_prompt, tool_names, subagent_names, max_tokens, temperature, thinking_config,
    created_at, updated_at
FROM agents;

DROP TABLE agents;
ALTER TABLE agents_new RENAME TO agents;

-- Rebuild indexes (kept):
CREATE INDEX IF NOT EXISTS idx_agents_provider_id ON agents(provider_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agents_display_name_unique ON agents(display_name);
-- idx_agents_parent_agent_id is intentionally NOT recreated (dropped).
```

- [ ] **Step 2: Commit**

```bash
git add crates/argus-repository/migrations/20260411000000_flatten_subagent_persistence.sql
git commit -m "feat(repository): add migration to flatten subagent persistence"
```

### Task 3: Update AgentRepository trait and SQLite implementation

**Files:**
- Modify: `crates/argus-repository/src/traits/agent.rs:10-44`
- Modify: `crates/argus-repository/src/sqlite/agent.rs`
- Modify: `crates/argus-repository/src/sqlite/mcp.rs` (test code: lines 14, 74-75)

- [ ] **Step 1: Remove subagent methods from the trait**

In `crates/argus-repository/src/traits/agent.rs`, remove these method signatures:
- `list_by_parent_id` (line 31)
- `add_subagent` (line 37)
- `remove_subagent` (line 40)

- [ ] **Step 2: Update SQLite implementation — remove `AgentType` import**

In `crates/argus-repository/src/sqlite/agent.rs` line 8, change:
```rust
use argus_protocol::{AgentType, ProviderId};
```
to:
```rust
use argus_protocol::ProviderId;
```

- [ ] **Step 3: Update upsert SQL — remove `parent_agent_id` and `agent_type` columns, add `subagent_names`**

For the auto-ID insert (around lines 36-51), change column list and values to remove `parent_agent_id` and `agent_type`, add `subagent_names`:
```sql
INSERT INTO agents (display_name, description, version, provider_id, model_id, system_prompt, tool_names, subagent_names, max_tokens, temperature, thinking_config)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
ON CONFLICT(display_name) DO UPDATE SET
    description = excluded.description, version = excluded.version, provider_id = excluded.provider_id,
    model_id = excluded.model_id, system_prompt = excluded.system_prompt,
    tool_names = excluded.tool_names, subagent_names = excluded.subagent_names,
    max_tokens = excluded.max_tokens, temperature = excluded.temperature,
    thinking_config = excluded.thinking_config, updated_at = CURRENT_TIMESTAMP
```

Same pattern for the explicit-ID insert (around lines 79-95).

- [ ] **Step 4: Update parameter binding — remove `agent_type` serialization, add `subagent_names`**

Remove the `agent_type` string conversion code (around lines 30-33):
```rust
// REMOVE:
let agent_type_str = match record.agent_type {
    AgentType::Standard => "standard",
    AgentType::Subagent => "subagent",
};
```

Add `subagent_names` serialization (follow the `tool_names` pattern at lines 15-18):
```rust
let subagent_names_json = serde_json::to_string(&record.subagent_names).map_err(|e| DbError::QueryFailed {
    reason: format!("failed to serialize subagent_names: {e}"),
})?;
```

Update parameter indices accordingly. Replace `parent_agent_id` param with `subagent_names` param.

- [ ] **Step 5: Update `row_to_agent_record` — remove `parent_agent_id`/`agent_type` parsing, add `subagent_names`**

Remove the `agent_type` deserialization (around lines 282-285) and `parent_agent_id` reading.

Add `subagent_names` deserialization (follow `tool_names` pattern at lines 256-261):
```rust
let subagent_names: Vec<String> = serde_json::from_str(&Self::get_column::<String>(&row, "subagent_names")?).map_err(|e| DbError::QueryFailed {
    reason: format!("failed to parse subagent_names: {e}"),
})?;
```

Update the `AgentRecord` construction to use `subagent_names` instead of `parent_agent_id` and `agent_type`.

- [ ] **Step 6: Remove `add_subagent` and `remove_subagent` method implementations**

Delete the `add_subagent` method (around lines 215-232) and `remove_subagent` method (around lines 234-251) from `SqliteAgentRepository`.

- [ ] **Step 7: Fix all test code in this crate**

Search for `AgentType`, `parent_agent_id`, `agent_type` in test code under `crates/argus-repository/` and update `AgentRecord` constructions to remove these fields and add `subagent_names: vec![]`.

Key file: `crates/argus-repository/src/sqlite/mcp.rs` lines 14, 74-75 (test code referencing `AgentType`).

- [ ] **Step 8: Verify this crate compiles**

```bash
cargo check -p argus-repository
```

Expected: Compiles without errors related to `AgentType`/`parent_agent_id`.

- [ ] **Step 9: Commit**

```bash
git add crates/argus-repository/
git commit -m "feat(repository): update agent CRUD for flat subagent model"
```

---

## Chunk 3: Template and Configuration (argus-template)

### Task 4: Update TomlAgentDef and TemplateManager

**Files:**
- Modify: `crates/argus-template/src/config.rs:6-39`
- Modify: `crates/argus-template/src/manager.rs:162-189`
- Modify: `agents/arguswing.toml`

- [ ] **Step 1: Add `subagent_names` to `TomlAgentDef`**

In `crates/argus-template/src/config.rs`, add to the struct:
```rust
pub subagent_names: Option<Vec<String>>,
```

In the `to_agent_record` method, replace `parent_agent_id: None` and `agent_type: AgentType::Standard` with:
```rust
subagent_names: self.subagent_names.clone().unwrap_or_default(),
```

- [ ] **Step 2: Remove subagent methods from `TemplateManager`**

In `crates/argus-template/src/manager.rs`, remove:
- `list_subagents` method (lines 162-169)
- `add_subagent` method (lines 172-179)
- `remove_subagent` method (lines 182-189)

Add new method:
```rust
pub async fn list_subagents_by_names(&self, names: &[String]) -> Result<Vec<AgentRecord>, DbError> {
    let mut results = Vec::new();
    for name in names {
        if let Some(record) = self.repository.find_by_display_name(name).await? {
            results.push(record);
        } else {
            tracing::warn!("subagent '{}' not found, skipping", name);
        }
    }
    Ok(results)
}
```

- [ ] **Step 3: Update TOML agent definitions**

In `agents/arguswing.toml`, add:
```toml
subagent_names = ["Chrome Explore"]
```

- [ ] **Step 4: Fix all test code in this crate**

Update any `AgentRecord` constructions in test code to remove `parent_agent_id`/`agent_type` and add `subagent_names: vec![]`.

- [ ] **Step 5: Commit**

```bash
git add crates/argus-template/ agents/
git commit -m "feat(template): use subagent_names in TOML config, remove subagent methods"
```

---

## Chunk 4: Scheduler and Runtime (argus-tool, argus-session, argus-job)

### Task 5: Add MAX_DISPATCH_DEPTH to argus-tool

**Files:**
- Modify: `crates/argus-tool/src/scheduler.rs`

- [ ] **Step 1: Add depth limit constant**

In `crates/argus-tool/src/scheduler.rs`, add near the `SchedulerBackend` trait:
```rust
/// Maximum allowed nesting depth for job dispatch chains.
pub const MAX_DISPATCH_DEPTH: u32 = 3;
```

- [ ] **Step 2: Add `dispatch_depth` field to `SchedulerDispatchRequest`**

In the `SchedulerDispatchRequest` struct, add:
```rust
pub dispatch_depth: u32,
```

- [ ] **Step 3: Commit**

```bash
git add crates/argus-tool/src/scheduler.rs
git commit -m "feat(tool): add MAX_DISPATCH_DEPTH and dispatch_depth to SchedulerDispatchRequest"
```

### Task 6: Update SessionSchedulerBackend in argus-session

**Files:**
- Modify: `crates/argus-session/src/manager.rs:393-449`

- [ ] **Step 1: Update `list_subagents` implementation**

Replace the current implementation (lines 428-449) with:
```rust
async fn list_subagents(&self) -> std::result::Result<Vec<SchedulerSubagent>, ToolError> {
    let agent_id = current_agent_id().ok_or_else(|| ToolError::ExecutionFailed {
        message: "no current agent context".into(),
    })?;
    let agent = self.template_manager.get(agent_id).await
        .map_err(|e| ToolError::ExecutionFailed { message: e.to_string() })?
        .ok_or_else(|| ToolError::ExecutionFailed { message: "agent not found".into() })?;

    let records = self.template_manager.list_subagents_by_names(&agent.subagent_names).await
        .map_err(|e| ToolError::ExecutionFailed { message: e.to_string() })?;

    Ok(records.into_iter().map(|r| SchedulerSubagent {
        agent_id: r.id,
        display_name: r.display_name,
        description: r.description,
    }).collect())
}
```

- [ ] **Step 2: Update `dispatch_job` to check scheduling capability and depth**

In `dispatch_job` implementation, add at the start:
```rust
// Check scheduling capability
let agent = self.template_manager.get(current_agent_id()...)
    .await?...;
if agent.subagent_names.is_empty() {
    return Err(ToolError::ExecutionFailed {
        message: "this agent has no subagents configured".into(),
    });
}

// Check dispatch depth
if request.dispatch_depth >= MAX_DISPATCH_DEPTH {
    return Err(ToolError::ExecutionFailed {
        message: format!("maximum dispatch depth ({}) exceeded", MAX_DISPATCH_DEPTH),
    });
}
```

When dispatching to subagent jobs, increment `dispatch_depth` in the request passed to `job_manager.dispatch_job()`.

- [ ] **Step 3: Fix all test code in this crate**

Update `AgentRecord` constructions and any references to `AgentType`/`parent_agent_id` in test code.

- [ ] **Step 4: Commit**

```bash
git add crates/argus-session/
git commit -m "feat(session): derive subagents from subagent_names, add depth guard"
```

### Task 7: Update argus-job

**Files:**
- Modify: `crates/argus-job/src/error.rs:12-14`

- [ ] **Step 1: Remove `SubagentCannotDispatch` error variant**

In `crates/argus-job/src/error.rs`, delete lines 12-14:
```rust
// REMOVE:
/// Subagent cannot dispatch jobs.
#[error("subagent cannot dispatch jobs")]
SubagentCannotDispatch,
```

- [ ] **Step 2: Fix all test code in this crate**

Update `AgentRecord` constructions to remove `parent_agent_id`/`agent_type` and add `subagent_names: vec![]`. Locations include `thread_pool.rs` tests and `job_manager.rs` tests.

- [ ] **Step 3: Commit**

```bash
git add crates/argus-job/
git commit -m "feat(job): remove SubagentCannotDispatch error, update test fixtures"
```

---

## Chunk 5: Agent and Wing Facade

### Task 8: Update argus-agent

**Files:**
- Modify: `crates/argus-agent/src/turn.rs` (test code)
- Modify: `crates/argus-agent/src/thread.rs` (test code)
- Modify: `crates/argus-agent/src/bin/turn.rs` (if needed)

- [ ] **Step 1: Fix all test code**

Search for `AgentType`, `parent_agent_id`, `agent_type` in all files under `crates/argus-agent/`. Update `AgentRecord` constructions to remove these fields and add `subagent_names: vec![]`.

Key locations:
- `src/turn.rs` around line 1692-1693
- `src/thread.rs` around line 1274-1275
- `src/bin/turn.rs` around line 224-225
- `tests/integration_test.rs`
- `tests/trace_integration_test.rs`

- [ ] **Step 2: Commit**

```bash
git add crates/argus-agent/
git commit -m "feat(agent): update test fixtures for flat subagent model"
```

### Task 9: Update argus-wing facade

**Files:**
- Modify: `crates/argus-wing/src/lib.rs:398-414`

- [ ] **Step 1: Remove subagent API methods**

In `crates/argus-wing/src/lib.rs`, remove:
- `list_subagents` method (lines 398-399)
- `add_subagent` method (lines 403-406)
- `remove_subagent` method (lines 410-414)

- [ ] **Step 2: Fix all test code**

Update `AgentRecord` constructions in test code (~10 test blocks).

- [ ] **Step 3: Commit**

```bash
git add crates/argus-wing/
git commit -m "feat(wing): remove subagent binding API"
```

---

## Chunk 6: Desktop — Rust Bridge

### Task 10: Update Tauri commands

**Files:**
- Modify: `crates/desktop/src-tauri/src/commands.rs:183-212`

- [ ] **Step 1: Remove Tauri command functions**

Remove from `commands.rs`:
- `list_subagents` command (lines 183-190)
- `add_subagent` command (lines 193-201)
- `remove_subagent` command (lines 204-212)

- [ ] **Step 2: Remove command registrations**

In `crates/desktop/src-tauri/src/lib.rs` lines 78-80, remove `commands::list_subagents`, `commands::add_subagents`, and `commands::remove_subagent` from the `.invoke_handler()` registration.

- [ ] **Step 3: Commit**

```bash
git add crates/desktop/src-tauri/
git commit -m "feat(desktop-tauri): remove subagent commands"
```

---

## Chunk 7: Desktop — Frontend

### Task 11: Update TypeScript types and API bindings

**Files:**
- Modify: `crates/desktop/lib/tauri.ts:86-151`

- [ ] **Step 1: Update `AgentRecord` TypeScript type**

In `crates/desktop/lib/tauri.ts`, update the interface:
```typescript
export interface AgentRecord {
  id: number;
  display_name: string;
  description: string;
  version: string;
  provider_id: number | null;
  model_id?: string | null;
  system_prompt: string;
  tool_names: string[];
  subagent_names: string[];
  max_tokens?: number;
  temperature?: number;
  thinking_config?: ThinkingConfig;
}
```

Remove `parent_agent_id` and `agent_type` fields.

- [ ] **Step 2: Remove subagent API methods**

Remove from the `agents` API object (lines 144-151):
```typescript
// REMOVE:
listSubagents: (parentId: number) => invoke<AgentRecord[]>("list_subagents", { parentId }),
addSubagent: (parentId: number, childId: number) => invoke<void>("add_subagent", { parentId, childId }),
removeSubagent: (parentId: number, childId: number) => invoke<void>("remove_subagent", { parentId, childId }),
```

- [ ] **Step 3: Commit**

```bash
git add crates/desktop/lib/tauri.ts
git commit -m "feat(desktop): update TypeScript AgentRecord type, remove subagent APIs"
```

### Task 12: Update frontend components

**Files:**
- Modify: `crates/desktop/app/settings/agents/page.tsx:64-178`
- Modify: `crates/desktop/components/settings/agent-card.tsx`
- Modify: `crates/desktop/components/settings/agent-editor.tsx`
- Modify: `crates/desktop/components/assistant-ui/agent-selector.tsx`

- [ ] **Step 1: Update `app/settings/agents/page.tsx`**

Remove the standard/subagent split logic (lines 64-69). Instead of separating into `parentAgents` and `subagents`, render all agents in a single flat list. Remove the subagent visibility toggle.

- [ ] **Step 2: Update `components/settings/agent-card.tsx`**

Remove the local `AgentRecord` interface (lines 9-22) that has `parent_agent_id` and `agent_type` — import from `tauri.ts` instead.

Remove the subagent badge rendering (lines 64-72). Replace with a dispatch-capable indicator derived from `agent.subagent_names.length > 0`.

- [ ] **Step 3: Update `components/settings/agent-editor.tsx`**

- Remove the parent agent dropdown (`<select id="parent_agent_id">`) and related logic.
- Remove the `parent_agent_id`/`agent_type` filtering on candidate list (line 294).
- Add a subagent name list editor (multi-select or tag input) that lets the user select from existing agent display names.
- Remove the circular dependency guard (lines 889-904) — no longer needed since there's no parent-child tree.

- [ ] **Step 4: Update `components/assistant-ui/agent-selector.tsx`**

Remove the parent/subagent grouping logic (lines 70-74, 161-164). Show all agents in a flat list. If dispatch capability indication is needed, derive from `subagent_names.length > 0`.

- [ ] **Step 5: Commit**

```bash
git add crates/desktop/app/ crates/desktop/components/
git commit -m "feat(desktop): rewrite agent settings UI for flat subagent model"
```

### Task 13: Update frontend tests

**Files:**
- Modify: `crates/desktop/tests/chat-subagent-job-details-drawer.test.mjs`
- Modify: `crates/desktop/tests/chat-subagent-job-details-drawer.behavior.test.tsx`
- Modify: `crates/desktop/tests/chat-store-subagent-job-details.test.mjs`
- Modify: `crates/desktop/tests/chat-store-session-model.test.mjs`
- Modify: `crates/desktop/tests/chat-page-runtime-integration.test.mjs`

- [ ] **Step 1: Update all test files**

Search for `parent_agent_id`, `agent_type`, `listSubagents`, `addSubagent`, `removeSubagent` in all test files. Update mock data and assertions to use the new flat model:
- Remove `parent_agent_id` and `agent_type` from mock `AgentRecord` objects
- Add `subagent_names: []` to mock objects
- Remove or update tests that call removed API methods

- [ ] **Step 2: Commit**

```bash
git add crates/desktop/tests/
git commit -m "test(desktop): update test fixtures for flat subagent model"
```

---

## Chunk 8: Verification and Cleanup

### Task 14: Full build and test verification

**Files:** None (verification only)

- [ ] **Step 1: Build the entire workspace**

```bash
cargo build --workspace
```

Expected: Clean build with no errors.

- [ ] **Step 2: Run all Rust tests**

```bash
cargo test --workspace
```

Expected: All tests pass.

- [ ] **Step 3: Run clippy**

```bash
cargo clippy --workspace -- -D warnings
```

Expected: No warnings.

- [ ] **Step 4: Run desktop tests**

```bash
cd crates/desktop && pnpm test
```

Expected: All tests pass.

### Task 15: Update crate documentation

**Files:**
- Modify: `crates/argus-template/CLAUDE.md`
- Modify: `crates/argus-template/AGENTS.md`
- Modify: `crates/argus-repository/CLAUDE.md` (if it references subagent methods)
- Modify: `crates/argus-session/CLAUDE.md` (references scheduler behavior)

- [ ] **Step 1: Update CLAUDE.md files**

Remove references to `AgentType`, `parent_agent_id`, subagent binding methods. Update descriptions to reflect the flat model with `subagent_names`.

- [ ] **Step 2: Update AGENTS.md files**

Sync any changes from CLAUDE.md updates.

- [ ] **Step 3: Commit**

```bash
git add crates/*/CLAUDE.md crates/*/AGENTS.md
git commit -m "docs: update crate documentation for flat subagent model"
```

### Task 16: Final commit and push

- [ ] **Step 1: Run prek**

```bash
prek
```

Expected: All checks pass.

- [ ] **Step 2: Push the branch**

```bash
git push -u origin codex/flatten-subagent-persistence
```

- [ ] **Step 3: Squash-review (optional)**

If the commit count is excessive, consider an interactive rebase to squash related commits. However, given the atomic nature of each task's commits, this may not be necessary.
