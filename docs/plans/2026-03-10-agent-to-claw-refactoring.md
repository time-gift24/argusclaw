# Agent to Claw Refactoring Design

## Summary

Rename the `agent` crate to `claw`, rename `Agent` struct to `AppContext`, and add a new `agents` module for future `AgentManager`.

## Motivation

- `AppContext` better represents the role as application-wide shared context
- `claw` as crate name distinguishes the library from agent-related concepts
- `agents` module prepares for future agent management capabilities

## Changes

### Naming

| Original | New |
|----------|-----|
| `crates/agent/` | `crates/claw/` |
| crate name `agent` | `claw` |
| `agent.rs` | `claw.rs` |
| `Agent` struct | `AppContext` |

### Directory Structure

```text
crates/claw/src/
├── lib.rs          # pub mod agents; pub mod claw; ...
├── claw.rs         # pub struct AppContext { ... }
├── agents/
│   └── mod.rs      # pub struct AgentManager {} // empty placeholder
├── db/             # unchanged
├── llm/            # unchanged
└── error.rs        # unchanged
```

### AppContext Structure

```rust
pub struct AppContext {
    llm_manager: Arc<LLMManager>,
    agent_manager: Arc<AgentManager>,  // new field
}
```

### Module Hierarchy

```
AppContext (claw.rs)
    ├── LLMManager (llm/)
    └── AgentManager (agents/)  // placeholder for future agent logic
```

### CLI Usage

```rust
// crates/cli/src/main.rs
use claw::AppContext;

let ctx = AppContext::init(env::var("DATABASE_URL").ok()).await?;
```

## Files to Modify

1. Rename directory: `crates/agent/` → `crates/claw/`
2. Update `crates/claw/Cargo.toml` name to `claw`
3. Rename `src/agent.rs` → `src/claw.rs`
4. Create `src/agents/mod.rs` with empty `AgentManager`
5. Update `src/lib.rs` exports
6. Update root `Cargo.toml` workspace members
7. Update `crates/cli/Cargo.toml` dependency
8. Update `crates/cli/src/main.rs` imports
9. Update `CLAUDE.md` documentation

## AgentManager Placeholder

The `agents` module will contain an empty `AgentManager` struct:

```rust
// crates/claw/src/agents/mod.rs
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct AgentManager {}

impl AgentManager {
    pub fn new() -> Self {
        Self {}
    }
}
```

This will be expanded in future work to manage agent instances and their lifecycles.
