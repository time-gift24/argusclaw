# Agent to Claw Refactoring Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Rename agent crate to claw, Agent struct to AppContext, and add agents module with AgentManager placeholder.

**Architecture:** Simple refactoring - rename crate/directory, rename struct, add new empty module. No behavior changes.

**Tech Stack:** Rust, Cargo workspace

---

## Prerequisites

- Read the design doc: `docs/plans/2026-03-10-agent-to-claw-refactoring.md`

---

### Task 1: Rename crate directory

**Files:**
- Rename: `crates/agent/` → `crates/claw/`

**Step 1: Rename the directory**

Run:
```bash
mv crates/agent crates/claw
```

**Step 2: Verify directory moved**

Run:
```bash
ls crates/claw/
```
Expected: `Cargo.toml  migrations/  src/  tests/`

**Step 3: Commit**

```bash
git add -A
git commit -m "refactor: rename agent crate directory to claw"
```

---

### Task 2: Update claw crate Cargo.toml

**Files:**
- Modify: `crates/claw/Cargo.toml:2`

**Step 1: Update package name**

Change line 2 from `name = "agent"` to `name = "claw"`:

```toml
[package]
name = "claw"
version = "0.1.0"
edition = "2024"
```

**Step 2: Verify Cargo recognizes the change**

Run:
```bash
cargo check -p claw
```
Expected: Compilation errors about `agent` imports in cli (expected)

**Step 3: Commit**

```bash
git add crates/claw/Cargo.toml
git commit -m "refactor: rename package from agent to claw"
```

---

### Task 3: Create agents module with AgentManager

**Files:**
- Create: `crates/claw/src/agents/mod.rs`

**Step 1: Create agents/mod.rs**

```rust
#[derive(Clone, Default)]
pub struct AgentManager {}

impl AgentManager {
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }
}
```

**Step 2: Commit**

```bash
git add crates/claw/src/agents/mod.rs
git commit -m "feat(claw): add agents module with AgentManager placeholder"
```

---

### Task 4: Rename agent.rs to claw.rs

**Files:**
- Rename: `crates/claw/src/agent.rs` → `crates/claw/src/claw.rs`

**Step 1: Rename the file**

Run:
```bash
mv crates/claw/src/agent.rs crates/claw/src/claw.rs
```

**Step 2: Commit**

```bash
git add -A
git commit -m "refactor(claw): rename agent.rs to claw.rs"
```

---

### Task 5: Update claw.rs - rename Agent to AppContext

**Files:**
- Modify: `crates/claw/src/claw.rs`

**Step 1: Update imports and add AgentManager import**

Replace the top imports section:

```rust
use std::sync::Arc;
use std::{env, path::Path, path::PathBuf};

#[cfg(feature = "dev")]
use crate::db::llm::{LlmProviderId, LlmProviderRecord};
use crate::agents::AgentManager;
use crate::db::sqlite::{SqliteLlmProviderRepository, connect, connect_path, migrate};
use crate::error::AgentError;
use crate::llm::LLMManager;
#[cfg(feature = "dev")]
use crate::llm::LlmEventStream;
```

**Step 2: Rename struct and add agent_manager field**

Replace the struct definition:

```rust
#[derive(Clone)]
pub struct AppContext {
    llm_manager: Arc<LLMManager>,
    #[allow(dead_code)]
    agent_manager: Arc<AgentManager>,
}
```

**Step 3: Update impl block - rename and update methods**

Replace the entire impl block:

