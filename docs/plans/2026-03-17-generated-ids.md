# Generated Provider And Agent IDs Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Move provider and user-created agent ids to backend-generated opaque ids, migrate existing databases safely, and remove manual id inputs from desktop settings while preserving the built-in `arguswing` template id.

**Architecture:** Keep `TEXT` primary keys in SQLite, but generate ids in Rust on create instead of collecting them from users. Add forward-only SQLite migrations that rewrite existing provider and agent ids through explicit mapping tables, then update dependent references in `agents.provider_id`, `threads.provider_id`, and `jobs.agent_id`. Keep `arguswing` as a stable built-in exception, and simplify the desktop editors so create/edit flows are keyed by generated route ids instead of user-authored identifiers.

**Tech Stack:** SQLite migrations, Rust domain models and Tauri v2 commands, Next.js app router, React 19, node:test, cargo test.

---

### Task 1: Lock the new UX in failing desktop tests

**Files:**
- Modify: `crates/desktop/tests/settings-editing-flows.test.mjs`
- Modify: `crates/desktop/tests/provider-connection-flow.test.mjs`

**Step 1: Write the failing test**
- Assert `ProviderEditor` no longer renders an editable `id` field.
- Assert `AgentEditor` no longer renders an editable `id` field.
- Assert provider and agent save logic does not require a manually entered id.

**Step 2: Run test to verify it fails**
Run: `node --test crates/desktop/tests/settings-editing-flows.test.mjs crates/desktop/tests/provider-connection-flow.test.mjs`
Expected: FAIL because both editors still expose manual id inputs.

**Step 3: Write minimal implementation**
- Remove assertions that depend on manual id entry from the editors.
- Add assertions for name-first create flows.

**Step 4: Run test to verify it passes**
Run: `node --test crates/desktop/tests/settings-editing-flows.test.mjs crates/desktop/tests/provider-connection-flow.test.mjs`
Expected: PASS.

### Task 2: Make provider creation accept optional ids and generate them in Rust

**Files:**
- Modify: `crates/desktop/lib/tauri.ts`
- Modify: `crates/desktop/src-tauri/src/commands.rs`
- Modify: `crates/claw/src/db/llm.rs`
- Modify: `crates/claw/src/llm/manager.rs`
- Modify: `crates/claw/src/claw.rs` if plumbing changes are needed
- Test: `crates/desktop/src-tauri/src/commands.rs`
- Test: `crates/claw/tests/db_sqlite_llm_repository.rs`

**Step 1: Write the failing test**
- Add a Tauri command test asserting provider input without `id` converts into a domain record with a generated id.
- Add a repository/manager test asserting create preserves a generated id and update reuses an existing one.

**Step 2: Run test to verify it fails**
Run: `cargo test -q --manifest-path crates/desktop/src-tauri/Cargo.toml provider_input_converts_into_domain_record -- --nocapture`
Run: `cargo test -q -p claw --features dev sqlite_repository_round_trips_per_model_context_length -- --nocapture`
Expected: FAIL because provider ids are still required input.

**Step 3: Write minimal implementation**
- Change desktop provider input types so `id` is optional.
- In Tauri/claw provider upsert flow, generate `Uuid::new_v4().to_string()` when `id` is missing.
- Preserve existing ids during edits.

**Step 4: Run test to verify it passes**
Run the two commands above.
Expected: PASS.

### Task 3: Make agent creation accept optional ids and generate them in Rust

**Files:**
- Modify: `crates/desktop/lib/tauri.ts`
- Modify: `crates/desktop/src-tauri/src/commands.rs`
- Modify: `crates/claw/src/agents/types.rs`
- Modify: `crates/claw/src/db/sqlite/agent.rs`
- Modify: `crates/claw/src/claw.rs`
- Test: `crates/claw/tests/agent_repository_test.rs`
- Test: `crates/desktop/src-tauri/src/commands.rs`

