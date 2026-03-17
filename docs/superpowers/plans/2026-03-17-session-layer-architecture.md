# Session Layer Architecture Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Session layer for organizing Threads, refactor monolithic `claw` crate into feature-based `argus-*` crates, and add turn logging with LRU cleanup.

**Architecture:** Feature-based crate decomposition with `argus-protocol` as the shared types layer. Session becomes the top-level container for Threads, eliminating the runtime Agent concept.

**Tech Stack:** Rust, SQLite (sqlx), tokio async, thiserror

---

## File Structure Overview

After implementation:
```
crates/
├── argus-protocol/     # NEW: Shared public types (no internal deps)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── ids.rs          # SessionId, ThreadId, AgentId, ProviderId
│       ├── error.rs        # ArgusError
│       ├── config.rs       # ThreadConfig
│       ├── events.rs      # ThreadEvent
│       ├── approval.rs     # Approval types
│       └── hooks.rs       # Hook types
│
├── argus-session/      # NEW: Session + Thread management
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── session_id.rs
│       ├── session.rs     # Session struct
│       ├── manager.rs     # SessionManager
│       ├── thread.rs      # Thread struct
│       └── repository.rs  # SessionRepository trait + impl
│
├── argus-template/     # NEW: Template management
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       └── manager.rs     # TemplateManager
│
├── argus-log/         # NEW: Turn logging
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── models.rs     # TurnLog struct
│       ├── repository.rs # TurnLogRepository trait + impl
│       └── cleaner.rs    # LogCleaner
│
├── argus-core/        # RENAMED: from claw
│   └── ...
│
├── cli/
└── desktop/
```

---

## Phase 1: Create argus-protocol Crate

> **Note on Dependencies:** The spec says argus-protocol has no internal deps, but some types like ThreadConfig depend on TurnConfig which depends on LLM types. For implementation, either:
> 1. Keep ThreadConfig/TurnConfig in argus-core initially, OR
> 2. Make argus-protocol depend on argus-core for these types
> The plan follows approach #1 - start minimal with just IDs and errors, expand as needed.

### Chunk 1: Setup argus-protocol

**Files:**
- Create: `crates/argus-protocol/Cargo.toml`
- Create: `crates/argus-protocol/src/lib.rs`
- Create: `crates/argus-protocol/src/ids.rs`
- Create: `crates/argus-protocol/src/error.rs`

- [ ] **Step 1: Create argus-protocol Cargo.toml**

```toml
[package]
name = "argus-protocol"
version = "0.1.0"
edition = "2021"

[dependencies]
chrono = { version = "0.4", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
thiserror = "2"
uuid = { version = "1", features = ["serde"] }
tokio = { version = "1", features = ["sync"] }
```

- [ ] **Step 2: Create ids.rs with strong types**

```rust
use serde::{Deserialize, Serialize};

/// Session ID - INTEGER auto-increment from database
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub i64);

impl SessionId {
    pub fn new(id: i64) -> Self {
        Self(id)
    }
    pub fn inner(&self) -> i64 {
        self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Thread ID - TEXT (UUID string)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ThreadId(pub String);

impl ThreadId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
    pub fn inner(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Agent/Template ID - INTEGER auto-increment from database
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub i64);

impl AgentId {
    pub fn new(id: i64) -> Self {
        Self(id)
    }
    pub fn inner(&self) -> i64 {
        self.0
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Provider ID - INTEGER auto-increment from database
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProviderId(pub i64);

impl ProviderId {
    pub fn new(id: i64) -> Self {
        Self(id)
    }
    pub fn inner(&self) -> i64 {
        self.0
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
```

- [ ] **Step 3: Create error.rs with ArgusError**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArgusError {
    #[error("Session not found: {0}")]
    SessionNotFound(i64),

    #[error("Session already loaded: {0}")]
    SessionAlreadyLoaded(i64),

    #[error("Session not loaded: {0}")]
    SessionNotLoaded(i64),

    #[error("Thread not found: {0}")]
    ThreadNotFound(String),

    #[error("Template not found: {0}")]
    TemplateNotFound(i64),

    #[error("Provider not found: {0}")]
    ProviderNotFound(i64),

    #[error("Turn log error: {reason}")]
    TurnLogError { reason: String },

    #[error("Database error: {reason}")]
    DatabaseError { reason: String },

    #[error("LLM error: {reason}")]
    LlmError { reason: String },

    #[error("IO error: {reason}")]
    IoError { reason: String },

    #[error("Serialization error: {reason}")]
    SerdeError { reason: String },
}

