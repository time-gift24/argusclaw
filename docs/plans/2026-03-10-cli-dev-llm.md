# Dev CLI for LLM Configuration and Manual Validation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `dev`-gated clap CLI for managing stored LLM providers and sending one-shot LLM completion requests for manual verification.

**Architecture:** Keep the repository private to `agent`, add dev-only passthrough methods on `LLMManager` and `Agent`, and expose a `clap` command surface from `cli` only when the `dev` feature is enabled. Parse provider imports from TOML in `cli`, route all operations through `Agent`, and keep production builds on the current startup-only path.

**Tech Stack:** Rust 2024, Tokio, clap, serde, toml, sqlx SQLite, existing ArgusClaw `Agent`/`LLMManager`

---

### Task 1: Add failing tests for new dev-only manager and CLI surfaces

**Files:**
- Modify: `crates/agent/tests/db_sqlite_llm_repository.rs`
- Modify: `crates/agent/tests/llm_manager.rs`
- Create: `crates/cli/src/dev.rs`
- Create: `crates/cli/src/dev/config.rs`

**Step 1: Write the failing test**

Add tests for:
- setting a default provider by id in the SQLite repository
- fetching provider records through `LLMManager` dev helpers
- parsing provider import TOML into one or more records
- parsing `clap` dev subcommands and argument exclusivity

**Step 2: Run test to verify it fails**

Run: `cargo test -p agent --features dev db_sqlite_llm_repository llm_manager`
Run: `cargo test -p cli --features dev`
Expected: FAIL because the `dev` feature, new repository method, and CLI modules do not exist yet

**Step 3: Write minimal implementation**

Add empty dev modules and method stubs required to compile the new tests.

**Step 4: Run test to verify it passes**

Run: `cargo test -p agent --features dev db_sqlite_llm_repository llm_manager`
Run: `cargo test -p cli --features dev`
Expected: PASS

### Task 2: Add `dev` feature plumbing in `agent` and `cli`

**Files:**
- Modify: `crates/agent/Cargo.toml`
- Modify: `crates/cli/Cargo.toml`

**Step 1: Write the failing test**

Extend the new CLI parser test to build only under `--features dev`.

**Step 2: Run test to verify it fails**

Run: `cargo test -p cli --features dev`
Expected: FAIL until `dev`-gated dependencies and modules are wired correctly

**Step 3: Write minimal implementation**

Add:
- `agent` feature: `dev = []`
- `cli` feature forwarding: `dev = ["agent/dev", "dep:clap", "dep:toml"]`
- optional `clap` and `toml` dependencies in `cli`

**Step 4: Run test to verify it passes**

Run: `cargo test -p cli --features dev`
Expected: PASS

### Task 3: Extend the repository and manager with dev-only passthrough methods

**Files:**
- Modify: `crates/agent/src/db/llm.rs`
- Modify: `crates/agent/src/db/sqlite/llm.rs`
- Modify: `crates/agent/src/error.rs`
- Modify: `crates/agent/src/llm/manager.rs`
- Modify: `crates/agent/src/agent.rs`

**Step 1: Write the failing test**

Add coverage for:
- `set_default_provider(id)` updating the default row transactionally
- `LLMManager::get_provider_record`
- `LLMManager::get_default_provider_record`
- `LLMManager::upsert_provider`
- `LLMManager::import_providers`

**Step 2: Run test to verify it fails**

Run: `cargo test -p agent --features dev --test db_sqlite_llm_repository --test llm_manager`
Expected: FAIL with missing repository and manager methods

**Step 3: Write minimal implementation**

Implement:
- repository trait method `set_default_provider`
- SQLite transaction logic for default reassignment
- dev-only `LLMManager` passthrough methods
- matching `Agent` passthrough methods
- any new error variants needed for not-found/default-selection flows

**Step 4: Run test to verify it passes**

Run: `cargo test -p agent --features dev --test db_sqlite_llm_repository --test llm_manager`
Expected: PASS

### Task 4: Add TOML import models and clap command parsing

**Files:**
- Create: `crates/cli/src/dev.rs`
- Create: `crates/cli/src/dev/config.rs`
- Modify: `crates/cli/src/main.rs`

**Step 1: Write the failing test**

Add tests for:
- parsing the TOML `[[providers]]` structure into import models
- parsing `provider import`, `provider upsert`, `provider set-default`, `provider get-default`, and `llm complete`
- enforcing `--provider` xor `--default` for `llm complete`

**Step 2: Run test to verify it fails**

Run: `cargo test -p cli --features dev`
Expected: FAIL because clap and TOML support are not wired into `main.rs`

**Step 3: Write minimal implementation**

Implement:
- a dev CLI parser module using `clap`
- TOML import structs and conversion into `agent::db::llm::LlmProviderRecord`
- a `main.rs` path that runs dev commands when a subcommand is present and otherwise preserves startup behavior

**Step 4: Run test to verify it passes**

Run: `cargo test -p cli --features dev`
Expected: PASS

### Task 5: Implement command handlers for provider management and direct completion

**Files:**
- Modify: `crates/cli/src/dev.rs`
- Modify: `crates/cli/src/main.rs`
- Modify: `crates/agent/src/llm/manager.rs`
- Modify: `crates/agent/src/agent.rs`

**Step 1: Write the failing test**

Add coverage for:
- `provider list/get/get-default` rendering without API keys
- `provider import` upserting multiple providers
- `llm complete` building a one-shot completion request through `Agent`

**Step 2: Run test to verify it fails**

Run: `cargo test -p cli --features dev`
Expected: FAIL until command handlers are connected to `Agent`

**Step 3: Write minimal implementation**

Implement:
- provider command handlers using `Agent`
- one-shot text completion helper using `CompletionRequest::new(vec![ChatMessage::user(prompt)])`
- human-readable stdout formatting for provider summaries/records and completion text

**Step 4: Run test to verify it passes**

Run: `cargo test -p cli --features dev`
Expected: PASS

### Task 6: Final verification

**Files:**
- Modify: any touched files needed to satisfy formatting or lints

**Step 1: Run formatting**

Run: `cargo fmt`
Expected: exit 0

**Step 2: Run agent and cli tests with dev enabled**

Run: `cargo test -p agent --features dev,openai-compatible`
Run: `cargo test -p cli --features dev`
Expected: PASS

**Step 3: Run workspace verification**

Run: `cargo test --workspace --all-features`
Run: `cargo clippy --workspace --all-targets --all-features`
Expected: PASS
