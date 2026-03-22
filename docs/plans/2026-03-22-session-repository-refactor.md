# Session Repository Refactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Move SQL operations from `SessionManager` into `argus-repository`, following the existing repository pattern (trait + SQLite implementation).

**Architecture:** Split `SessionManager` into pure business logic (in-memory session management) and data access (via `SessionRepository` trait). The repository trait lives in `argus-repository` alongside other repository traits (`ThreadRepository`, `AgentRepository`, etc.).

**Tech Stack:** Rust, sqlx, async-trait, thiserror

---

## Task 1: Add SessionRepository Trait

**Files:**
- Create: `crates/argus-repository/src/traits/session.rs`
- Modify: `crates/argus-repository/src/traits/mod.rs`

**Step 1: Create SessionRepository trait**

```rust
// crates/argus-repository/src/traits/session.rs
use async_trait::async_trait;
use argus_protocol::SessionId;

use crate::error::DbError;
use crate::types::SessionRecord;

#[derive(Debug, Clone)]
pub struct SessionSummaryRecord {
    pub id: SessionId,
    pub name: String,
    pub thread_count: i64,
    pub template_id: Option<i64>,
    pub provider_id: Option<i64>,
    pub updated_at: String,
}

#[async_trait]
pub trait SessionRepository: Send + Sync {
    /// Create a new session.
    async fn create_session(&self, name: &str) -> Result<SessionId, DbError>;

    /// Get a session by ID.
    async fn get_session(&self, id: SessionId) -> Result<Option<SessionRecord>, DbError>;

    /// List all sessions with thread counts.
    async fn list_sessions(&self) -> Result<Vec<SessionSummaryRecord>, DbError>;

    /// Update session name and updated_at.
    async fn update_session(&self, id: SessionId, name: &str) -> Result<(), DbError>;

    /// Delete a session and its threads (cascade).
    async fn delete_session(&self, id: SessionId) -> Result<bool, DbError>;

    /// Delete sessions older than the specified days.
    async fn cleanup_old_sessions(&self, days: u32) -> Result<u64, DbError>;
}
```

**Step 2: Export from traits/mod.rs**

Add to `crates/argus-repository/src/traits/mod.rs`:
```rust
mod session;
pub use session::{SessionRepository, SessionSummaryRecord};
```

**Step 3: Run to verify compilation**

Run: `cargo check -p argus-repository`
Expected: OK (after adding types)

---

## Task 2: Add SessionRecord Types

**Files:**
- Create: `crates/argus-repository/src/types/session.rs`
- Modify: `crates/argus-repository/src/types/mod.rs`

**Step 1: Create SessionRecord type**

```rust
// crates/argus-repository/src/types/session.rs
use std::fmt;

use serde::{Deserialize, Serialize};

use argus_protocol::SessionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionIdRecord(pub i64);

impl SessionIdRecord {
    pub fn new(id: i64) -> Self {
        Self(id)
    }
}

impl fmt::Display for SessionIdRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<SessionId> for SessionIdRecord {
    fn from(id: SessionId) -> Self {
        Self(id.inner())
    }
}

impl From<SessionIdRecord> for SessionId {
    fn from(id: SessionIdRecord) -> Self {
        SessionId::new(id.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: SessionId,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}
```

**Step 2: Export from types/mod.rs**

Add to `crates/argus-repository/src/types/mod.rs`:
```rust
mod session;
pub use session::{SessionIdRecord, SessionRecord};
```

**Step 3: Run to verify compilation**

Run: `cargo check -p argus-repository`
Expected: OK

---

## Task 3: Implement SessionRepository for ArgusSqlite

**Files:**
- Create: `crates/argus-repository/src/sqlite/session.rs`
- Modify: `crates/argus-repository/src/sqlite/mod.rs`

**Step 1: Create SQLite implementation**