pub type Result<T> = std::result::Result<T, ArgusError>;
```

- [ ] **Step 4: Create lib.rs exports**

```rust
pub mod ids;
pub mod error;
pub mod config;
pub mod events;
pub mod approval;
pub mod hooks;

pub use ids::{SessionId, ThreadId, AgentId, ProviderId};
pub use error::{ArgusError, Result};
// ... other exports
```

- [ ] **Step 5: Update workspace Cargo.toml**

Add to workspace members (update as new crates are created):
```toml
[workspace]
members = [
    "crates/argus-protocol",
    "crates/claw",
    "crates/cli",
    "crates/desktop/src-tauri",
]
```

- [ ] **Step 6: Verify build**

```bash
cargo build -p argus-protocol
```

- [ ] **Step 7: Commit**

```bash
git add crates/argus-protocol/ Cargo.toml
git commit -m "feat(protocol): create argus-protocol crate with shared types

- Add SessionId, ThreadId, AgentId, ProviderId strong types
- Add ArgusError enum with common error variants
- Configure workspace to include new crate

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Chunk 2: Move protocol types to argus-protocol

**Files:**
- Modify: `crates/argus-protocol/src/lib.rs`
- Create: `crates/argus-protocol/src/config.rs` (move from claw)
- Create: `crates/argus-protocol/src/events.rs` (move from claw)
- Create: `crates/argus-protocol/src/approval.rs` (move from claw)
- Create: `crates/argus-protocol/src/hooks.rs` (move from claw)

- [ ] **Step 1: Read current protocol/thread_id.rs**

```bash
cat crates/claw/src/protocol/thread_id.rs
```

- [ ] **Step 2: Create config.rs with ThreadConfig**

(Move from claw - see existing ThreadConfig in claw/src/agents/thread/config.rs)

- [ ] **Step 3: Create events.rs with ThreadEvent**

(Move from claw/src/protocol/thread_event.rs)

- [ ] **Step 4: Create approval.rs**

(Move from claw/src/protocol/approval.rs)

- [ ] **Step 5: Create hooks.rs**

(Move from claw/src/protocol/hooks.rs)

- [ ] **Step 6: Update lib.rs exports**

- [ ] **Step 7: Verify build and tests pass**

```bash
cargo build -p argus-protocol
```

- [ ] **Step 8: Commit**

```bash
git add crates/argus-protocol/
git commit -m "feat(protocol): move protocol types from claw

- Move ThreadConfig, ThreadEvent, approval types, hooks
- Update exports in lib.rs

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Phase 2: Database Migrations

### Chunk 3: Create sessions and turn_logs tables

**Files:**
- Create: `crates/claw/migrations/<timestamp>_add_sessions.sql` (use `sqlx migrate add add_sessions` in claw directory)
- Create: `crates/claw/migrations/<timestamp>_add_turn_logs.sql` (use `sqlx migrate add add_turn_logs` in claw directory)

- [ ] **Step 1: Create sessions table migration**

```sql
-- Create sessions table
CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Index for listing sessions
CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at DESC);
```

- [ ] **Step 2: Create turn_logs table migration**

```sql
-- Create turn_logs table
CREATE TABLE IF NOT EXISTS turn_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    turn_seq INTEGER NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    model TEXT NOT NULL,
    latency_ms INTEGER NOT NULL,
    turn_data TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(thread_id, turn_seq)
);

CREATE INDEX IF NOT EXISTS idx_turn_logs_thread ON turn_logs(thread_id);
CREATE INDEX IF NOT EXISTS idx_turn_logs_created ON turn_logs(created_at);
```

- [ ] **Step 3: Add columns to threads table**

```sql
-- Add session_id and template_id to threads
ALTER TABLE threads ADD COLUMN session_id INTEGER REFERENCES sessions(id);
ALTER TABLE threads ADD COLUMN template_id INTEGER REFERENCES agents(id);
```

- [ ] **Step 4: Create default "Legacy" session and migrate existing threads**

```sql
-- Insert default session for existing threads
INSERT INTO sessions (id, name, created_at, updated_at)
VALUES (1, 'Legacy', datetime('now'), datetime('now'));

