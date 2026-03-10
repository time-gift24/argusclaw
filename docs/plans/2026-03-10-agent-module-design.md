# Agent Module Design

Date: 2026-03-10

## Overview

Design for custom agent storage and management in the ArgusClaw `agents` module. Custom agents are user-defined configurations stored in SQLite, loaded into memory at runtime, and dispatched through `AgentManager`.

## Requirements Summary

| Item | Decision |
|------|----------|
| LLM Association | Reference via `provider_id` |
| Tools Association | Store as `Vec<String>` (tool names), lookup from ToolManager at runtime |
| LLM Parameters | Only `max_tokens` and `temperature` |
| Default Agent | Not needed, accessed via builtin agents |
| Architecture | AgentManager only manages DB-stored Custom agents |
| Agent Fields | `id`, `provider_id`, `system_prompt`, `tool_names`, `max_tokens`, `temperature`, `display_name`, `description`, `version` |

## Architecture

### File Structure

```
crates/claw/src/
├── agents/
│   ├── mod.rs           # AgentManager + module re-exports
│   └── types.rs         # AgentId, AgentRecord, AgentSummary, AgentRepository trait
├── db/
│   ├── mod.rs           # (existing)
│   └── sqlite/
│       ├── mod.rs       # Add SqliteAgentRepository export
│       └── agent.rs     # SqliteAgentRepository implementation
└── claw.rs              # AppContext integration

crates/claw/migrations/
└── 20260310000001_create_agents.sql
```

### Data Flow

```
AppContext
    └── AgentManager (memory cache + business logic)
            └── AgentRepository (trait, implemented by SqliteAgentRepository)
                    └── SQLite DB
```

## Domain Types

### `agents/types.rs`

```rust
use std::fmt;
use std::str::FromStr;
use async_trait::async_trait;
use crate::db::DbError;

/// Unique identifier for an agent.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentId(String);

impl AgentId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl AsRef<str> for AgentId { ... }
impl fmt::Display for AgentId { ... }

/// Full agent record stored in database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRecord {
    pub id: AgentId,
    pub display_name: String,
    pub description: String,
    pub version: String,
    pub provider_id: String,           // references llm_providers.id
    pub system_prompt: String,
    pub tool_names: Vec<String>,       // stored as JSON in DB
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,      // stored as INTEGER * 100 in DB
}

/// Summary for listing (excludes large fields like system_prompt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSummary {
    pub id: AgentId,
    pub display_name: String,
    pub description: String,
    pub version: String,
    pub provider_id: String,
}

/// Repository trait for agent persistence.
#[async_trait]
pub trait AgentRepository: Send + Sync {
    async fn upsert(&self, record: &AgentRecord) -> Result<(), DbError>;
    async fn get(&self, id: &AgentId) -> Result<Option<AgentRecord>, DbError>;
    async fn list(&self) -> Result<Vec<AgentSummary>, DbError>;
    async fn delete(&self, id: &AgentId) -> Result<bool, DbError>;
}
```

### Design Notes

- `temperature` stored as `INTEGER * 100` to avoid SQLite floating-point precision issues
- `tool_names` stored as JSON TEXT
- `AgentSummary` excludes `system_prompt` for efficient list display

## SQLite Schema

### Migration (`20260310000001_create_agents.sql`)

```sql
CREATE TABLE IF NOT EXISTS agents (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '1.0.0',
    provider_id TEXT NOT NULL REFERENCES llm_providers(id) ON DELETE RESTRICT,
    system_prompt TEXT NOT NULL,
    tool_names TEXT NOT NULL DEFAULT '[]',
    max_tokens INTEGER,
    temperature INTEGER,               -- stored as value * 100
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_agents_provider_id ON agents(provider_id);
```

## SQLite Implementation

### `db/sqlite/agent.rs`

```rust
pub struct SqliteAgentRepository {
    pool: SqlitePool,
}

impl SqliteAgentRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AgentRepository for SqliteAgentRepository {
    async fn upsert(&self, record: &AgentRecord) -> Result<(), DbError> {
        // INSERT ... ON CONFLICT DO UPDATE
        // Serialize tool_names to JSON
        // Convert temperature to INTEGER (f32 * 100)
    }

    async fn get(&self, id: &AgentId) -> Result<Option<AgentRecord>, DbError> {
        // Parse tool_names from JSON
        // Convert temperature back to f32 (i64 / 100.0)
    }

    async fn list(&self) -> Result<Vec<AgentSummary>, DbError> {
        // SELECT without system_prompt
    }

    async fn delete(&self, id: &AgentId) -> Result<bool, DbError> {
        // DELETE, return true if row was deleted
    }
}
```

