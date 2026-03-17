# Session Layer Architecture Design

**Date:** 2026-03-17
**Status:** Draft
**Author:** Design session

## Summary

Add a Session layer to organize Threads, refactor the crate structure from monolithic `claw` to feature-based `argus-*` crates, and add a turn logging system with LRU-based cleanup.

## Motivation

- **Organization:** Users need a way to group related conversations (Threads) together
- **Multi-tasking:** Support multiple active sessions simultaneously (like browser tabs)
- **Simplification:** Eliminate the confusing "runtime Agent" concept - Thread should reference templates directly
- **Observability:** Track token usage and latency per turn for analytics and recovery
- **Maintainability:** Split monolithic `claw` crate into focused, independent feature crates

## Architecture Overview

### Current Database Schema

```sql
-- LLM Providers (INTEGER 自增 ID)
CREATE TABLE llm_providers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    display_name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    models TEXT NOT NULL DEFAULT '[]',
    default_model TEXT NOT NULL,
    encrypted_api_key BLOB NOT NULL,
    ...
);

-- Agents (INTEGER 自增 ID)
CREATE TABLE agents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    display_name TEXT NOT NULL,
    ...
    provider_id INTEGER REFERENCES llm_providers(id) ON DELETE RESTRICT,
    ...
);

-- Threads (TEXT ID, provider_id 改为 INTEGER)
CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    provider_id INTEGER NOT NULL REFERENCES llm_providers(id),
    title TEXT,
    token_count INTEGER NOT NULL DEFAULT 0,
    turn_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Messages
CREATE TABLE messages ( ... );

-- Approval Requests
CREATE TABLE approval_requests ( ... );

-- Workflows
CREATE TABLE workflows ( ... );

-- Jobs
CREATE TABLE jobs ( ... );
```

### Data Model (After Changes)

```
sessions (new, INTEGER id)
    ↓ 1:N
threads (modified: add session_id, template_id)
    ↓ 1:N
messages (unchanged)
    ↓
turn_logs (new)

agents (templates, global, INTEGER id)
    ↑ referenced by threads.template_id

llm_providers (unchanged)
    ↑ providers threads.provider_id
```

### Crate Structure

```
crates/
├── argus-protocol/     # Shared public types (no internal deps)
├── argus-session/      # Session + Thread management (merged, tightly coupled)
├── argus-template/     # Template management (was AgentManager)
├── argus-log/          # Turn logging with LRU cleanup
├── argus-tool/         # Tool management
├── argus-llm/          # LLM providers
├── argus-approval/     # Approval system
├── argus-core/         # AppContext facade (orchestrates all crates)
├── cli/                # CLI frontend
└── desktop/            # Tauri desktop frontend
```

### Dependency Graph

```
                argus-protocol
                      ↑
    ┌─────────────────┼─────────────────┐
    │                 │                 │
argus-session   argus-template   argus-log   argus-tool   argus-llm   argus-approval
    │                 │                 │           │           │           │
    └─────────────────┴─────────────────┴───────────┴───────────┴───────────┘
                                │
                                ↓
                          argus-core
                                ↑
                          ┌─────┴─────┐
                         cli      desktop
```

**Dependency rules:**
- `argus-protocol` has NO dependencies on other argus crates (leaf crate)
- Feature crates depend ONLY on `argus-protocol` (not on each other)
- `argus-core` depends on ALL feature crates and orchestrates them
- `cli` and `desktop` depend ONLY on `argus-core`

## Detailed Design

### 1. Database Schema

#### New: sessions table

```sql
CREATE TABLE sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

#### Modified: threads table

Add columns:
- `session_id INTEGER NOT NULL REFERENCES sessions(id)`
- `template_id INTEGER NOT NULL REFERENCES agents(id)`

Note: `provider_id` already exists in current schema as INTEGER FK to `llm_providers(id)`.

#### New: turn_logs table

```sql
CREATE TABLE turn_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    turn_seq INTEGER NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    model TEXT NOT NULL,
    latency_ms INTEGER NOT NULL,
    turn_data TEXT NOT NULL,  -- JSON: full turn info for recovery
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(thread_id, turn_seq)
);

CREATE INDEX idx_turn_logs_thread ON turn_logs(thread_id);
CREATE INDEX idx_turn_logs_created ON turn_logs(created_at);
```

#### Deleted

- `ThreadRepository` - was never used, remove entirely

### 2. argus-protocol Crate

All public types that cross crate boundaries. No dependencies on other argus crates.

```rust
// IDs (strong typing - matching current INTEGER id pattern)
pub struct SessionId(i64);        // INTEGER, auto-increment
pub struct ThreadId(String);       // TEXT (keeps current pattern)
pub struct AgentId(i64);           // INTEGER, auto-increment
pub struct ProviderId(i64);        // INTEGER, auto-increment

