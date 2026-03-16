# ArgusWing Default Agent Design

**Date:** 2026-03-16
**Status:** Approved
**Scope:** Backend implementation for seeding and managing the default `arguswing` agent

## Overview

Add a default agent (`arguswing`) defined in a TOML file, embedded at compile time, and upserted at runtime. The desktop frontend can assume this agent always exists and use it for conversations by default.

## Requirements

- Agent exists by default after app initialization
- Agent ID is stable and predictable (`arguswing`)
- Agent is provider-agnostic — binds to default provider at runtime
- Agent configuration is version-controlled in code (single source of truth)

## Implementation

### 1. Agent Definition File

**File:** `agents/arguswing.toml`

```toml
id = "arguswing"
display_name = "ArgusWing"
description = "Default assistant for ArgusWing"
version = "0.1.0"
system_prompt = "You are ArgusWing, a helpful AI assistant."
tool_names = ["shell", "read", "grep", "glob"]
# provider_id omitted = bind to default at runtime
# max_tokens omitted = use provider default
# temperature omitted = use provider default
```

### 2. Compile-Time Embedding

**File:** `crates/claw/src/agents/builtins.rs`

```rust
use super::types::{AgentId, AgentRecord};

/// Default ArgusWing agent definition embedded at compile time.
const ARGUSWING_TOML: &str = include_str!("../../../agents/arguswing.toml");

/// Load the built-in ArgusWing agent record.
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
```

### 3. Runtime Initialization

**File:** `crates/claw/src/claw.rs`

```rust
use crate::agents::builtins::load_arguswing;

const DEFAULT_AGENT_ID: &str = "arguswing";

impl AppContext {
    pub async fn init(database_target: Option<String>) -> Result<Self, AgentError> {
        // ... existing init code ...

        // Ensure default agent exists
        ensure_default_agent(&agent_manager).await?;

        Ok(Self { ... })
    }
}

/// Ensures the default ArgusWing agent exists in the database.
async fn ensure_default_agent(agent_manager: &AgentManager) -> Result<(), AgentError> {
    let default_agent = load_arguswing().map_err(|e| AgentError::BuiltinAgentLoadFailed {
        reason: e.to_string(),
    })?;
    agent_manager.upsert_template(default_agent).await?;
    Ok(())
}
```

### 4. Public API

**File:** `crates/claw/src/claw.rs`

```rust
impl AppContext {
    /// Get the default ArgusWing agent template.
    pub async fn get_default_agent_template(&self) -> Result<AgentRecord, AgentError> {
        self.get_template(&AgentId::new(DEFAULT_AGENT_ID))
            .await?
            .ok_or_else(|| AgentError::DefaultAgentNotFound)
    }

    /// Create a runtime agent from the default template.
    /// Binds to the default LLM provider.
    pub async fn create_default_agent(&self) -> Result<AgentId, AgentError> {
        let template = self.get_default_agent_template().await?;
        let default_provider = self.get_default_provider_record().await?;
        let mut record = template;
        record.provider_id = default_provider.id.to_string();
        self.agent_manager.create_agent(&record).await
    }
}
```

### 5. Error Handling

**File:** `crates/claw/src/error.rs`

```rust
pub enum AgentError {
    // ... existing variants ...

    #[error("Failed to load built-in agent: {reason}")]
    BuiltinAgentLoadFailed { reason: String },

    #[error("Default agent not found")]
    DefaultAgentNotFound,
}
```

### 6. Exports

**File:** `crates/claw/src/lib.rs`

```rust
pub const DEFAULT_AGENT_ID: &str = "arguswing";
```

### 7. Desktop Frontend Commands

**File:** `crates/desktop/src-tauri/src/commands.rs`

```rust
#[tauri::command]
pub async fn get_default_agent_template(
    ctx: State<'_, Arc<AppContext>>,
) -> Result<AgentRecord, String> {
    ctx.get_default_agent_template()
        .await
        .map_err(|e| e.to_string())
}

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

## File Changes Summary

| File | Change |
|------|--------|
| `agents/arguswing.toml` | **New** — Agent definition file |
| `crates/claw/src/agents/builtins.rs` | **New** — TOML parsing and loading |
| `crates/claw/src/agents/mod.rs` | Export `builtins` module |
| `crates/claw/src/claw.rs` | Add `ensure_default_agent()`, public API methods |
| `crates/claw/src/error.rs` | Add `BuiltinAgentLoadFailed`, `DefaultAgentNotFound` |
| `crates/claw/src/lib.rs` | Export `DEFAULT_AGENT_ID` |
| `crates/desktop/src-tauri/src/commands.rs` | Add Tauri commands |
| `crates/desktop/src-tauri/src/lib.rs` | Register new commands |

## Flow

1. **Compile time** → `include_str!` embeds `agents/arguswing.toml` into binary
2. **`AppContext::init()`** → `load_arguswing()` parses TOML → upserts to database
3. **Desktop starts** → calls `create_default_agent` → gets runtime agent bound to default provider
4. **User conversation** → messages go through `arguswing` agent

## Benefits Over Migration Approach

- **Single source of truth**: Agent definition is code, not database state
- **Version controlled**: Changes tracked in git, not migration history
- **Easier updates**: Modify TOML, rebuild, upsert handles the rest
- **No migration needed**: Cleaner, less to manage

## Out of Scope

- Updating agent config via UI
- Multiple built-in agents
- Provider selection at conversation start