-- Update existing threads to belong to Legacy session
UPDATE threads SET session_id = 1 WHERE session_id IS NULL;
```

- [ ] **Step 5: Make session_id and template_id NOT NULL**

```sql
ALTER TABLE threads ALTER COLUMN session_id INTEGER NOT NULL;
ALTER TABLE threads ALTER COLUMN template_id INTEGER NOT NULL;
```

- [ ] **Step 6: Commit**

```bash
git add crates/claw/migrations/
git commit -m "feat(db): add sessions and turn_logs tables

- Add sessions table for organizing threads
- Add turn_logs table for tracking turn metrics
- Add session_id and template_id to threads
- Create default Legacy session for existing data

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Phase 3: Create argus-log Crate

### Chunk 4: Implement TurnLogRepository and LogCleaner

**Files:**
- Create: `crates/argus-log/Cargo.toml`
- Create: `crates/argus-log/src/lib.rs`
- Create: `crates/argus-log/src/models.rs`
- Create: `crates/argus-log/src/repository.rs`
- Create: `crates/argus-log/src/cleaner.rs`

- [ ] **Step 1: Create argus-log Cargo.toml**

```toml
[package]
name = "argus-log"
version = "0.1.0"
edition = "2021"

[dependencies]
argus-protocol = { path = "../argus-protocol" }
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }
tokio = { version = "1", features = ["sync"] }
chrono = { version = "0.4", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
async-trait = "0.1"
thiserror = "2"

[dev-dependencies]
tokio = { version = "1", features = ["full", "test-util"] }
```

- [ ] **Step 2: Create models.rs with TurnLog struct**

```rust
use argus_protocol::{ThreadId, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnLog {
    pub thread_id: ThreadId,
    pub turn_seq: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub model: String,
    pub latency_ms: i64,
    pub turn_data: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupReport {
    pub deleted_count: i64,
}
```

- [ ] **Step 3: Create repository.rs with TurnLogRepository trait**

```rust
use async_trait::async_trait;
use argus_protocol::Result;
use crate::models::{TurnLog, CleanupReport};
use argus_protocol::ThreadId;

#[async_trait]
pub trait TurnLogRepository: Send + Sync {
    async fn append(&self, log: TurnLog) -> Result<()>;
    async fn get_by_thread(&self, thread_id: &ThreadId) -> Result<Vec<TurnLog>>;
    async fn get_active_thread_ids(&self, limit: i64) -> Result<Vec<ThreadId>>;
    async fn delete_except(&self, keep_thread_ids: &[ThreadId]) -> Result<i64>;
}

pub struct SqliteTurnLogRepository {
    pool: SqlitePool,
}

impl SqliteTurnLogRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TurnLogRepository for SqliteTurnLogRepository {
    async fn append(&self, log: TurnLog) -> Result<()> {
        // Implementation
    }

    async fn get_by_thread(&self, thread_id: &ThreadId) -> Result<Vec<TurnLog>> {
        // Implementation
    }

    async fn get_active_thread_ids(&self, limit: i64) -> Result<Vec<ThreadId>> {
        // Implementation - get distinct thread_ids ordered by most recent activity
    }

    async fn delete_except(&self, keep_thread_ids: &[ThreadId]) -> Result<i64> {
        // Implementation - delete logs for threads not in keep list
    }
}
```

- [ ] **Step 4: Create cleaner.rs with LogCleaner**

```rust
use crate::repository::TurnLogRepository;
use crate::models::CleanupReport;
use argus_protocol::Result;

pub struct LogCleaner<R: TurnLogRepository> {
    repository: Arc<R>,
    max_threads: i64,
}

impl<R: TurnLogRepository> LogCleaner<R> {
    pub fn new(repository: Arc<R>, max_threads: i64) -> Self {
        Self { repository, max_threads }
    }

    pub async fn cleanup(&self) -> Result<CleanupReport> {
        let keep_ids = self.repository.get_active_thread_ids(self.max_threads).await?;
        let deleted = self.repository.delete_except(&keep_ids).await?;
        Ok(CleanupReport { deleted_count: deleted })
    }
}
```

- [ ] **Step 5: Create lib.rs exports**

- [ ] **Step 6: Verify build**

```bash
cargo build -p argus-log
```

- [ ] **Step 7: Write and run tests**

```bash
# Test LRU cleanup with 25 threads, verify 20 remain
cargo test -p argus-log
```

- [ ] **Step 8: Commit**