```rust
impl AppContext {
    pub async fn init(database_target: Option<String>) -> Result<Self, AgentError> {
        let database_target = resolve_database_target(database_target)?;
        let pool = match &database_target {
            DatabaseTarget::Url(database_url) => connect(database_url).await,
            DatabaseTarget::Path(path) => {
                ensure_parent_dir(path)?;
                connect_path(path).await
            }
        }?;
        migrate(&pool).await?;

        let repository = Arc::new(SqliteLlmProviderRepository::new(pool));
        let llm_manager = Arc::new(LLMManager::new(repository));
        let agent_manager = Arc::new(AgentManager::new());

        Ok(Self::new(llm_manager, agent_manager))
    }

    #[must_use]
    pub fn new(llm_manager: Arc<LLMManager>, agent_manager: Arc<AgentManager>) -> Self {
        Self { llm_manager, agent_manager }
    }

    #[must_use]
    pub fn llm_manager(&self) -> Arc<LLMManager> {
        Arc::clone(&self.llm_manager)
    }

    #[cfg(feature = "dev")]
    pub async fn upsert_provider(&self, record: LlmProviderRecord) -> Result<(), AgentError> {
        self.llm_manager.upsert_provider(record).await
    }

    #[cfg(feature = "dev")]
    pub async fn import_providers(
        &self,
        records: Vec<LlmProviderRecord>,
    ) -> Result<(), AgentError> {
        self.llm_manager.import_providers(records).await
    }

    #[cfg(feature = "dev")]
    pub async fn get_provider_record(
        &self,
        id: &LlmProviderId,
    ) -> Result<LlmProviderRecord, AgentError> {
        self.llm_manager.get_provider_record(id).await
    }

    #[cfg(feature = "dev")]
    pub async fn get_default_provider_record(&self) -> Result<LlmProviderRecord, AgentError> {
        self.llm_manager.get_default_provider_record().await
    }

    #[cfg(feature = "dev")]
    pub async fn set_default_provider(&self, id: &LlmProviderId) -> Result<(), AgentError> {
        self.llm_manager.set_default_provider(id).await
    }

    #[cfg(feature = "dev")]
    pub async fn complete_text(
        &self,
        provider_id: Option<&LlmProviderId>,
        prompt: impl Into<String>,
    ) -> Result<String, AgentError> {
        self.llm_manager.complete_text(provider_id, prompt).await
    }

    #[cfg(feature = "dev")]
    pub async fn stream_text(
        &self,
        provider_id: Option<&LlmProviderId>,
        prompt: impl Into<String>,
    ) -> Result<LlmEventStream, AgentError> {
        self.llm_manager.stream_text(provider_id, prompt).await
    }
}
```

**Step 4: Update tests to use AppContext**

Replace the tests module:

```rust
#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{AppContext, expand_home_path, resolve_database_target};

    #[test]
    fn resolve_database_target_keeps_sqlite_urls() {
        let target = resolve_database_target(Some("sqlite::memory:".to_string()))
            .expect("sqlite urls should resolve");

        assert!(matches!(target, super::DatabaseTarget::Url(url) if url == "sqlite::memory:"));
    }

    #[test]
    fn expand_home_path_resolves_tilde_prefix() {
        let path = expand_home_path("~/.argusclaw/sqlite.db").expect("home path should resolve");

        assert!(path.ends_with(".argusclaw/sqlite.db"));
    }

    #[tokio::test]
    async fn init_creates_an_app_context_from_a_filesystem_database_path() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("nested").join("sqlite.db");

        let ctx = AppContext::init(Some(database_path.display().to_string()))
            .await
            .expect("app context init should succeed");
        let providers = ctx
            .llm_manager()
            .list_providers()
            .await
            .expect("provider list should succeed");

        assert!(providers.is_empty());
        assert!(database_path.exists());
    }
}
```

**Step 5: Verify claw crate compiles**

Run:
```bash
cargo check -p claw
```
Expected: No errors

**Step 6: Commit**

```bash
git add crates/claw/src/claw.rs
git commit -m "refactor(claw): rename Agent to AppContext, add AgentManager"
```

---

### Task 6: Update lib.rs exports

**Files:**
- Modify: `crates/claw/src/lib.rs`

**Step 1: Update module declarations and exports**

Replace entire file:

```rust
pub mod agents;
pub mod claw;
pub mod db;
pub mod error;
pub mod llm;

pub use claw::AppContext;
pub use error::AgentError;
```

**Step 2: Verify claw crate compiles**

Run:
```bash
cargo check -p claw
```
Expected: No errors

**Step 3: Run claw tests**

Run:
```bash
cargo test -p claw
```
Expected: All tests pass

**Step 4: Commit**

```bash
git add crates/claw/src/lib.rs
git commit -m "refactor(claw): update lib.rs to export AppContext and agents module"
```

---

### Task 7: Update workspace Cargo.toml

**Files:**
- Modify: `Cargo.toml:2`

**Step 1: Update workspace members**

Change line 2 from `members = ["crates/agent", "crates/cli"]` to:

```toml
members = ["crates/claw", "crates/cli"]
```

**Step 2: Verify workspace resolves**

Run:
```bash
cargo check
```
Expected: Only cli import errors (expected)

**Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "refactor: update workspace members for claw rename"
```

---

### Task 8: Update cli Cargo.toml dependency

**Files:**
- Modify: `crates/cli/Cargo.toml`

**Step 1: Update agent dependency to claw**

Find the `agent` dependency and rename to `claw`:

```toml
claw = { path = "../claw" }
```

**Step 2: Verify dependency resolves**

Run:
```bash
cargo check -p cli
```
Expected: Only import errors in main.rs (expected)

**Step 3: Commit**

```bash
git add crates/cli/Cargo.toml
git commit -m "refactor(cli): update dependency from agent to claw"
```

---

### Task 9: Update cli main.rs imports

**Files:**
- Modify: `crates/cli/src/main.rs`

**Step 1: Update imports**

Change `use agent::Agent;` to:

```rust
use claw::AppContext;
```

**Step 2: Update variable names**

Change `Agent` references to `AppContext`:

```rust
let ctx = AppContext::init(env::var("DATABASE_URL").ok()).await?;