```rust
// crates/argus-repository/src/sqlite/session.rs
use async_trait::async_trait;
use sqlx::Row;

use crate::error::DbError;
use crate::traits::{SessionRepository, SessionSummaryRecord};
use crate::types::SessionRecord;
use argus_protocol::SessionId;

use super::{ArgusSqlite, DbResult};

#[async_trait]
impl SessionRepository for ArgusSqlite {
    async fn create_session(&self, name: &str) -> DbResult<SessionId> {
        let result = sqlx::query(
            "INSERT INTO sessions (name, created_at, updated_at) VALUES (?1, datetime('now'), datetime('now'))",
        )
        .bind(name)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(SessionId::new(result.last_insert_rowid()))
    }

    async fn get_session(&self, id: SessionId) -> DbResult<Option<SessionRecord>> {
        let row = sqlx::query("SELECT id, name, created_at, updated_at FROM sessions WHERE id = ?1")
            .bind(id.inner())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_session_record(r)).transpose()
    }

    async fn list_sessions(&self) -> DbResult<Vec<SessionSummaryRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT s.id, s.name, s.updated_at, COUNT(t.id) as thread_count,
                   (SELECT t2.template_id FROM threads t2 WHERE t2.session_id = s.id LIMIT 1) as template_id,
                   (SELECT t3.provider_id FROM threads t3 WHERE t3.session_id = s.id LIMIT 1) as provider_id
            FROM sessions s
            LEFT JOIN threads t ON t.session_id = s.id
            GROUP BY s.id
            ORDER BY s.updated_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter()
            .map(|r| self.map_session_summary_record(r))
            .collect()
    }

    async fn update_session(&self, id: SessionId, name: &str) -> DbResult<()> {
        sqlx::query("UPDATE sessions SET name = ?2, updated_at = datetime('now') WHERE id = ?1")
            .bind(id.inner())
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        Ok(())
    }

    async fn delete_session(&self, id: SessionId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id.inner())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        Ok(result.rows_affected() > 0)
    }

    async fn cleanup_old_sessions(&self, days: u32) -> DbResult<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM sessions WHERE id IN (
                SELECT s.id FROM sessions s
                WHERE COALESCE(
                    (SELECT MAX(t.updated_at) FROM threads t WHERE t.session_id = s.id),
                    s.updated_at
                ) < datetime('now', '-' || ?1 || ' days')
            )
            "#,
        )
        .bind(days as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        Ok(result.rows_affected())
    }
}

impl ArgusSqlite {
    fn map_session_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<SessionRecord> {
        Ok(SessionRecord {
            id: SessionId::new(row.try_get::<i64, _>("id")?),
            name: row.try_get::<String, _>("name")?,
            created_at: row.try_get::<String, _>("created_at")?,
            updated_at: row.try_get::<String, _>("updated_at")?,
        })
    }

    fn map_session_summary_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<SessionSummaryRecord> {
        use argus_protocol::SessionId;

        let updated_at_str: String = row.try_get("updated_at")?;

        Ok(SessionSummaryRecord {
            id: SessionId::new(row.try_get::<i64, _>("id")?),
            name: row.try_get::<String, _>("name")?,
            thread_count: row.try_get::<i64, _>("thread_count")?,
            template_id: row.try_get("template_id")?,
            provider_id: row.try_get("provider_id")?,
            updated_at: updated_at_str,
        })
    }
}
```

**Step 2: Add module to sqlite/mod.rs**

Add to `crates/argus-repository/src/sqlite/mod.rs`:
```rust
mod session;
```

**Step 3: Run to verify compilation**

Run: `cargo check -p argus-repository`
Expected: OK

---

## Task 4: Refactor SessionManager to Use Repository

**Files:**
- Modify: `crates/argus-session/src/manager.rs`

**Step 1: Add repository field and refactor methods**

In `crates/argus-session/src/manager.rs`, modify the `SessionManager` struct to hold a `Arc<dyn SessionRepository>` instead of `SqlitePool`:

```rust
pub struct SessionManager {
    repository: Arc<dyn SessionRepository>,
    sessions: DashMap<SessionId, Arc<Session>>,
    // ... other fields
}
```

Update constructor to accept repository:
```rust
pub fn new(
    repository: Arc<dyn SessionRepository>,
    // ... other fields
) -> Self
```

Refactor each method to use repository instead of direct SQL:

- `list_sessions()` → delegate to `repository.list_sessions()`
- `load()` → still uses SQL for thread loading (keep in manager for now)
- `create()` → delegate to `repository.create_session()`
- `delete()` → delegate to `repository.delete_session()`
- `update_session_title()` → delegate to `repository.update_session()`
- `cleanup_old_sessions()` → delegate to `repository.cleanup_old_sessions()`

**Step 2: Update create method**

Change from:
```rust
pub async fn create(&self, name: String) -> Result<SessionId> {
    let result = sqlx::query(
        "INSERT INTO sessions (name, created_at, updated_at) VALUES (?, datetime('now'), datetime('now'))",
    )
    .bind(&name)
    .execute(&self.pool)
    .await
    .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

    let id = result.last_insert_rowid() as i64;
    Ok(SessionId::new(id))
}
```

To:
```rust
pub async fn create(&self, name: String) -> Result<SessionId> {
    self.repository.create_session(&name).await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })
}
```

**Step 3: Update delete method**

Change from direct SQL to:
```rust
pub async fn delete(&self, session_id: SessionId) -> Result<()> {
    self.repository.delete_session(session_id).await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;
    self.sessions.remove(&session_id);
    Ok(())
}
```

**Step 4: Update update_session_title method**

Change from direct SQL to:
```rust
pub async fn update_session_title(&self, session_id: SessionId, title: &str) -> Result<()> {
    self.repository.update_session(session_id, title).await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })
}
```

**Step 5: Update cleanup_old_sessions method**

Change from direct SQL to:
```rust
pub async fn cleanup_old_sessions(&self, days: u32) -> Result<u64> {
    self.repository.cleanup_old_sessions(days).await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })
}
```

**Step 6: Run to verify compilation**

Run: `cargo check -p argus-session`
Expected: OK after all changes

---

## Task 5: Update ArgusWing to Use Repository

