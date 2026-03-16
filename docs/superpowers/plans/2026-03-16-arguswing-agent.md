# ArgusWing Default Agent Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a default `arguswing` agent defined in TOML, embedded at compile time, and upserted at runtime.

**Architecture:** Agent definition lives in `agents/arguswing.toml`, embedded via `include_str!` in `crates/claw/src/agents/builtins.rs`, parsed at runtime and upserted during `AppContext::init()`. Desktop frontend uses new Tauri commands to create runtime agent instances.

**Tech Stack:** Rust, TOML, sqlx, Tauri

---

## Chunk 1: Core Implementation (claw crate)

### Task 1: Add toml dependency

**Files:**
- Modify: `crates/claw/Cargo.toml`

- [ ] **Step 1: Add toml crate to dependencies**

Add `toml` to the dependencies section:

```toml
toml = "0.8"
```

- [ ] **Step 2: Verify build**

Run: `cargo check -p claw`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add crates/claw/Cargo.toml
git commit -m "chore: add toml dependency for agent config parsing"
```

---

### Task 2: Create agent definition file

**Files:**
- Create: `agents/arguswing.toml`

- [ ] **Step 1: Create agents directory and TOML file**

```toml
id = "arguswing"
display_name = "ArgusWing"
description = "Default assistant for ArgusWing"
version = "0.1.0"
system_prompt = "You are ArgusWing, a helpful AI assistant."
tool_names = ["shell", "read", "grep", "glob"]
```

- [ ] **Step 2: Verify file exists**

Run: `cat agents/arguswing.toml`
Expected: File contents displayed

- [ ] **Step 3: Commit**

```bash
git add agents/arguswing.toml
git commit -m "feat: add arguswing agent definition file"
```

---

### Task 3: Add error variants

**Files:**
- Modify: `crates/claw/src/error.rs`

- [ ] **Step 1: Add new error variants**

Add at the end of the `AgentError` enum (after `ThreadBuildFailed`):

```rust
    #[error("failed to load built-in agent: {reason}")]
    BuiltinAgentLoadFailed { reason: String },

    #[error("default agent not found")]
    DefaultAgentNotFound,
```

- [ ] **Step 2: Verify build**

Run: `cargo check -p claw`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add crates/claw/src/error.rs
git commit -m "feat: add BuiltinAgentLoadFailed and DefaultAgentNotFound errors"
```

---

### Task 4: Create builtins module

**Files:**
- Create: `crates/claw/src/agents/builtins.rs`
- Modify: `crates/claw/src/agents/mod.rs`

- [ ] **Step 1: Create builtins.rs with TOML parsing**

```rust
//! Built-in agent definitions embedded at compile time.

use super::types::{AgentId, AgentRecord};

/// Default ArgusWing agent definition embedded at compile time.
const ARGUSWING_TOML: &str = include_str!("../../../agents/arguswing.toml");

/// Load the built-in ArgusWing agent record.
///
/// # Errors
///
/// Returns an error if the embedded TOML is malformed or missing required fields.
pub fn load_arguswing() -> Result<AgentRecord, toml::de::Error> {
    #[derive(serde::Deserialize)]
    struct AgentDef {
        id: String,
        display_name: String,
        description: String,
        version: String,
        system_prompt: String,
        tool_names: Vec<String>,
    }

    let def: AgentDef = toml::from_str(ARGUSWING_TOML)?;
    Ok(AgentRecord {
        id: AgentId::new(def.id),
        display_name: def.display_name,
        description: def.description,
        version: def.version,
        provider_id: String::new(),
        system_prompt: def.system_prompt,
        tool_names: def.tool_names,
        max_tokens: None,
        temperature: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_arguswing_parses_embedded_toml() {
        let agent = load_arguswing().expect("embedded TOML should parse");
        assert_eq!(agent.id.as_ref(), "arguswing");
        assert_eq!(agent.display_name, "ArgusWing");
        assert_eq!(agent.tool_names, vec!["shell", "read", "grep", "glob"]);
        assert!(agent.provider_id.is_empty());
    }
}
```