```bash
git add crates/argus-log/
git commit -m "feat(log): add argus-log crate with TurnLogRepository

- Add TurnLog struct for recording turn metrics
- Implement SqliteTurnLogRepository
- Add LogCleaner with LRU-based cleanup (20 threads max)

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Phase 4: Create argus-template Crate

### Chunk 5: Implement TemplateManager

**Files:**
- Create: `crates/argus-template/Cargo.toml`
- Create: `crates/argus-template/src/lib.rs`
- Create: `crates/argus-template/src/manager.rs`

- [ ] **Step 1: Create argus-template Cargo.toml**

```toml
[package]
name = "argus-template"
version = "0.1.0"
edition = "2021"

[dependencies]
argus-protocol = { path = "../argus-protocol" }
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }
tokio = { version = "1", features = ["sync"] }
serde = { version = "1", features = ["derive"] }
async-trait = "0.1"
thiserror = "2"
```

- [ ] **Step 2: Create manager.rs**

```rust
use argus_protocol::{AgentId, ProviderId, Result, ArgusError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTemplate {
    pub id: AgentId,
    pub display_name: String,
    pub description: String,
    pub version: String,
    pub provider_id: Option<ProviderId>,
    pub system_prompt: String,
    pub tool_names: Vec<String>,
    pub max_tokens: Option<i32>,
    pub temperature: Option<i32>,
}

pub struct TemplateManager {
    pool: SqlitePool,
}

impl TemplateManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn upsert(&self, template: AgentTemplate) -> Result<AgentId> {
        // Implementation
    }

    pub async fn get(&self, id: AgentId) -> Result<Option<AgentTemplate>> {
        // Implementation
    }

    pub async fn list(&self) -> Result<Vec<AgentTemplate>> {
        // Implementation
    }

    pub async fn delete(&self, id: AgentId) -> Result<()> {
        // Implementation
    }
}
```

- [ ] **Step 3: Create lib.rs exports**

- [ ] **Step 4: Verify build**

```bash
cargo build -p argus-template
```

- [ ] **Step 5: Commit**

```bash
git add crates/argus-template/
git commit -m "feat(template): add argus-template crate with TemplateManager

- Add AgentTemplate struct
- Implement TemplateManager with CRUD operations

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Phase 5: Create argus-session Crate

> **Note:** LLMManager and LlmProvider stay in argus-core (not argus-session). The SessionManager holds Arc<LLMManager> from argus-core.

### Chunk 6: Implement Session and SessionManager

**Files:**
- Create: `crates/argus-session/Cargo.toml`
- Create: `crates/argus-session/src/lib.rs`
- Create: `crates/argus-session/src/session.rs`
- Create: `crates/argus-session/src/manager.rs`
- Create: `crates/argus-session/src/thread.rs`
- Create: `crates/argus-session/src/repository.rs`

- [ ] **Step 1: Create argus-session Cargo.toml**

```toml
[package]
name = "argus-session"
version = "0.1.0"
edition = "2021"

[dependencies]
argus-protocol = { path = "../argus-protocol" }
argus-log = { path = "../argus-log" }
argus-template = { path = "../argus-template" }
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }
tokio = { version = "1", features = ["sync"] }
dashmap = "6"
serde = { version = "1", features = ["derive"] }
async-trait = "0.1"
thiserror = "2"
chrono = { version = "0.4", features = ["serde"] }
```

- [ ] **Step 2: Create session.rs**

```rust
use argus_protocol::{SessionId, ThreadId, AgentId, ProviderId, Result, ThreadEvent, ThreadConfig};
use dashmap::DashMap;
use tokio::sync::{broadcast, Mutex};

pub struct Session {
    pub id: SessionId,
    pub name: String,
    pub threads: DashMap<ThreadId, Arc<Mutex<Thread>>>,
    // ... shared managers
}

impl Session {
    pub async fn create_thread(
        &self,
        template_id: AgentId,
        provider_id: ProviderId,
        config: ThreadConfig,
    ) -> Result<ThreadId> {
        // Implementation
    }

    pub fn list_threads(&self) -> Vec<ThreadSummary> {
        // Implementation
    }

    pub fn get_thread(&self, thread_id: &ThreadId) -> Option<Arc<Mutex<Thread>>> {
        self.threads.get(thread_id).map(|r| r.clone())
    }

    pub async fn delete_thread(&self, thread_id: &ThreadId) -> Result<()> {
        // Implementation
    }
}
```

- [ ] **Step 3: Create thread.rs**