**Step 1: Write the failing test**
- Add a desktop/Tauri test asserting agent create payloads can omit `id`.
- Add a repository test asserting non-built-in agents get generated ids on create.
- Add a regression test asserting `arguswing` still keeps `DEFAULT_AGENT_ID`.

**Step 2: Run test to verify it fails**
Run: `cargo test -q -p claw --features dev upsert_allows_empty_provider_id -- --nocapture`
Run: `cargo test -q --manifest-path crates/desktop/src-tauri/Cargo.toml chat_session_payload_serializes_effective_provider_id -- --nocapture`
Expected: FAIL or require updates because agent ids are still required input.

**Step 3: Write minimal implementation**
- Accept optional `id` on agent create payloads.
- Generate ids for user-created agents only.
- Preserve `arguswing` as the fixed built-in id.

**Step 4: Run test to verify it passes**
Run the two commands above plus any new focused test names.
Expected: PASS.

### Task 4: Add SQLite migrations that rewrite existing provider and agent ids safely

**Files:**
- Create: `crates/claw/migrations/20260317xxxxxx_generated_provider_ids.sql`
- Create: `crates/claw/migrations/20260317xxxxxx_generated_agent_ids.sql`
- Modify: `crates/claw/tests/db_sqlite_llm_repository.rs`
- Modify: `crates/claw/tests/agent_repository_test.rs`
- Modify: `crates/claw/tests/thread_repository_test.rs`
- Modify: `crates/claw/tests/job_repository_test.rs`

**Step 1: Write the failing test**
- Add migration-aware tests that seed old-style provider ids and agent ids, run migrations, and verify:
  - provider ids change
  - `agents.provider_id` tracks new provider ids
  - `threads.provider_id` tracks new provider ids
  - non-built-in agent ids change
  - `jobs.agent_id` tracks new agent ids
  - `arguswing` remains `arguswing`

**Step 2: Run test to verify it fails**
Run: `cargo test -q -p claw --features dev -- --nocapture`
Expected: FAIL because no rewrite migrations exist yet.

**Step 3: Write minimal implementation**
- Add SQL migrations that create mapping tables, rewrite ids, update dependent references, and clean up temporary state.
- Keep migrations idempotent enough for test databases created from scratch.

**Step 4: Run test to verify it passes**
Run: `cargo test -q -p claw --features dev -- --nocapture`
Expected: PASS.

### Task 5: Remove manual id fields from desktop editors and adapt save flows

**Files:**
- Modify: `crates/desktop/components/settings/provider-editor.tsx`
- Modify: `crates/desktop/components/settings/agent-editor.tsx`
- Modify: `crates/desktop/app/settings/providers/page.tsx` if list hints need adjustments
- Modify: `crates/desktop/components/settings/provider-card.tsx`
- Modify: `crates/desktop/components/settings/agent-card.tsx`

**Step 1: Write the failing test**
- Assert the provider and agent editors no longer render editable id inputs.
- Assert save buttons no longer require manual id values.
- Assert cards keep working with generated ids.

**Step 2: Run test to verify it fails**
Run: `node --test crates/desktop/tests/settings-editing-flows.test.mjs crates/desktop/tests/agent-card-display.test.mjs crates/desktop/tests/provider-connection-flow.test.mjs`
Expected: FAIL because editors still expose id fields.

**Step 3: Write minimal implementation**
- Remove the manual id inputs.
- Adjust create-mode draft defaults and save guards to use name-based required fields instead.
- Keep route-based edit behavior keyed by stored ids returned from the backend.

**Step 4: Run test to verify it passes**
Run the command above.
Expected: PASS.

### Task 6: Verify the whole desktop and backend surface

**Files:**
- Modify only if verification reveals regressions

**Step 1: Run focused Rust verification**
Run: `cargo test -q -p claw --features dev -- --nocapture`
Expected: PASS.

**Step 2: Run focused desktop tests**
Run: `node --test crates/desktop/tests/*.test.mjs`
Expected: PASS.

**Step 3: Run desktop production build**
Run: `pnpm build`
Expected: PASS in `crates/desktop`.