- [ ] **Step 2: Add builtins module to mod.rs**

Add at line 4 (after `pub mod agent;`):

```rust
pub(crate) mod builtins;
```

Note: `builtins` is crate-internal; external consumers (cli/desktop) use `AppContext` API only.

- [ ] **Step 3: Verify build and test**

Run: `cargo test -p claw builtins::tests`
Expected: Test passes

- [ ] **Step 4: Commit**

```bash
git add crates/claw/src/agents/builtins.rs crates/claw/src/agents/mod.rs
git commit -m "feat: add builtins module for compile-time agent definitions"
```

---

### Task 5: Add initialization and public API to AppContext

**Files:**
- Modify: `crates/claw/src/claw.rs`

- [ ] **Step 1: Add import for builtins**

Add in the imports section (at the end of the `use crate::agents::...` block):

```rust
use crate::agents::builtins::load_arguswing;
```

- [ ] **Step 2: Add DEFAULT_AGENT_ID constant**

Add after the imports section (around line 26):

```rust
/// The ID of the default built-in agent.
pub const DEFAULT_AGENT_ID: &str = "arguswing";
```

- [ ] **Step 3: Add ensure_default_agent function**

Add before `impl AppContext` (around line 36):

```rust
/// Ensures the default ArgusWing agent exists in the database.
async fn ensure_default_agent(agent_manager: &AgentManager) -> Result<(), AgentError> {
    let default_agent = load_arguswing().map_err(|e| AgentError::BuiltinAgentLoadFailed {
        reason: e.to_string(),
    })?;
    agent_manager.upsert_template(default_agent).await?;
    Ok(())
}
```

- [ ] **Step 4: Call ensure_default_agent in init**

Add after `AgentManager::new(...)` call (around line 61, before scheduler creation):

```rust
        // Ensure default agent exists
        ensure_default_agent(&agent_manager).await?;
```

- [ ] **Step 5: Add get_default_agent_template method**

Add in `impl AppContext` after `delete_template` method (around line 250):

```rust
    /// Get the default ArgusWing agent template.
    ///
    /// This agent is guaranteed to exist after `AppContext::init()`.
    pub async fn get_default_agent_template(&self) -> Result<AgentRecord, AgentError> {
        self.get_template(&AgentId::new(DEFAULT_AGENT_ID))
            .await?
            .ok_or(AgentError::DefaultAgentNotFound)
    }

    /// Create a runtime agent from the default template.
    ///
    /// Binds to the default LLM provider at runtime.
    ///
    /// # Errors
    ///
    /// Returns `DefaultProviderNotConfigured` if no default provider is set.
    pub async fn create_default_agent(&self) -> Result<AgentId, AgentError> {
        let template = self.get_default_agent_template().await?;
        let default_provider = self.get_default_provider_record().await?;
        let mut record = template;
        record.provider_id = default_provider.id.to_string();
        self.agent_manager.create_agent(&record).await
    }
```

- [ ] **Step 6: Verify build**

Run: `cargo check -p claw`
Expected: Compiles without errors

- [ ] **Step 7: Run tests**

Run: `cargo test -p claw`
Expected: All tests pass

- [ ] **Step 8: Commit**

```bash
git add crates/claw/src/claw.rs
git commit -m "feat: add default agent initialization and public API"
```

---

### Task 6: Export DEFAULT_AGENT_ID in lib.rs

**Files:**
- Modify: `crates/claw/src/lib.rs`

- [ ] **Step 1: Add constant export**

Add after line 29 (after `pub use claw::AppContext;`):

```rust
// Default agent ID
pub use claw::DEFAULT_AGENT_ID;
```

- [ ] **Step 2: Verify build**

