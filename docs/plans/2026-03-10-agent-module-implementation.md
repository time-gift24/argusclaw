# Agent Module Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement custom agent storage and management with SQLite persistence and in-memory caching.

**Architecture:** Follow existing `LlmProviderRepository` pattern. Domain types and trait in `agents/types.rs`, AgentManager with DashMap cache in `agents/mod.rs`, SQLite implementation in `db/sqlite/agent.rs`.

**Tech Stack:** Rust, sqlx, DashMap, async-trait, serde_json, thiserror

---

## Task 1: Create Domain Types

**Files:**
- Create: `crates/claw/src/agents/types.rs`
- Modify: `crates/claw/src/agents/mod.rs`

**Step 1: Create `agents/types.rs` with AgentId and AgentRecord**

```rust
//! Agent domain types.

use std::fmt;
use std::str::FromStr;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::db::DbError;

/// Unique identifier for an agent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(String);

impl AgentId {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl AsRef<str> for AgentId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for AgentId {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

/// Full agent record stored in database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRecord {
    pub id: AgentId,
    pub display_name: String,
    pub description: String,
    pub version: String,
    pub provider_id: String,
    pub system_prompt: String,
    pub tool_names: Vec<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

impl AgentRecord {
    /// Create a minimal agent record for testing.
    #[cfg(test)]
    pub fn for_test(id: &str, provider_id: &str) -> Self {
        Self {
            id: AgentId::new(id),
            display_name: format!("Test Agent {id}"),
            description: "A test agent".to_string(),
            version: "1.0.0".to_string(),
            provider_id: provider_id.to_string(),
            system_prompt: "You are a test agent.".to_string(),
            tool_names: vec![],
            max_tokens: None,
            temperature: None,
        }
    }
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

impl From<AgentRecord> for AgentSummary {
    fn from(record: AgentRecord) -> Self {
        Self {
            id: record.id,
            display_name: record.display_name,
            description: record.description,
            version: record.version,
            provider_id: record.provider_id,
        }
    }
}

/// Repository trait for agent persistence.
#[async_trait]
pub trait AgentRepository: Send + Sync {
    /// Create or update an agent.
    async fn upsert(&self, record: &AgentRecord) -> Result<(), DbError>;

    /// Get an agent by ID.
    async fn get(&self, id: &AgentId) -> Result<Option<AgentRecord>, DbError>;

    /// List all agents (summaries only).
    async fn list(&self) -> Result<Vec<AgentSummary>, DbError>;

    /// Delete an agent. Returns true if a row was deleted.
    async fn delete(&self, id: &AgentId) -> Result<bool, DbError>;
}
```

**Step 2: Update `agents/mod.rs` to export types**

```rust
//! Agent management module.

mod types;

pub use types::{AgentId, AgentRecord, AgentRepository, AgentSummary};

use std::sync::Arc;

use dashmap::DashMap;

use crate::db::DbError;

/// Manages custom agents with in-memory caching.
#[derive(Clone)]
pub struct AgentManager {
    repository: Arc<dyn AgentRepository>,
    cache: DashMap<AgentId, AgentRecord>,
}

impl AgentManager {
    #[must_use]
    pub fn new(repository: Arc<dyn AgentRepository>) -> Self {
        Self {
            repository,
            cache: DashMap::new(),
        }
    }

    /// Create or update an agent.
    pub async fn upsert(&self, record: AgentRecord) -> Result<(), DbError> {
        self.repository.upsert(&record).await?;
        self.cache.insert(record.id.clone(), record);
        Ok(())
    }

    /// Get an agent by ID with read-through cache.
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

    /// List all agents (summaries only).
    pub async fn list(&self) -> Result<Vec<AgentSummary>, DbError> {
        self.repository.list().await
    }

    /// Delete an agent.
    pub async fn delete(&self, id: &AgentId) -> Result<bool, DbError> {
        let deleted = self.repository.delete(id).await?;
        if deleted {
            self.cache.remove(id);
        }
        Ok(deleted)
    }
}
```

**Step 3: Run cargo check**

Run: `cd /Users/wanyaozhong/Projects/argusclaw && cargo check --package argusclaw-claw 2>&1 | head -30`
Expected: Errors about missing `AgentRepository` implementation (trait object cannot be made)

**Step 4: Commit**

```bash
git add crates/claw/src/agents/types.rs crates/claw/src/agents/mod.rs
git commit -m "feat(claw): add agent domain types and AgentManager stub"
```