// Summary types (for list queries - lazy loading)
pub struct SessionSummary {
    pub id: SessionId,
    pub name: String,
    pub thread_count: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct ThreadSummary {
    pub id: ThreadId,
    pub title: Option<String>,
    pub token_count: u32,
    pub turn_count: u32,
    pub template_id: AgentId,
    pub provider_id: ProviderId,
    pub created_at: DateTime<Utc>,
}

// Events (existing, moved here)
pub enum ThreadEvent { ... }

// Config (existing, moved here)
pub struct ThreadConfig { ... }

// Approval types (existing, moved here)
pub struct ApprovalRequest { ... }
pub struct ApprovalResponse { ... }
pub enum ApprovalDecision { ... }

// Error types
#[derive(Debug, thiserror::Error)]
pub enum ArgusError {
    #[error("Session not found: {0}")]
    SessionNotFound(SessionId),

    #[error("Session already loaded: {0}")]
    SessionAlreadyLoaded(SessionId),

    #[error("Session not loaded: {0}")]
    SessionNotLoaded(SessionId),

    #[error("Thread not found: {0}")]
    ThreadNotFound(ThreadId),

    #[error("Template not found: {0}")]
    TemplateNotFound(AgentId),

    #[error("Provider not found: {0}")]
    ProviderNotFound(ProviderId),

    #[error("Turn log error: {reason}")]
    TurnLogError { reason: String },

    #[error("Database error: {reason}")]
    DatabaseError { reason: String },

    #[error("LLM error: {reason}")]
    LlmError { reason: String },
}
```

### 3. argus-session Crate

Contains Session and Thread management - tightly coupled, kept together.

```rust
// SessionManager - lazy loading from database
pub struct SessionManager {
    db_pool: SqlitePool,
    sessions: DashMap<SessionId, Arc<Session>>,
    template_manager: Arc<TemplateManager>,
    llm_manager: Arc<LLMManager>,
    tool_manager: Arc<ToolManager>,
    turn_log_repository: Arc<TurnLogRepository>,
    // ... other shared resources
}

impl SessionManager {
    /// List all sessions (summary only, no loading)
    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>>;

    /// Load a session into memory
    pub async fn load(&self, session_id: &SessionId) -> Result<Arc<Session>>;

    /// Unload a session from memory
    pub async fn unload(&self, session_id: &SessionId) -> Result<()>;

    /// Create a new session
    pub async fn create(&self, name: String) -> Result<SessionId>;

    /// Delete a session and all its threads
    pub async fn delete(&self, session_id: &SessionId) -> Result<()>;
}

// Session - in-memory container for threads
pub struct Session {
    id: SessionId,
    name: String,
    threads: DashMap<ThreadId, Arc<Mutex<Thread>>>,
    // ... shared managers
}

impl Session {
    /// Create a thread with template + provider binding
    pub async fn create_thread(
        &self,
        template_id: AgentId,
        provider_id: ProviderId,
        config: ThreadConfig,
    ) -> Result<ThreadId>;

    /// List threads in this session
    pub fn list_threads(&self) -> Vec<ThreadSummary>;

    /// Get a thread by ID
    pub fn get_thread(&self, thread_id: &ThreadId) -> Option<Arc<Mutex<Thread>>>;

    /// Delete a thread
    pub async fn delete_thread(&self, thread_id: &ThreadId) -> Result<()>;
}

// Thread - holds provider binding + messages directly
// NO runtime Agent concept - Thread references template directly
pub struct Thread {
    id: ThreadId,
    session_id: SessionId,
    template_id: AgentId,           // References global template (INTEGER)
    provider_id: ProviderId,        // Provider to use (INTEGER)
    provider: Arc<dyn LlmProvider>, // Bound at creation, fixed for lifetime
    system_prompt: String,          // Copied from template at creation
    messages: Vec<ChatMessage>,
    event_sender: broadcast::Sender<ThreadEvent>,
    config: ThreadConfig,
    token_count: u32,
    turn_count: u32,
    // ... compactor, approval_manager, hooks
}

impl Thread {
    /// Send a message to this thread
    pub async fn send_message(&mut self, content: String) -> Result<()>;

