# Agent Provider Model Binding Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Allow agent templates to optionally bind to a concrete provider/model pair, default chat sessions to that binding, and keep temporary chat model switching as a session-level override.

**Architecture:** Add nullable `agents.model` storage, thread the optional model through `AgentRecord`, Tauri DTOs, and desktop state, then resolve an effective provider/model pair before building runtime agents. Keep runtime fallback behavior for unbound templates such as `arguswing`, and include model override in chat session identity so different temporary model choices do not reuse the same runtime session.

**Tech Stack:** SQLite migrations, Rust domain/repository code, Tauri v2 commands, Next.js app router, React 19, Zustand, node:test, cargo test.

---

### Task 1: Lock the new agent editor and chat-store behavior in failing tests

**Files:**
- Modify: `crates/desktop/tests/settings-editing-flows.test.mjs`
- Modify: `crates/desktop/tests/chat-store-session-model.test.mjs`
- Modify: `crates/desktop/tests/chat-tauri-bindings.test.mjs`

**Step 1: Write the failing test**
- Assert `AgentEditor` exposes both provider and model fields.
- Assert save logic requires provider/model to be both empty or both set.
- Assert chat store session keys include model override.

**Step 2: Run test to verify it fails**
Run: `node --test crates/desktop/tests/settings-editing-flows.test.mjs crates/desktop/tests/chat-store-session-model.test.mjs crates/desktop/tests/chat-tauri-bindings.test.mjs`
Expected: FAIL because agent templates still only store provider and session keys ignore model override.

**Step 3: Write minimal implementation**
- Update test expectations only for the intended new UI and store behavior.

**Step 4: Run test to verify it passes**
Run the same command.
Expected: PASS.

### Task 2: Add agent model storage to SQLite and repository round-trips

**Files:**
- Create: `crates/claw/migrations/2026031712xxxx_add_agent_model.sql`
- Modify: `crates/claw/src/agents/types.rs`
- Modify: `crates/claw/src/db/sqlite/agent.rs`
- Modify: `crates/claw/tests/agent_repository_test.rs`
- Modify: `crates/claw/tests/generated_id_migrations.rs`

**Step 1: Write the failing test**
- Add repository coverage that persists and reads `model`.
- Ensure the generated-id migration staging test includes the new migration file.

**Step 2: Run test to verify it fails**
Run: `cargo test -q -p claw --features dev --test agent_repository_test -- --nocapture`
Run: `cargo test -q -p claw --features dev --test generated_id_migrations -- --nocapture`
Expected: FAIL because `agents` has no `model` column yet.

**Step 3: Write minimal implementation**
- Add nullable `model` column.
- Add `model: Option<String>` to `AgentRecord`.
- Read/write `model` in the SQLite repository.

**Step 4: Run test to verify it passes**
Run the two commands above.
Expected: PASS.

### Task 3: Thread agent model through Tauri DTOs and desktop bindings

**Files:**
- Modify: `crates/desktop/lib/tauri.ts`
- Modify: `crates/desktop/src-tauri/src/commands.rs`
- Modify: `crates/desktop/tests/chat-tauri-bindings.test.mjs`

**Step 1: Write the failing test**
- Add command-layer tests asserting `AgentInput` preserves optional `model`.
- Assert chat payload shape still includes `effective_provider_id` and `effective_model`.

**Step 2: Run test to verify it fails**
Run: `cargo test -q --manifest-path crates/desktop/src-tauri/Cargo.toml agent_input_generates_an_id_when_create_payload_omits_it -- --nocapture`
Expected: FAIL or require updates because `model` is missing from command DTOs.

**Step 3: Write minimal implementation**
- Add optional `model` to desktop and Tauri agent DTOs.
- Preserve id-generation behavior.

**Step 4: Run test to verify it passes**
Run the focused Tauri test command(s).
Expected: PASS.

### Task 4: Validate and resolve effective provider/model in Rust runtime creation

**Files:**
- Modify: `crates/claw/src/claw.rs`
- Modify: `crates/claw/src/agents/agent/manager.rs`
- Modify: `crates/claw/src/protocol/runtime_agent.rs`
- Modify: `crates/claw/src/llm/manager.rs` if helper reuse is needed
- Modify: `crates/claw/tests/llm_manager.rs`
- Modify: `crates/desktop/src-tauri/src/commands.rs`

**Step 1: Write the failing test**
- Add validation tests for invalid provider/model combinations.
- Add runtime creation coverage asserting template-bound model becomes the returned effective model.

**Step 2: Run test to verify it fails**
Run: `cargo test -q -p claw --features dev -- --nocapture`
Expected: FAIL because runtime creation still binds the provider default model.

**Step 3: Write minimal implementation**
- Resolve effective provider/model before runtime-agent creation.
- When a model is selected, build provider instances with `get_provider_with_model`.
- Return `effective_model` from `RuntimeAgentHandle` and use it directly in Tauri chat payload construction.
- Keep fallback to app default provider/model for unbound templates.

**Step 4: Run test to verify it passes**
Run the same cargo test command.
Expected: PASS.

### Task 5: Update the desktop agent editor and cards to edit/display concrete model bindings

**Files:**
- Modify: `crates/desktop/components/settings/agent-editor.tsx`
- Modify: `crates/desktop/components/settings/agent-card.tsx`
- Modify: `crates/desktop/tests/settings-editing-flows.test.mjs`
- Modify: `crates/desktop/tests/agent-card-display.test.mjs`

**Step 1: Write the failing test**
- Assert provider selector and model selector both exist.
- Assert switching provider resets the model to that provider's default.
- Assert cards render `Provider / Model` when configured.

**Step 2: Run test to verify it fails**
Run: `node --test crates/desktop/tests/settings-editing-flows.test.mjs crates/desktop/tests/agent-card-display.test.mjs`
Expected: FAIL because the editor only exposes provider and cards only display provider.

**Step 3: Write minimal implementation**
- Add model selector scoped to selected provider.
- Allow clearing back to runtime fallback.
- Update card display for provider/model pair.

**Step 4: Run test to verify it passes**
Run the same node test command.
Expected: PASS.

### Task 6: Make chat session identity and selectors honor model override

**Files:**
- Modify: `crates/desktop/lib/chat-store.ts`
- Modify: `crates/desktop/components/assistant-ui/provider-selector.tsx`
- Modify: `crates/desktop/components/assistant-ui/thread.tsx`
- Modify: `crates/desktop/tests/chat-store-session-model.test.mjs`

**Step 1: Write the failing test**
- Assert session keys include model override.
- Assert provider selector display falls back to the active session's effective provider/model.

**Step 2: Run test to verify it fails**
Run: `node --test crates/desktop/tests/chat-store-session-model.test.mjs`
Expected: FAIL because the store currently keys only by template and provider preference.

**Step 3: Write minimal implementation**
- Include model override in session identity.
- Keep temporary provider/model switching creating distinct runtime sessions.
- Ensure context ring reads the effective session model.

**Step 4: Run test to verify it passes**
Run the same node test command.
Expected: PASS.

### Task 7: Run end-to-end verification

**Files:**
- Modify only if verification reveals regressions

**Step 1: Run focused Rust verification**
Run: `cargo test -q -p claw --features dev -- --nocapture`
Expected: PASS.

**Step 2: Run focused desktop verification**
Run: `node --test crates/desktop/tests/*.test.mjs`
Expected: PASS.

**Step 3: Run desktop production build**
Run: `pnpm build`
Expected: PASS in `crates/desktop`.