**Files:**
- Modify: `crates/argus-wing/src/lib.rs`

**Step 1: Add repository dependency to ArgusWing**

The `ArgusWing` struct already holds `Arc<SessionManager>`. We need to create the repository and pass it to `SessionManager::new`.

In `crates/argus-wing/src/lib.rs`, update `ArgusWing::init()` to create an `ArgusSqlite` and pass it as `Arc<dyn SessionRepository>`:

```rust
// In init():
let repository: Arc<dyn SessionRepository> = Arc::new(ArgusSqlite::new(pool.clone()));

let session_manager = Arc::new(SessionManager::new(
    repository,
    template_manager.clone(),
    provider_resolver,
    tool_manager.clone(),
    compactor_manager.clone(),
    trace_dir,
));
```

Note: You may need to add `use argus_repository::SessionRepository;` to the imports.

**Step 2: Run to verify compilation**

Run: `cargo check -p argus-wing`
Expected: OK

---

## Task 6: Update ArgusWing with_pool

**Files:**
- Modify: `crates/argus-wing/src/lib.rs`

**Step 1: Update with_pool to also call cleanup**

Add the same cleanup call to `with_pool()`:
```rust
let count = session_manager
    .cleanup_old_sessions(14)
    .await
    .map_err(|e| ArgusError::DatabaseError {
        reason: format!("Failed to cleanup old sessions: {}", e),
    })?;
tracing::info!(deleted = count, "Cleaned up {} old sessions", count);
```

**Step 2: Run to verify compilation**

Run: `cargo check -p argus-wing`
Expected: OK

---

## Task 7: Add Tests for SessionRepository

**Files:**
- Create: `crates/argus-repository/src/sqlite/session_tests.rs`

**Step 1: Write integration tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn create_test_db() -> DbResult<ArgusSqlite> {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let pool = connect_path(&path).await?;
        migrate(&pool).await?;
        Ok(ArgusSqlite::new(pool))
    }

    #[tokio::test]
    async fn create_and_get_session() -> DbResult<()> {
        let db = create_test_db().await?;

        let id = db.create_session("test-session").await?;
        let session = db.get_session(id).await?;

        assert!(session.is_some());
        assert_eq!(session.unwrap().name, "test-session");
        Ok(())
    }

    #[tokio::test]
    async fn list_sessions_empty() -> DbResult<()> {
        let db = create_test_db().await?;

        let sessions = db.list_sessions().await?;
        assert!(sessions.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn update_session() -> DbResult<()> {
        let db = create_test_db().await?;

        let id = db.create_session("original").await?;
        db.update_session(id, "renamed").await?;

        let session = db.get_session(id).await?.unwrap();
        assert_eq!(session.name, "renamed");
        Ok(())
    }

    #[tokio::test]
    async fn delete_session() -> DbResult<()> {
        let db = create_test_db().await?;

        let id = db.create_session("to-delete").await?;
        let deleted = db.delete_session(id).await?;
        assert!(deleted);

        let session = db.get_session(id).await?;
        assert!(session.is_none());
        Ok(())
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p argus-repository session`
Expected: All tests pass

---

## Task 8: Run Full Test Suite

**Step 1: Run all cargo tests**

Run: `cd /Users/wanyaozhong/projects/argusclaw/.worktrees/feature-manage-chat-threads && cargo test --all`
Expected: All tests pass

**Step 2: Run prek**

Run: `cd /Users/wanyaozhong/projects/argusclaw/.worktrees/feature-manage-chat-threads && prek`
Expected: OK (fmt, clippy)

---

## Task 9: Commit

**Step 1: Stage and commit**

```bash
git add -A
git commit -m "refactor: move SessionManager SQL operations to argus-repository

- Add SessionRepository trait in argus-repository
- Add SessionRecord types for persistence
- Implement SessionRepository for ArgusSqlite
- Refactor SessionManager to use repository instead of direct SQL
- Update ArgusWing to create and pass repository to SessionManager
- Add cleanup_old_sessions call to with_pool()

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Summary of Changes

| File | Change |
|------|--------|
| `argus-repository/src/traits/session.rs` | New - SessionRepository trait |
| `argus-repository/src/traits/mod.rs` | Export SessionRepository |
| `argus-repository/src/types/session.rs` | New - SessionIdRecord, SessionRecord types |
| `argus-repository/src/types/mod.rs` | Export session types |
| `argus-repository/src/sqlite/session.rs` | New - SQLite implementation |
| `argus-repository/src/sqlite/mod.rs` | Include session module |
| `argus-session/src/manager.rs` | Use repository instead of direct SQL |
| `argus-wing/src/lib.rs` | Create repository, pass to SessionManager |

---

## Verification Checklist

- [ ] `cargo check -p argus-repository` passes
- [ ] `cargo check -p argus-session` passes
- [ ] `cargo check -p argus-wing` passes
- [ ] `cargo test -p argus-repository` passes
- [ ] `cargo test --all` passes
- [ ] `prek` passes (fmt, clippy)
