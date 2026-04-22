# Flatten Subagent Persistence

> Date: 2026-04-11
> Status: Draft

## Goal

Remove the hierarchical parent-child distinction from agent persistence. All agents are stored flat in the `agents` table with equal status. Subagent configuration becomes a property of the parent agent (`subagent_names` field), not a structural relationship in the database.

## Motivation

The current model uses `parent_agent_id` and `agent_type` to distinguish agents from subagents in the same table. This creates unnecessary complexity:

- Subagents are promoted/demoted in-place via `add_subagent`/`remove_subagent`, blurring the identity boundary.
- `AgentType::Subagent` serves as a recursion guard, but a simple depth limit achieves the same goal more flexibly.
- The parent-child FK complicates migrations and queries without adding proportional value.

The scheduler still needs to know which agents a parent can dispatch to, but this is a configuration concern, not a persistence concern.

## Design

### 1. Data Model Changes

**`AgentRecord` (argus-protocol):**
- Remove `parent_agent_id: Option<AgentId>`
- Remove `agent_type: AgentType`
- Add `subagent_names: Vec<String>` — list of display names this agent can dispatch to

**Remove `AgentType` enum entirely.**

**Scheduling capability is derived:** `!agent_record.subagent_names.is_empty()` means the agent has dispatch capability.

### 2. Database Migration

```sql
-- Step 1: Add subagent_names column
ALTER TABLE agents ADD COLUMN subagent_names TEXT NOT NULL DEFAULT '[]';

-- Step 2: Migrate existing parent-child relationships
-- For each agent that has children (parent_agent_id = this agent),
-- collect child display_names into this agent's subagent_names JSON array.
-- Implementation: temp table or application-level migration.

-- Step 3: Rebuild table without parent_agent_id and agent_type
-- SQLite requires table recreation to drop columns.
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
-- Copy data, drop old, rename.
-- Rebuild indexes:
--   idx_agents_provider_id ON agents(provider_id)             -- kept
--   idx_agents_display_name_unique ON agents(display_name)     -- kept (UNIQUE)
--   idx_agents_parent_agent_id ON agents(parent_agent_id)      -- intentionally dropped
```

### 3. Repository Layer Changes (argus-repository)

**Remove from `AgentRepository` trait:**
- `list_by_parent_id(parent_id)`
- `add_subagent(parent_id, child_id)`
- `remove_subagent(parent_id, child_id)`

**Update:**
- Upsert methods handle `subagent_names` as part of `AgentRecord` serialization (same pattern as `tool_names`).
- Query methods read `subagent_names` from the new column.

### 4. Template Manager Changes (argus-template)

**Remove methods:**
- `add_subagent(parent, child)`
- `remove_subagent(parent, child)`
- `list_subagents(parent_id)`

**Add method:**
- `list_subagents_by_names(names: &[String]) -> Vec<AgentRecord>` — batch lookup by display names

**Update `TomlAgentDef`:**
- Add `subagent_names: Option<Vec<String>>` field
- Map to `AgentRecord::subagent_names` during conversion

**Update TOML agent definitions:**
```toml
[agent]
display_name = "ArgusWing"
subagent_names = ["Chrome Explore"]
```

### 5. Scheduler and Runtime Changes (argus-tool, argus-session, argus-job)

**`argus-tool` (SchedulerBackend trait definition):**
- Review `SchedulerSubagent` type description and `SchedulerTool` tool description string for terminology alignment
- Add `MAX_DISPATCH_DEPTH: u32 = 3` constant alongside `SchedulerBackend` trait

**`SessionSchedulerBackend::list_subagents()`:**
- Read `subagent_names` from the current agent's record
- Call `TemplateManager::list_subagents_by_names()` to resolve

**Dispatch permission check:**
- Replace `agent_type == Standard` with `!agent_record.subagent_names.is_empty()`
- If empty, `dispatch_job()` returns an error immediately

