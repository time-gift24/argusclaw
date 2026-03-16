# ArgusWing Default Agent Design

**Date:** 2026-03-16
**Status:** Approved
**Scope:** Backend implementation for seeding and managing the default `arguswing` agent

## Overview

Add a default agent (`arguswing`) that is seeded via database migration and ensured at runtime. The desktop frontend can assume this agent always exists and use it for conversations by default.

## Requirements

- Agent exists by default after app initialization
- Agent ID is stable and predictable (`arguswing`)
- Agent is provider-agnostic — binds to default provider at runtime
- Agent configuration can evolve with code version

## Data Model

```rust
AgentRecord {
    id: AgentId::new("arguswing"),
    display_name: "ArgusWing",
    description: "Default assistant for ArgusWing",
    version: env!("CARGO_PKG_VERSION"),
    provider_id: String::new(),  // Empty = bind to default at runtime
    system_prompt: "You are ArgusWing, a helpful AI assistant.",
    tool_names: vec!["shell", "read", "grep", "glob"],
    max_tokens: None,
    temperature: None,
}
```

### Key Decisions

- **Provider-agnostic**: `provider_id` is empty in the template, resolved to default provider when creating runtime agent
- **Version tracking**: Uses crate version to help track agent config changes
- **Full capabilities**: All built-in tools enabled by default

## Implementation

### 1. Migration

**File:** `crates/claw/migrations/20260316XXXXXX_seed_arguswing_agent.sql`

```sql
-- Seed the default ArgusWing agent
INSERT OR IGNORE INTO agents (
    id, display_name, description, version, provider_id,
    system_prompt, tool_names, max_tokens, temperature
) VALUES (
    'arguswing',
    'ArgusWing',
    'Default assistant for ArgusWing',
    '0.1.0',
    NULL,
    'You are ArgusWing, a helpful AI assistant.',
    '["shell", "read", "grep", "glob"]',
    NULL,
    NULL
);
```

- `INSERT OR IGNORE` is idempotent — safe to run multiple times
- Uses `NULL` for `provider_id` (SQLite equivalent of empty)

### 2. Runtime Initialization

**File:** `crates/claw/src/claw.rs`

Add `ensure_default_agent()` function called during `AppContext::init()`:

```rust
const DEFAULT_AGENT_ID: &str = "arguswing";

/// Ensures the default ArgusWing agent exists in the database.
async fn ensure_default_agent(agent_manager: &AgentManager) -> Result<(), AgentError> {
    let default_agent = AgentRecord {
        id: AgentId::new(DEFAULT_AGENT_ID),
        display_name: "ArgusWing".to_string(),
        description: "Default assistant for ArgusWing".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        provider_id: String::new(),
        system_prompt: "You are ArgusWing, a helpful AI assistant.".to_string(),
        tool_names: vec!["shell".to_string(), "read".to_string(),
                        "grep".to_string(), "glob".to_string()],
        max_tokens: None,
        temperature: None,
    };

    agent_manager.upsert_template(default_agent).await?;
    Ok(())
}
```

### 3. Public API

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

### 4. Error Handling

**File:** `crates/claw/src/error.rs`

```rust
#[error("Default agent not found")]
DefaultAgentNotFound,
```

### 5. Exports

**File:** `crates/claw/src/lib.rs`

Export `DEFAULT_AGENT_ID` constant for consumers:

```rust
pub const DEFAULT_AGENT_ID: &str = "arguswing";
```

### 6. Desktop Frontend Commands

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
| `crates/claw/migrations/20260316XXXXXX_seed_arguswing_agent.sql` | New migration |
| `crates/claw/src/claw.rs` | Add initialization and API methods |
| `crates/claw/src/error.rs` | Add `DefaultAgentNotFound` error |
| `crates/claw/src/lib.rs` | Export `DEFAULT_AGENT_ID` |
| `crates/desktop/src-tauri/src/commands.rs` | Add Tauri commands |
| `crates/desktop/src-tauri/src/lib.rs` | Register new commands |

## Flow

1. **Migration runs** → seeds `arguswing` agent into database
2. **`AppContext::init()`** → upserts `arguswing` agent (ensures consistency)
3. **Desktop starts** → calls `create_default_agent` → gets runtime agent bound to default provider
4. **User conversation** → messages go through `arguswing` agent

## Out of Scope

- Updating agent config via UI
- Multiple default agents
- Provider selection at conversation start