    /// Subscribe to thread events
    pub fn subscribe(&self) -> broadcast::Receiver<ThreadEvent>;
}
```

**Key design decision:** Thread holds `template_id` (reference) and `system_prompt` (copied). The prompt is copied at creation so template changes don't affect existing threads.

### 4. argus-template Crate

Renamed from AgentManager, manages agent templates only.

```rust
pub struct TemplateManager {
    repository: Arc<dyn TemplateRepository>,
}

impl TemplateManager {
    pub async fn upsert(&self, template: AgentTemplate) -> Result<AgentId>;
    pub async fn get(&self, id: AgentId) -> Result<Option<AgentTemplate>>;
    pub async fn list(&self) -> Result<Vec<AgentTemplate>>;
    pub async fn delete(&self, id: AgentId) -> Result<()>;
}

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
```

### 5. argus-log Crate

Turn logging with LRU-based cleanup.

```rust
pub struct TurnLog {
    pub thread_id: ThreadId,
    pub turn_seq: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub model: String,
    pub latency_ms: i64,
    pub turn_data: String,  // JSON: full turn info for recovery
    pub created_at: DateTime<Utc>,
}

#[async_trait]
pub trait TurnLogRepository: Send + Sync {
    async fn append(&self, log: TurnLog) -> Result<()>;
    async fn get_by_thread(&self, thread_id: &ThreadId) -> Result<Vec<TurnLog>>;
    async fn get_active_thread_ids(&self, limit: i64) -> Result<Vec<ThreadId>>;
    async fn delete_except(&self, keep_thread_ids: &[ThreadId]) -> Result<i64>;
}

pub struct LogCleaner {
    repository: Arc<dyn TurnLogRepository>,
    max_threads: i64,  // Default: 20
}

impl LogCleaner {
    /// Keep only logs for 20 most recently active threads
    pub async fn cleanup(&self) -> Result<CleanupReport>;
}
```

**Cleanup trigger:** Periodic background task runs every 5 minutes by default (configurable via AppContext).

**Integration point:** After each turn completes in `Thread::execute_turn()`, append a `TurnLog` record.

### 6. argus-core Crate (AppContext Facade)

Single entry point for cli and desktop. Orchestrates all feature crates.

```rust
pub struct AppContext {
    db_pool: SqlitePool,
    template_manager: Arc<TemplateManager>,
    session_manager: Arc<SessionManager>,
    llm_manager: Arc<LLMManager>,
    tool_manager: Arc<ToolManager>,
    turn_log_repository: Arc<TurnLogRepository>,
    log_cleaner: Arc<LogCleaner>,
    shutdown: CancellationToken,
}

impl AppContext {
    // ========== Session Management ==========

    pub async fn create_session(&self, name: String) -> Result<SessionId>;
    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>>;
    pub async fn load_session(&self, session_id: SessionId) -> Result<()>;
    pub async fn unload_session(&self, session_id: SessionId) -> Result<()>;
    pub async fn delete_session(&self, session_id: SessionId) -> Result<()>;

    // ========== Thread Management ==========

    pub async fn create_thread(
        &self,
        session_id: SessionId,
        template_id: AgentId,
        provider_id: ProviderId,
        config: ThreadConfig,
    ) -> Result<ThreadId>;

    pub async fn list_threads(&self, session_id: SessionId) -> Result<Vec<ThreadSummary>>;
    pub async fn delete_thread(&self, session_id: SessionId, thread_id: ThreadId) -> Result<()>;

    // ========== Messaging ==========

    pub async fn send_message(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        content: String,
    ) -> Result<()>;

    pub async fn subscribe(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<broadcast::Receiver<ThreadEvent>>;

    // ========== Template Management ==========

    pub async fn upsert_template(&self, template: AgentTemplate) -> Result<AgentId>;
    pub async fn list_templates(&self) -> Result<Vec<AgentTemplate>>;
    pub async fn delete_template(&self, template_id: AgentId) -> Result<()>;
}
```

### 7. Migration Plan

Execute in order:

**Phase 1: Foundation**
1. Create `argus-protocol` crate
2. Move all public types from `claw/src/protocol/` to `argus-protocol`
3. Update `claw` to depend on `argus-protocol`

**Phase 2: Extract Feature Crates**
4. Create `argus-template`, extract TemplateManager logic from AgentManager
5. Create `argus-tool`, extract ToolManager and tools
6. Create `argus-llm`, extract LLMManager and providers
7. Create `argus-approval`, extract ApprovalManager
8. Create `argus-log` with TurnLogRepository + LogCleaner

**Phase 3: Session/Thread Refactor**
9. Create `sessions` table migration
10. Create a default "Legacy" session for existing threads
11. Add `session_id`, `template_id` columns to `threads` table (migrate existing threads to default session)
12. Create `turn_logs` table
13. Create `argus-session`, implement SessionManager + Session + Thread
14. Delete runtime Agent concept - Thread references template directly
15. Delete unused ThreadRepository

**Phase 4: Core Facade**
16. Rename `claw` → `argus-core`
17. Update AppContext API to session-based interface
18. Update `cli` and `desktop` to use new API

**Phase 5: Cleanup**
19. Remove old AgentManager code
20. Update all imports across crates
21. Run full test suite, fix any breaks

### 8. Testing Strategy

| Level | Scope | Location |
|-------|-------|----------|
| Unit | Each crate's internal logic | `#[cfg(test)]` in source files |
| Integration | Cross-crate interactions | `argus-core/tests/` |
| E2E | Full flow via AppContext | `cli/tests/` |

**Key test scenarios:**
- Session lifecycle: create → load → create thread → send message → unload → load → recover
- LRU cleanup: create 25 threads with logs, run cleanup, verify only 20 kept
- Concurrent sessions: multiple sessions active, independent messaging
- Recovery: reconstruct thread state from `turn_logs`
- Template isolation: modifying template doesn't affect existing threads

## Open Questions

None - all questions resolved during design session.

## Future Considerations

- **Session metadata:** May want to add description, tags, or other metadata to sessions
- **Thread archival:** Could add soft-delete or archive functionality for threads
- **Log export:** May want to export turn_logs for external analytics
- **Multi-agent per thread:** Schema supports future 1:N (Agent → Thread), currently 1:1