---

## Task 2: Create SQLite Migration

**Files:**
- Create: `crates/claw/migrations/20260310000001_create_agents.sql`

**Step 1: Create migration file**

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
    temperature INTEGER,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_agents_provider_id ON agents(provider_id);
```

**Step 2: Verify migration file syntax**

Run: `cat crates/claw/migrations/20260310000001_create_agents.sql`
Expected: File contents shown

**Step 3: Commit**

```bash
git add crates/claw/migrations/20260310000001_create_agents.sql
git commit -m "feat(claw): add agents table migration"
```

---

## Task 3: Implement SQLite Repository

**Files:**
- Create: `crates/claw/src/db/sqlite/agent.rs`
- Modify: `crates/claw/src/db/sqlite/mod.rs`

**Step 1: Create `db/sqlite/agent.rs`**

```rust
//! SQLite implementation of AgentRepository.

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::agents::{AgentId, AgentRecord, AgentRepository, AgentSummary};
use crate::db::DbError;

pub struct SqliteAgentRepository {
    pool: SqlitePool,
}

impl SqliteAgentRepository {
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn map_record(row: sqlx::sqlite::SqliteRow) -> Result<AgentRecord, DbError> {
        let tool_names_json: String = row.try_get("tool_names").map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        let tool_names: Vec<String> = serde_json::from_str(&tool_names_json)
            .map_err(|e| DbError::QueryFailed {
                reason: format!("failed to parse tool_names JSON: {e}"),
            })?;

        let temperature: Option<f32> = row
            .try_get::<Option<i64>, _>("temperature")
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?
            .map(|t| t as f32 / 100.0);

        Ok(AgentRecord {
            id: AgentId::new(row.try_get::<String, _>("id").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?),
            display_name: row.try_get("display_name").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            description: row.try_get("description").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            version: row.try_get("version").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            provider_id: row.try_get("provider_id").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            system_prompt: row.try_get("system_prompt").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            tool_names,
            max_tokens: row.try_get("max_tokens").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            temperature,
        })
    }

    fn map_summary(row: sqlx::sqlite::SqliteRow) -> Result<AgentSummary, DbError> {
        Ok(AgentSummary {
            id: AgentId::new(row.try_get::<String, _>("id").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?),
            display_name: row.try_get("display_name").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            description: row.try_get("description").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            version: row.try_get("version").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            provider_id: row.try_get("provider_id").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
        })
    }
}