## AgentManager

### `agents/mod.rs`

```rust
mod types;

pub use types::{AgentId, AgentRecord, AgentSummary, AgentRepository};

use std::sync::Arc;
use dashmap::DashMap;

/// Manages custom agents with in-memory caching.
pub struct AgentManager {
    repository: Arc<dyn AgentRepository>,
    cache: DashMap<AgentId, AgentRecord>,
}

impl AgentManager {
    pub fn new(repository: Arc<dyn AgentRepository>) -> Self {
        Self {
            repository,
            cache: DashMap::new(),
        }
    }

    /// Create or update an agent. Updates both DB and cache.
    pub async fn upsert(&self, record: AgentRecord) -> Result<(), DbError> {
        self.repository.upsert(&record).await?;
        self.cache.insert(record.id.clone(), record);
        Ok(())
    }

    /// Get agent by ID. Checks cache first, then DB.
    pub async fn get(&self, id: &AgentId) -> Result<Option<AgentRecord>, DbError> {
        if let Some(cached) = self.cache.get(id) {
            return Ok(Some(cached.clone()));
        }

        if let Some(record) = self.repository.get(id).await? {
            self.cache.insert(id.clone(), record.clone());
            return Ok(Some(record));
        }

        Ok(None)
    }

    /// List all agents (summaries only, no caching).
    pub async fn list(&self) -> Result<Vec<AgentSummary>, DbError> {
        self.repository.list().await
    }

    /// Delete an agent. Removes from both DB and cache.
    pub async fn delete(&self, id: &AgentId) -> Result<bool, DbError> {
        let deleted = self.repository.delete(id).await?;
        if deleted {
            self.cache.remove(id);
        }
        Ok(deleted)
    }

    /// Load all agents into cache (call at startup if needed).
    pub async fn warm_cache(&self) -> Result<(), DbError> {
        // Load all records and populate cache
    }
}
```

### Design Notes

- Uses `DashMap` for concurrent-safe in-memory caching
- `get()` uses read-through cache strategy
- `upsert()` and `delete()` update both DB and cache

## AppContext Integration

### `claw.rs` Changes

```rust
impl AppContext {
    pub async fn init(database_target: Option<String>) -> Result<Self, AgentError> {
        // ...existing code...

        let agent_repository = Arc::new(SqliteAgentRepository::new(pool.clone()));
        let agent_manager = Arc::new(AgentManager::new(agent_repository));

        Ok(Self::new(llm_manager, agent_manager, tool_manager))
    }

    pub fn agent_manager(&self) -> Arc<AgentManager> {
        Arc::clone(&self.agent_manager)
    }
}
```

## File Changes Summary

| File | Operation | Description |
|------|-----------|-------------|
| `crates/claw/src/agents/mod.rs` | Rewrite | AgentManager + re-exports |
| `crates/claw/src/agents/types.rs` | Create | AgentId, AgentRecord, AgentRepository trait |
| `crates/claw/src/db/sqlite/mod.rs` | Modify | Add `SqliteAgentRepository` export |
| `crates/claw/src/db/sqlite/agent.rs` | Create | SQLite implementation |
| `crates/claw/src/claw.rs` | Modify | Integrate AgentRepository |
| `crates/claw/migrations/20260310000001_create_agents.sql` | Create | Table migration |

## Checklist

- [ ] Create `agents/types.rs` with domain types
- [ ] Rewrite `agents/mod.rs` with AgentManager
- [ ] Create `db/sqlite/agent.rs` with SQLite implementation
- [ ] Update `db/sqlite/mod.rs` to export SqliteAgentRepository
- [ ] Create migration `20260310000001_create_agents.sql`
- [ ] Update `claw.rs` to integrate AgentRepository
- [ ] Add unit tests for AgentManager
- [ ] Add unit tests for SqliteAgentRepository
