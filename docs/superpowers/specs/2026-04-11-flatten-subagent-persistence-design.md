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
-- Rebuild indexes (including unique on display_name).
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

### 5. Scheduler and Runtime Changes (argus-session, argus-job)

**`SessionSchedulerBackend::list_subagents()`:**
- Read `subagent_names` from the current agent's record
- Call `TemplateManager::list_subagents_by_names()` to resolve

**Dispatch permission check:**
- Replace `agent_type == Standard` with `!agent_record.subagent_names.is_empty()`
- If empty, `dispatch_job()` returns an error immediately

**Recursion guard:**
- Introduce `dispatch_depth: u32` in the dispatch context
- Each nested dispatch increments depth by 1
- Reject when depth exceeds threshold (default: 3)
- Depth is passed through the dispatch chain, not persisted

### 6. Wing Facade and Desktop Changes

**argus-wing:**
- Remove `add_subagent` / `remove_subagent` API methods
- Agent CRUD naturally includes `subagent_names` via `AgentRecord`

**Tauri commands (desktop/src-tauri):**
- Remove `add_subagent` / `remove_subagent` commands
- Agent DTOs include `subagent_names` field

**Frontend (desktop):**
- Agent config UI: add subagent name list editor
- Dispatch capability indicator: derived from `subagent_names.length > 0`

### 7. Files Changed (Summary)

| Crate | Change |
|-------|--------|
| `argus-protocol` | Remove `AgentType`, update `AgentRecord` |
| `argus-repository` | Remove subagent methods, update SQL, add migration |
| `argus-template` | Update TOML config, remove subagent methods, add `list_subagents_by_names` |
| `argus-session` | Update `SchedulerBackend` |
| `argus-job` | Update recursion guard |
| `argus-wing` | Remove subagent API |
| `desktop/src-tauri` | Remove subagent commands |
| `desktop` | Update frontend agent config UI |

## Risks and Mitigations

| Risk | Mitigation |
|------|-----------|
| Existing subagent data lost during migration | Migration step 2 preserves parent-child data by converting to `subagent_names` |
| Circular subagent references (A names B, B names A) | Runtime depth limit prevents infinite dispatch loops |
| Subagent names become stale after rename | `list_subagents_by_names` returns only found agents; missing names are silently skipped with a warning log |
| Thread snapshots reference old `AgentType` field | Thread trace snapshots are self-contained; old traces remain valid. New traces omit the removed field. |