Run: `cargo check -p claw`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add crates/claw/src/lib.rs
git commit -m "feat: export DEFAULT_AGENT_ID constant"
```

---

## Chunk 2: Desktop Frontend Integration

### Task 7: Add Tauri commands

**Files:**
- Modify: `crates/desktop/src-tauri/src/commands.rs`

- [ ] **Step 1: Add get_default_agent_template command**

Add after `delete_agent_template` function (around line 228):

```rust
#[tauri::command]
pub async fn get_default_agent_template(
    ctx: State<'_, Arc<AppContext>>,
) -> Result<AgentRecord, String> {
    ctx.get_default_agent_template()
        .await
        .map_err(|e| e.to_string())
}
```

- [ ] **Step 2: Add create_default_agent command**

Add after `get_default_agent_template`:

```rust
#[tauri::command]
pub async fn create_default_agent(
    ctx: State<'_, Arc<AppContext>>,
) -> Result<String, String> {
    let agent_id = ctx
        .create_default_agent()
        .await
        .map_err(|e| e.to_string())?;
    Ok(agent_id.to_string())
}
```

- [ ] **Step 3: Verify build**

Run: `cargo check -p arguswing-desktop`
Expected: Compiles without errors

- [ ] **Step 4: Commit**

```bash
git add crates/desktop/src-tauri/src/commands.rs
git commit -m "feat: add default agent Tauri commands"
```

---

### Task 8: Register commands in Tauri builder

**Files:**
- Modify: `crates/desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Register new commands in invoke_handler**

Add to the `invoke_handler` list (after `commands::delete_agent_template`):

```rust
            commands::get_default_agent_template,
            commands::create_default_agent,
```

The full invoke_handler should look like:

```rust
        .invoke_handler(tauri::generate_handler![
            commands::list_providers,
            commands::get_provider,
            commands::upsert_provider,
            commands::delete_provider,
            commands::set_default_provider,
            commands::test_provider_connection,
            commands::test_provider_input,
            commands::list_agent_templates,
            commands::get_agent_template,
            commands::upsert_agent_template,
            commands::delete_agent_template,
            commands::get_default_agent_template,
            commands::create_default_agent,
        ])
```

- [ ] **Step 2: Verify build**

Run: `cargo check -p arguswing-desktop`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add crates/desktop/src-tauri/src/lib.rs
git commit -m "feat: register default agent commands in Tauri builder"
```

---

## Chunk 3: Integration Testing

### Task 9: Add integration test

**Files:**
- Modify: `crates/claw/src/claw.rs` (add test to existing tests module)

- [ ] **Step 1: Add test for default agent initialization**

Add to the `#[cfg(test)] mod tests` section:

```rust
    #[tokio::test]
    async fn init_creates_default_arguswing_agent() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("sqlite.db");

        let ctx = AppContext::init(Some(database_path.display().to_string()))
            .await
            .expect("app context init should succeed");

        let default_agent = ctx
            .get_default_agent_template()
            .await
            .expect("default agent should exist");

        assert_eq!(default_agent.id.as_ref(), "arguswing");
        assert_eq!(default_agent.display_name, "ArgusWing");
        assert!(default_agent.provider_id.is_empty());
    }
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p claw init_creates_default_arguswing_agent`
Expected: Test passes

- [ ] **Step 3: Commit**

```bash
git add crates/claw/src/claw.rs
git commit -m "test: add integration test for default agent initialization"
```

---

### Task 10: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test -p claw`
Expected: All tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -p claw -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Run full prek check**

Run: `prek`
Expected: All checks pass

- [ ] **Step 4: Verify desktop builds**

Run: `cargo check -p arguswing-desktop`
Expected: Compiles without errors

---

## Summary

| Task | Description | Files |
|------|-------------|-------|
| 1 | Add toml dependency | `Cargo.toml` |
| 2 | Create agent TOML | `agents/arguswing.toml` |
| 3 | Add error variants | `error.rs` |
| 4 | Create builtins module | `builtins.rs`, `mod.rs` |
| 5 | Add init and API | `claw.rs` |
| 6 | Export constant | `lib.rs` |
| 7 | Add Tauri commands | `commands.rs` |
| 8 | Register commands | `lib.rs` (desktop) |
| 9 | Add integration test | `claw.rs` |
| 10 | Final verification | - |