```rust
use argus_protocol::{SessionId, ThreadId, AgentId, ProviderId, ThreadEvent, ThreadConfig, Result};

pub struct Thread {
    pub id: ThreadId,
    pub session_id: SessionId,
    pub template_id: AgentId,
    pub provider_id: ProviderId,
    pub provider: Arc<dyn LlmProvider>,
    pub system_prompt: String,
    pub messages: Vec<ChatMessage>,
    pub event_sender: broadcast::Sender<ThreadEvent>,
    pub config: ThreadConfig,
    pub token_count: u32,
    pub turn_count: u32,
}

impl Thread {
    pub async fn send_message(&mut self, content: String) -> Result<()> {
        // Implementation - execute turn and log
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ThreadEvent> {
        self.event_sender.subscribe()
    }
}
```

- [ ] **Step 4: Create manager.rs with SessionManager**

```rust
pub struct SessionManager {
    pool: SqlitePool,
    sessions: DashMap<SessionId, Arc<Session>>,
    template_manager: Arc<TemplateManager>,
    llm_manager: Arc<LLMManager>,
    turn_log_repository: Arc<dyn TurnLogRepository>,
}

impl SessionManager {
    pub fn new(
        pool: SqlitePool,
        template_manager: Arc<TemplateManager>,
        llm_manager: Arc<LLMManager>,
        turn_log_repository: Arc<dyn TurnLogRepository>,
    ) -> Self {
        Self {
            pool,
            sessions: DashMap::new(),
            template_manager,
            llm_manager,
            turn_log_repository,
        }
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>> {
        // Query sessions from DB without loading into memory
    }

    pub async fn load(&self, session_id: SessionId) -> Result<Arc<Session>> {
        // Load session into memory
    }

    pub async fn unload(&self, session_id: SessionId) -> Result<()> {
        // Unload session from memory
    }

    pub async fn create(&self, name: String) -> Result<SessionId> {
        // Create new session in DB
    }

    pub async fn delete(&self, session_id: SessionId) -> Result<()> {
        // Delete session and all threads
    }
}
```

- [ ] **Step 5: Create lib.rs exports**

- [ ] **Step 6: Verify build**

```bash
cargo build -p argus-session
```

- [ ] **Step 7: Commit**