#[async_trait]
impl AgentRepository for SqliteAgentRepository {
    async fn upsert(&self, record: &AgentRecord) -> Result<(), DbError> {
        let tool_names_json = serde_json::to_string(&record.tool_names).map_err(|e| {
            DbError::QueryFailed {
                reason: format!("failed to serialize tool_names: {e}"),
            }
        })?;

        let temperature_int = record.temperature.map(|t| (t * 100.0) as i64);

        sqlx::query(
            r#"INSERT INTO agents (id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
               ON CONFLICT(id) DO UPDATE SET
                   display_name = excluded.display_name,
                   description = excluded.description,
                   version = excluded.version,
                   provider_id = excluded.provider_id,
                   system_prompt = excluded.system_prompt,
                   tool_names = excluded.tool_names,
                   max_tokens = excluded.max_tokens,
                   temperature = excluded.temperature,
                   updated_at = CURRENT_TIMESTAMP"#,
        )
        .bind(record.id.as_ref())
        .bind(&record.display_name)
        .bind(&record.description)
        .bind(&record.version)
        .bind(&record.provider_id)
        .bind(&record.system_prompt)
        .bind(&tool_names_json)
        .bind(record.max_tokens.map(|t| t as i64))
        .bind(temperature_int)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn get(&self, id: &AgentId) -> Result<Option<AgentRecord>, DbError> {
        let row = sqlx::query(
            r#"SELECT id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature
               FROM agents
               WHERE id = ?1"#,
        )
        .bind(id.as_ref())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(Self::map_record).transpose()
    }

    async fn list(&self) -> Result<Vec<AgentSummary>, DbError> {
        let rows = sqlx::query(
            r#"SELECT id, display_name, description, version, provider_id
               FROM agents
               ORDER BY display_name ASC"#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter().map(Self::map_summary).collect()
    }

    async fn delete(&self, id: &AgentId) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM agents WHERE id = ?1")
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }
}
```

**Step 2: Update `db/sqlite/mod.rs` to export**

Add at the end of `crates/claw/src/db/sqlite/mod.rs`:

```rust
mod agent;

pub use agent::SqliteAgentRepository;
```

**Step 3: Run cargo check**

Run: `cd /Users/wanyaozhong/Projects/argusclaw && cargo check --package argusclaw-claw 2>&1 | head -30`
Expected: Compiles successfully or shows minor issues

**Step 4: Commit**

```bash
git add crates/claw/src/db/sqlite/agent.rs crates/claw/src/db/sqlite/mod.rs
git commit -m "feat(claw): implement SqliteAgentRepository"
```

---

## Task 4: Integrate into AppContext

**Files:**
- Modify: `crates/claw/src/claw.rs`

**Step 1: Update AppContext initialization**

In `crates/claw/src/claw.rs`, modify the `init` function to create `SqliteAgentRepository`:

Add import at top:
```rust
use crate::agents::AgentRepository;
use crate::db::sqlite::SqliteAgentRepository;
```

Modify `init` function (around line 32-39):

```rust
let repository = Arc::new(SqliteLlmProviderRepository::new(pool.clone()));
let agent_repository = Arc::new(SqliteAgentRepository::new(pool));
let llm_manager = Arc::new(LLMManager::new(repository));
let agent_manager = Arc::new(AgentManager::new(agent_repository));
let tool_manager = Arc::new(ToolManager::new());
```

Add accessor method:
```rust
#[must_use]
pub fn agent_manager(&self) -> Arc<AgentManager> {
    Arc::clone(&self.agent_manager)
}
```

**Step 2: Run cargo check**

Run: `cd /Users/wanyaozhong/Projects/argusclaw && cargo check --package argusclaw-claw 2>&1 | head -30`
Expected: Compiles successfully

**Step 3: Commit**

```bash
git add crates/claw/src/claw.rs
git commit -m "feat(claw): integrate AgentRepository into AppContext"
```

---

## Task 5: Add Unit Tests for AgentManager

**Files:**
- Modify: `crates/claw/src/agents/mod.rs` (add test module)

**Step 1: Add mock repository for testing**

Add to end of `crates/claw/src/agents/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::RwLock;

    /// In-memory mock repository for testing.
    struct MockAgentRepository {
        agents: RwLock<HashMap<String, AgentRecord>>,
    }

