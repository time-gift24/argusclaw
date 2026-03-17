# Generated Provider And Agent IDs Design

**Date:** 2026-03-17

## Context

Desktop settings currently treat `provider.id` and `agent.id` as user-entered fields. That couples three different concerns:
- database primary keys
- route parameters for `/settings/providers/[id]` and `/settings/agents/[id]`
- user-facing naming

This coupling already causes a real desktop regression: clicking provider edit can flash and return to the list when the route parameter no longer resolves to a record cleanly. The risk grows when ids contain whitespace, slashes, reserved URL characters, or later need renaming.

The product goal is to remove that burden from users. Users should only name providers and agents. The system should own stable internal identifiers.

## Decision

Adopt system-generated opaque text ids for:
- all LLM providers
- all user-created agent templates

Keep the built-in default template `arguswing` as the single fixed-id exception.

## Why This Approach

### Benefits
- Route params become safe opaque identifiers instead of user content.
- Display names can change without forcing primary-key churn.
- Provider and agent creation flows become simpler and more consistent.
- Existing foreign-key-like references remain string-based, so we can migrate without changing Rust/TypeScript primitives from `String` to numeric ids.

### Why Not Name-Derived Slugs
- Renames become primary-key migrations.
- Duplicate names require suffix logic.
- Slugs still expose route-safety edge cases.
- The database key would still be partially user-controlled.

### Why Not Integer IDs
- This would require a larger cross-layer refactor across DTOs, tests, routing, and persistence.
- It is unnecessary to solve the current UX and routing problem.

## Scope

### Provider IDs
For providers:
- frontends stop asking for an editable `id`
- backend generates a new opaque id on create
- update keeps using the existing stored id
- existing providers are migrated to new generated ids

### Agent IDs
For agents:
- user-created templates stop asking for an editable `id`
- backend generates a new opaque id on create
- update keeps using the existing stored id
- existing non-built-in agent templates are migrated to new generated ids
- built-in `arguswing` remains `arguswing`

## Data Migration Strategy

### Provider Migration
Create a migration that:
1. adds any new provider columns needed by current desktop behavior if missing
2. creates a temporary provider id mapping table
3. generates a fresh opaque id for each existing provider row
4. rewrites `llm_providers.id`
5. rewrites dependent references:
   - `agents.provider_id`
   - `threads.provider_id`
6. drops temporary mapping state

### Agent Migration
Create a migration that:
1. creates a temporary agent id mapping table for all agents except `arguswing`
2. generates a fresh opaque id for each user-created agent template
3. rewrites `agents.id`
4. rewrites dependent references:
   - `jobs.agent_id`
5. leaves `arguswing` untouched

### Runtime Tables
`approval_requests.agent_id` should not be migrated as if it were a template foreign key. It represents runtime-agent identity rather than template identity. We can safely leave existing rows alone or clear pending runtime records if a cleanup step becomes necessary later.

## Backend Contract Changes

### Provider Input
Creation input should no longer require a user-provided `id`.
- `ProviderInput.id` becomes optional at the desktop/Tauri boundary.
- `LlmProviderRecord.id` stays required in the domain model.
- Tauri or claw generates a new `LlmProviderId` when create input omits it.

### Agent Input
Creation input should no longer require a user-provided `id`.
- desktop agent DTO should accept optional `id`
- backend generates a new `AgentId` when create input omits it
- existing update flows continue using stored ids

### ID Generation
Use opaque text ids generated in Rust, based on `Uuid::new_v4()`.
Reasons:
- already used in the codebase
- route-safe without extra escaping logic
- no additional dependency needed

## Frontend UX Changes

### Provider Editor
Remove the editable `ID` field.
Keep:
- display name
- kind
- base URL
- API key
- models
- per-model max context

### Agent Editor
Remove the editable `ID` field.
Keep:
- display name
- description
- version
- provider selection
- system prompt
- model/runtime parameters

### Listing UI
Lists may still surface the internal id as secondary diagnostic text if helpful, but it is no longer editable.

## Testing Strategy

### Desktop Tests
Cover:
- provider editor no longer renders manual id input
- agent editor no longer renders manual id input
- new/create flows can save without user-entered ids
- edit navigation continues to use route ids

### Rust Tests
Cover:
- create-without-id generates ids for providers and agents
- update preserves existing ids
- migration rewrites dependent provider references in agents/threads
- migration rewrites dependent agent references in jobs
- built-in `arguswing` keeps its fixed id

## Risks And Mitigations

### Risk: Foreign-key-like references drift during migration
Mitigation:
- do migration through explicit mapping tables
- verify dependent tables in tests after migration

### Risk: Built-in default agent loses its contract
Mitigation:
- exclude `arguswing` from agent-id migration
- add explicit regression tests around `DEFAULT_AGENT_ID`

### Risk: Frontend create vs update detection becomes ambiguous
Mitigation:
- make create/update mode explicit via route and optional id handling
- only generate ids when input omits `id`

## Success Criteria
- users no longer type provider ids or agent ids in desktop settings
- provider edit route no longer flashes back because of unsafe ids
- existing databases migrate cleanly with provider/agent references preserved
- built-in `arguswing` still loads and behaves as the default template