```bash
git add crates/argus-session/
git commit -m "feat(session): add argus-session crate

- Add Session struct as thread container
- Add Thread struct with direct template reference (no runtime Agent)
- Add SessionManager with lazy loading

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Phase 6: Update AppContext and Integration

### Chunk 7: Update AppContext API

**Files:**
- Modify: `crates/claw/src/claw.rs`
- Modify: `crates/claw/src/lib.rs`

- [ ] **Step 1: Read current AppContext**

```bash
head -100 crates/claw/src/claw.rs
```

- [ ] **Step 2: Add SessionManager to AppContext**

```rust
pub struct AppContext {
    // ... existing fields
    pub session_manager: Arc<SessionManager>,
    pub turn_log_repository: Arc<dyn TurnLogRepository>,
    pub log_cleaner: Arc<LogCleaner>,
}
```

- [ ] **Step 3: Add new API methods**

```rust
impl AppContext {
    // Session Management
    pub async fn create_session(&self, name: String) -> Result<SessionId> {
        self.session_manager.create(name).await
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>> {
        self.session_manager.list_sessions().await
    }

    pub async fn load_session(&self, session_id: SessionId) -> Result<()> {
        self.session_manager.load(session_id).await
    }

    pub async fn unload_session(&self, session_id: SessionId) -> Result<()> {
        self.session_manager.unload(session_id).await
    }

    pub async fn delete_session(&self, session_id: SessionId) -> Result<()> {
        self.session_manager.delete(session_id).await
    }

    // Thread Management
    pub async fn create_thread(
        &self,
        session_id: SessionId,
        template_id: AgentId,
        provider_id: ProviderId,
        config: ThreadConfig,
    ) -> Result<ThreadId> {
        let session = self.session_manager.load(session_id).await?;
        session.create_thread(template_id, provider_id, config).await
    }

    // ... etc
}
```

- [ ] **Step 4: Update lib.rs exports**

- [ ] **Step 5: Verify build**

```bash
cargo build -p claw
```

- [ ] **Step 6: Commit**

```bash
git add crates/claw/src/
git commit -m "feat(claw): add session-based API to AppContext

- Add SessionManager to AppContext
- Add session/thread management methods
- Deprecate old Agent-based API

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Chunk 8: Update cli and desktop

**Files:**
- Modify: `crates/cli/src/agent.rs`
- Modify: `crates/desktop/src-tauri/src/commands.rs`

- [ ] **Step 1: Update cli to use new API**

```rust
// Old: ctx.create_default_agent_with_approval()
// New: ctx.create_session() + ctx.create_thread()

// Example:
let session_id = ctx.create_session("My Session".to_string()).await?;
ctx.load_session(session_id).await?;
let thread_id = ctx.create_thread(
    session_id,
    template_id,
    provider_id,
    ThreadConfig::default(),
).await?;
```

- [ ] **Step 2: Update desktop Tauri commands**

- [ ] **Step 3: Verify build**

```bash
cargo build --all
```

- [ ] **Step 4: Commit**

```bash
git add crates/cli/ crates/desktop/
git commit -m "feat(cli,desktop): update to use session-based API

- Update CLI to create sessions before creating threads
- Update Tauri commands to use new API

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Phase 7: Cleanup

### Chunk 9: Remove unused code

**Files:**
- Delete: `crates/claw/src/agents/agent/runtime.rs`
- Delete: `crates/claw/src/db/thread.rs` (ThreadRepository)
- Modify: `crates/claw/src/agents/mod.rs`

- [ ] **Step 1: Remove runtime Agent concept**

- [ ] **Step 2: Remove ThreadRepository**

- [ ] **Step 3: Commit**

```bash
git commit -m "refactor(claw): remove unused runtime Agent and ThreadRepository

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

### Chunk 10: Rename claw to argus-core

**Files:**
- Rename: `crates/claw` → `crates/argus-core`
- Modify: `Cargo.toml` workspace members

- [ ] **Step 1: Rename claw directory**

```bash
mv crates/claw crates/argus-core
```

- [ ] **Step 2: Update Cargo.toml**

```toml
[workspace]
members = [
    "crates/argus-protocol",
    "crates/argus-session",
    "crates/argus-template",
    "crates/argus-log",
    "crates/argus-core",
    "crates/cli",
    "crates/desktop/src-tauri",
]
```

- [ ] **Step 3: Update internal crate name references**

Update all internal `name = "claw"` to `name = "argus-core"` in Cargo.toml files.

- [ ] **Step 4: Verify build**

```bash
cargo build --all
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: rename claw to argus-core

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

### Chunk 11: Final verification

**Files:**
- All

- [ ] **Step 1: Run static checks**

```bash
# Run prek for static analysis
cargo install prek 2>/dev/null || true
prek
```

- [ ] **Step 2: Run cargo deny**

```bash
cargo deny check
```

- [ ] **Step 3: Run all tests**

```bash
cargo test --all
```

- [ ] **Step 4: Verify CLI and desktop build**

```bash
cargo build -p cli
cargo build -p desktop
```

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "chore: complete session layer architecture

- Add Session layer for organizing Threads
- Refactor to argus-* crate structure
- Add turn logging with LRU cleanup
- Update all consumers

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Optional: Extract Additional Feature Crates

The following feature crates are optional and can be extracted later for further decoupling:

### Optional: argus-tool Crate

Extract ToolManager and tools from argus-core.

### Optional: argus-llm Crate

Extract LLMManager and providers from argus-core.

### Optional: argus-approval Crate

Extract ApprovalManager from argus-core.

**Note:** These extractions are optional - the core session functionality works without them. Extract when there's a clear need for independent versioning or reuse.

| Phase | Chunk | Description |
|-------|-------|-------------|
| 1 | 1 | Create argus-protocol crate |
| 1 | 2 | Move protocol types to argus-protocol |
| 2 | 3 | Database migrations |
| 3 | 4 | Create argus-log crate |
| 4 | 5 | Create argus-template crate |
| 5 | 6 | Create argus-session crate |
| 6 | 7 | Update AppContext API |
| 6 | 8 | Update cli and desktop |
| 7 | 9 | Remove unused code |
| 7 | 10 | Rename claw to argus-core |
| 7 | 11 | Final verification |

**Optional Crates (not required for core functionality):**
- argus-tool (ToolManager extraction)
- argus-llm (LLMManager extraction)
- argus-approval (ApprovalManager extraction)

---

## Notes

- This plan follows the spec exactly. See `docs/superpowers/specs/2026-03-17-session-layer-architecture.md`
- Each chunk should be self-contained and testable
- Commit after each chunk to enable easy rollback if needed
- Run `cargo build --all` after each phase to verify integration