**Recursion guard:**
- Derive dispatch depth from the current thread's parent chain at runtime
- Reject when depth exceeds `MAX_DISPATCH_DEPTH` (default: 3, defined in `argus-tool/src/scheduler.rs`)
- Depth is not persisted or passed through request payloads

### 6. Wing Facade and Desktop Changes

**argus-wing:**
- Remove `add_subagent` / `remove_subagent` API methods
- Agent CRUD naturally includes `subagent_names` via `AgentRecord`

**Tauri commands (desktop/src-tauri):**
- Remove `add_subagent` / `remove_subagent` commands
- Agent DTOs include `subagent_names` field

**Frontend (desktop) — expanded scope:**

TypeScript type changes (`crates/desktop/lib/tauri.ts`):
1. Remove `parent_agent_id` and `agent_type` from the `AgentRecord` type
2. Add `subagent_names: string[]`
3. Remove `listSubagents`, `addSubagent`, `removeSubagent` from the `agents` API object

UI component changes:
1. Rewrite `app/settings/agents/page.tsx` — remove the standard/subagent split and subagent visibility toggle
2. Rewrite `components/settings/agent-card.tsx` — remove subagent badge rendering
3. Rewrite `components/settings/agent-editor.tsx` — remove `parent_agent_id` / `agent_type` filtering, add subagent name list editor
4. Update `components/assistant-ui/agent-selector.tsx` — derive dispatch capability from `subagent_names.length > 0` instead of `agent_type`

Test file updates:
- `tests/chat-subagent-job-details-drawer.test.mjs`
- `tests/chat-subagent-job-details-drawer.behavior.test.tsx`
- `tests/chat-store-subagent-job-details.test.mjs`
- `tests/chat-store-session-model.test.mjs`
- `tests/chat-page-runtime-integration.test.mjs`

### 7. Files Changed (Summary)

| Crate | Change |
|-------|--------|
| `argus-protocol` | Remove `AgentType`, update `AgentRecord`, update `lib.rs` re-exports |
| `argus-repository` | Remove subagent methods, update SQL, add migration, update `sqlite/agent.rs` imports |
| `argus-template` | Update TOML config, remove subagent methods, add `list_subagents_by_names`, update `AGENTS.md` |
| `argus-tool` | Add `MAX_DISPATCH_DEPTH` constant, review `SchedulerSubagent`/`SchedulerTool` descriptions |
| `argus-session` | Update `SchedulerBackend` impl |
| `argus-agent` | Update `turn.rs`, `thread.rs`, update ~5 test blocks |
| `argus-job` | Update recursion guard, update ~6 test blocks |
| `argus-wing` | Remove subagent API, update ~10 test blocks |
| `desktop/src-tauri` | Remove subagent commands |
| `desktop` | Rewrite agent settings page, agent-card, agent-editor, agent-selector; update 5 test files |

**Test code scope:** All test code constructing `AgentRecord` must be updated to remove `parent_agent_id` and `agent_type` fields. This affects approximately 20+ test blocks across `argus-agent`, `argus-job`, `argus-session`, `argus-wing`, and `argus-repository`.

**Documentation sync:** Update crate-level `AGENTS.md` files in `argus-template`, `argus-repository`, and any other crates that reference `AgentType`, `parent_agent_id`, or subagent methods.

**API breaking change:** Removing `AgentType` from `argus-protocol` public exports (`lib.rs`) will cause compile errors in all crates that import it. Remove the enum and fix all downstream compile errors in a single commit.

## Risks and Mitigations

| Risk | Mitigation |
|------|-----------|
| Existing subagent data lost during migration | Migration step 2 preserves parent-child data by converting to `subagent_names` |
| Circular subagent references (A names B, B names A) | Runtime depth limit prevents infinite dispatch loops |
| Subagent names become stale after rename | `list_subagents_by_names` returns only found agents; missing names are silently skipped with a warning log |
| Thread snapshots reference old `AgentType` field | Serde compatibility: `AgentRecord` does not use `deny_unknown_fields`, so old trace files containing `parent_agent_id` and `agent_type` fields will deserialize successfully with those fields silently ignored. No migration of on-disk traces is required. |