#[cfg(feature = "dev")]
if dev::try_run(ctx.clone()).await? {
    return Ok(());
}

let provider_count = ctx.llm_manager().list_providers().await?.len();

tracing::info!(provider_count, "argusclaw initialized");
```

Full updated main.rs:

```rust
use std::env;

#[cfg(feature = "dev")]
mod dev;

use claw::AppContext;
use anyhow::Result;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let ctx = AppContext::init(env::var("DATABASE_URL").ok()).await?;

    #[cfg(feature = "dev")]
    if dev::try_run(ctx.clone()).await? {
        return Ok(());
    }

    let provider_count = ctx.llm_manager().list_providers().await?.len();

    tracing::info!(provider_count, "argusclaw initialized");

    Ok(())
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("argusclaw=info,claw=info"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .compact()
        .init();
}
```

**Step 3: Verify full project compiles**

Run:
```bash
cargo check
```
Expected: No errors

**Step 4: Commit**

```bash
git add crates/cli/src/main.rs
git commit -m "refactor(cli): update imports to use AppContext from claw"
```

---

### Task 10: Update CLAUDE.md documentation

**Files:**
- Modify: `CLAUDE.md`

**Step 1: Update Project Structure section**

Replace the Project Structure section to reflect new names:

```text
## Project Structure

crates/
├── claw/
│   ├── src/
│   │   ├── lib.rs                    # Library root, module declarations and exports
│   │   ├── error.rs                  # Top-level error types
│   │   ├── claw.rs                   # AppContext; owns LLMManager, AgentManager
│   │   ├── agents/                   # Agent management
│   │   │   └── mod.rs                # AgentManager (placeholder)
│   │   ├── db/                       # Storage abstractions and implementations
│   │   │   ├── mod.rs                # DB module entry point and shared DB errors
│   │   │   ├── llm.rs                # LLM provider records and repository trait
│   │   │   └── sqlite/               # SQLx-backed SQLite implementation
│   │   │       ├── mod.rs            # SQLite connect/migrate helpers
│   │   │       └── llm.rs            # SQLite LLM provider repository
│   │   └── llm/                      # LLM domain types, manager, and provider implementations
│   │       ├── mod.rs                # LLM module entry point and re-exports
│   │       ├── error.rs              # Provider-agnostic LLM errors
│   │       ├── manager.rs            # LLMManager: list providers and build provider instances
│   │       ├── provider.rs           # Core LlmProvider trait and request/response types
│   │       ├── retry.rs              # Retry wrapper for LlmProvider
│   │       ├── secret.rs             # Host-bound API key encryption/decryption
│   │       └── providers/            # Concrete provider implementations
│   │           ├── mod.rs            # Provider module exports
│   │           └── openai_compatible.rs # OpenAI-compatible provider factory and implementation
│   ├── migrations/                   # SQLx migrations
│   └── tests/                        # E2E tests only; multi-module scenarios that do not fit inline tests
└── cli/
    ├── CLAUDE.md                      # CLI module guide
    └── src/
        ├── main.rs                    # CLI bootstrap: tracing, DB init, AppContext startup
        ├── dev.rs                     # Dev-only commands (behind `dev` feature)
        └── dev/
            └── config.rs              # Provider import TOML format
```

**Step 2: Update any other references to agent/Agent**

Look for and update any remaining references to `agent` crate or `Agent` struct in comments.

**Step 3: Verify documentation is accurate**

Read the updated file to confirm it matches the new structure.

**Step 4: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md for agent-to-claw refactoring"
```

---

### Task 11: Final verification

**Step 1: Run all tests**

Run:
```bash
cargo test
```
Expected: All tests pass

**Step 2: Run clippy**

Run:
```bash
cargo clippy --all --benches --tests --examples --all-features
```
Expected: Zero warnings

**Step 3: Run format check**

Run:
```bash
cargo fmt --check
```
Expected: No output (all formatted)

**Step 4: Verify git status is clean**

Run:
```bash
git status
```
Expected: nothing to commit, working tree clean

---

## Summary

After completing all tasks:
- `crates/agent/` renamed to `crates/claw/`
- `Agent` struct renamed to `AppContext`
- New `agents` module with `AgentManager` placeholder
- All imports and documentation updated
- All tests pass, no clippy warnings