    impl MockAgentRepository {
        fn new() -> Self {
            Self {
                agents: RwLock::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl AgentRepository for MockAgentRepository {
        async fn upsert(&self, record: &AgentRecord) -> Result<(), DbError> {
            let mut agents = self.agents.write().unwrap();
            agents.insert(record.id.as_ref().to_string(), record.clone());
            Ok(())
        }

        async fn get(&self, id: &AgentId) -> Result<Option<AgentRecord>, DbError> {
            let agents = self.agents.read().unwrap();
            Ok(agents.get(id.as_ref()).cloned())
        }

        async fn list(&self) -> Result<Vec<AgentSummary>, DbError> {
            let agents = self.agents.read().unwrap();
            Ok(agents.values().map(|r| r.clone().into()).collect())
        }

        async fn delete(&self, id: &AgentId) -> Result<bool, DbError> {
            let mut agents = self.agents.write().unwrap();
            Ok(agents.remove(id.as_ref()).is_some())
        }
    }

    fn create_test_record(id: &str) -> AgentRecord {
        AgentRecord {
            id: AgentId::new(id),
            display_name: format!("Agent {id}"),
            description: "Test agent".to_string(),
            version: "1.0.0".to_string(),
            provider_id: "test-provider".to_string(),
            system_prompt: "You are a test agent.".to_string(),
            tool_names: vec!["tool1".to_string()],
            max_tokens: Some(1000),
            temperature: Some(0.7),
        }
    }

    #[tokio::test]
    async fn upsert_stores_and_retrieves_agent() {
        let repo = Arc::new(MockAgentRepository::new());
        let manager = AgentManager::new(repo);

        let record = create_test_record("agent-1");
        manager.upsert(record.clone()).await.unwrap();

        let retrieved = manager.get(&AgentId::new("agent-1")).await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, record.id);
        assert_eq!(retrieved.display_name, record.display_name);
        assert_eq!(retrieved.temperature, record.temperature);
    }

    #[tokio::test]
    async fn get_returns_none_for_missing_agent() {
        let repo = Arc::new(MockAgentRepository::new());
        let manager = AgentManager::new(repo);

        let result = manager.get(&AgentId::new("missing")).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn delete_removes_agent() {
        let repo = Arc::new(MockAgentRepository::new());
        let manager = AgentManager::new(repo);

        let record = create_test_record("agent-to-delete");
        manager.upsert(record).await.unwrap();

        let deleted = manager.delete(&AgentId::new("agent-to-delete")).await.unwrap();
        assert!(deleted);

        let result = manager.get(&AgentId::new("agent-to-delete")).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn delete_returns_false_for_missing_agent() {
        let repo = Arc::new(MockAgentRepository::new());
        let manager = AgentManager::new(repo);

        let deleted = manager.delete(&AgentId::new("missing")).await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn list_returns_summaries() {
        let repo = Arc::new(MockAgentRepository::new());
        let manager = AgentManager::new(repo);

        manager.upsert(create_test_record("agent-1")).await.unwrap();
        manager.upsert(create_test_record("agent-2")).await.unwrap();

        let summaries = manager.list().await.unwrap();
        assert_eq!(summaries.len(), 2);
    }

    #[tokio::test]
    async fn cache_is_updated_on_upsert() {
        let repo = Arc::new(MockAgentRepository::new());
        let manager = AgentManager::new(repo);

        let mut record = create_test_record("cached-agent");
        manager.upsert(record.clone()).await.unwrap();

        // Update the record
        record.display_name = "Updated Name".to_string();
        manager.upsert(record.clone()).await.unwrap();

        // Should get updated version from cache
        let retrieved = manager.get(&AgentId::new("cached-agent")).await.unwrap().unwrap();
        assert_eq!(retrieved.display_name, "Updated Name");
    }
}
```

**Step 2: Run tests**

Run: `cd /Users/wanyaozhong/Projects/argusclaw && cargo test --package argusclaw-claw agents::tests 2>&1`
Expected: All 6 tests pass

**Step 3: Commit**

```bash
git add crates/claw/src/agents/mod.rs
git commit -m "test(claw): add unit tests for AgentManager"
```

---

## Task 6: Add Integration Tests for SQLite Repository

**Files:**
- Create: `crates/claw/tests/agent_repository_test.rs`

**Step 1: Create integration test file**

```rust
//! Integration tests for SqliteAgentRepository.

use std::sync::Arc;

use argusclaw_claw::agents::{AgentId, AgentRecord, AgentRepository, AgentSummary};
use argusclaw_claw::db::sqlite::{SqliteAgentRepository, connect};

fn create_test_record(id: &str, provider_id: &str) -> AgentRecord {
    AgentRecord {
        id: AgentId::new(id),
        display_name: format!("Agent {id}"),
        description: "Integration test agent".to_string(),
        version: "1.0.0".to_string(),
        provider_id: provider_id.to_string(),
        system_prompt: "You are a test agent.".to_string(),
        tool_names: vec!["tool1".to_string(), "tool2".to_string()],
        max_tokens: Some(2000),
        temperature: Some(0.5),
    }
}

#[tokio::test]
async fn upsert_and_get_agent() {
    let pool = connect("sqlite::memory:").await.unwrap();
    let repo = SqliteAgentRepository::new(pool);

    let record = create_test_record("test-agent-1", "provider-1");
    repo.upsert(&record).await.unwrap();

    let retrieved = repo.get(&AgentId::new("test-agent-1")).await.unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.id.as_ref(), "test-agent-1");
    assert_eq!(retrieved.tool_names, vec!["tool1", "tool2"]);
}

#[tokio::test]
async fn get_returns_none_for_missing() {
    let pool = connect("sqlite::memory:").await.unwrap();
    let repo = SqliteAgentRepository::new(pool);

    let result = repo.get(&AgentId::new("missing")).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn upsert_updates_existing() {
    let pool = connect("sqlite::memory:").await.unwrap();
    let repo = SqliteAgentRepository::new(pool);

    let mut record = create_test_record("update-test", "provider-1");
    repo.upsert(&record).await.unwrap();

    record.display_name = "Updated Name".to_string();
    record.version = "2.0.0".to_string();
    repo.upsert(&record).await.unwrap();

    let retrieved = repo.get(&AgentId::new("update-test")).await.unwrap().unwrap();
    assert_eq!(retrieved.display_name, "Updated Name");
    assert_eq!(retrieved.version, "2.0.0");
}

#[tokio::test]
async fn list_returns_summaries() {
    let pool = connect("sqlite::memory:").await.unwrap();
    let repo = SqliteAgentRepository::new(pool);

    repo.upsert(&create_test_record("list-1", "provider-1")).await.unwrap();
    repo.upsert(&create_test_record("list-2", "provider-1")).await.unwrap();

    let summaries = repo.list().await.unwrap();
    assert_eq!(summaries.len(), 2);

    let ids: Vec<&str> = summaries.iter().map(|s| s.id.as_ref()).collect();
    assert!(ids.contains(&"list-1"));
    assert!(ids.contains(&"list-2"));
}

#[tokio::test]
async fn delete_removes_agent() {
    let pool = connect("sqlite::memory:").await.unwrap();
    let repo = SqliteAgentRepository::new(pool);

    repo.upsert(&create_test_record("delete-test", "provider-1")).await.unwrap();

    let deleted = repo.delete(&AgentId::new("delete-test")).await.unwrap();
    assert!(deleted);

    let result = repo.get(&AgentId::new("delete-test")).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn delete_returns_false_for_missing() {
    let pool = connect("sqlite::memory:").await.unwrap();
    let repo = SqliteAgentRepository::new(pool);

    let deleted = repo.delete(&AgentId::new("missing")).await.unwrap();
    assert!(!deleted);
}

#[tokio::test]
async fn temperature_precision_preserved() {
    let pool = connect("sqlite::memory:").await.unwrap();
    let repo = SqliteAgentRepository::new(pool);

    let mut record = create_test_record("temp-test", "provider-1");
    record.temperature = Some(0.73);
    repo.upsert(&record).await.unwrap();

    let retrieved = repo.get(&AgentId::new("temp-test")).await.unwrap().unwrap();
    // Allow small floating point error
    assert!((retrieved.temperature.unwrap() - 0.73).abs() < 0.01);
}

#[tokio::test]
async fn summary_excludes_large_fields() {
    let pool = connect("sqlite::memory:").await.unwrap();
    let repo = SqliteAgentRepository::new(pool);

    let record = create_test_record("summary-test", "provider-1");
    repo.upsert(&record).await.unwrap();

    let summaries = repo.list().await.unwrap();
    let summary = summaries.iter().find(|s| s.id.as_ref() == "summary-test").unwrap();

    // Summary should not have system_prompt field
    // (This is enforced at compile time by the type system)
    assert_eq!(summary.display_name, "Agent summary-test");
}
```

**Step 2: Run tests**

Run: `cd /Users/wanyaozhong/Projects/argusclaw && cargo test --package argusclaw-claw --test agent_repository_test 2>&1`
Expected: All tests pass

**Step 3: Commit**

```bash
git add crates/claw/tests/agent_repository_test.rs
git commit -m "test(claw): add integration tests for SqliteAgentRepository"
```

---

## Task 7: Run Full Test Suite and Lint

**Step 1: Run all tests**

Run: `cd /Users/wanyaozhong/Projects/argusclaw && cargo test --package argusclaw-claw 2>&1`
Expected: All tests pass

**Step 2: Run clippy**

Run: `cd /Users/wanyaozhong/Projects/argusclaw && cargo clippy --package argusclaw-claw --all-features 2>&1`
Expected: Zero warnings

**Step 3: Run fmt**

Run: `cd /Users/wanyaozhong/Projects/argusclaw && cargo fmt --check 2>&1`
Expected: No output (already formatted)

**Step 4: Final commit (if any fixes needed)**

```bash
git add -A
git commit -m "fix(claw): address clippy warnings and format code"
```

---

## Verification Checklist

After all tasks complete:

- [ ] `cargo test --package argusclaw-claw` passes
- [ ] `cargo clippy --package argusclaw-claw --all-features` has zero warnings
- [ ] `cargo fmt --check` passes
- [ ] Migration runs successfully on fresh database
- [ ] AgentManager can upsert, get, list, delete agents
- [ ] Temperature precision is preserved through SQLite storage

---

## Implementation Order

1. Task 1: Domain Types (foundation)
2. Task 2: Migration (DB schema)
3. Task 3: SQLite Repository (persistence)
4. Task 4: AppContext Integration (wire up)
5. Task 5: AgentManager Tests (unit tests)
6. Task 6: SQLite Tests (integration tests)
7. Task 7: Final Verification (quality gates)
