# Thread Module Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Introduce Thread as a multi-turn conversation session that manages Turn execution, context compaction, approval flow, and event broadcasting.

**Architecture:** Thread wraps message history and executes Turns sequentially. It broadcasts events (Processing/Idle) via tokio broadcast channel to CLI and Tauri subscribers. Auto-compacts context when approaching token threshold. Integrates approval flow via Hook mechanism.

**Tech Stack:** Rust, tokio (async, broadcast channel), derive_builder, uuid, thiserror

---

## Design Summary

### Core Types

| Type | Description |
|------|-------------|
| `ThreadId` | Strongly-typed UUID wrapper |
| `ThreadState` | `Idle` \| `Processing` |
| `ThreadEvent` | Processing/Completed/Failed/Idle events |
| `ThreadConfig` | compact_threshold_ratio, compact_strategy, turn_config |
| `CompactStrategy` | KeepRecent / KeepTokens / Summarize |
| `ThreadError` | TurnFailed, CompactFailed, etc. |

### Thread Structure

```rust
pub struct Thread {
    pub id: ThreadId,
    pub messages: Vec<ChatMessage>,
    pub token_count: u32,
    pub turn_count: u32,
    pub config: ThreadConfig,
    pub provider: Arc<dyn LlmProvider>,
    pub tool_manager: Arc<ToolManager>,
    pub approval_manager: Option<Arc<ApprovalManager>>,
    pub repository: Option<Arc<dyn ThreadRepository>>,
    event_sender: broadcast::Sender<ThreadEvent>,
}
```

### Key Decisions

1. **No internal locks** - Turn executes sequentially, caller ensures synchronization
2. **Builder pattern** - Use `derive_builder` for Thread and ThreadConfig
3. **Streaming Turn** - `execute_turn_streaming` returns event receiver, non-blocking
4. **Approval via Hook** - `ApprovalHookHandler` blocks on approval requests
5. **Hybrid persistence** - In-memory messages + async background persistence

---

## Task 1: Create Thread Module Structure

**Files:**
- Create: `crates/claw/src/agents/thread/mod.rs`
- Create: `crates/claw/src/agents/thread/types.rs`
- Create: `crates/claw/src/agents/thread/error.rs`
- Modify: `crates/claw/src/agents/mod.rs`

**Step 1: Create types.rs with core types**

```rust
//! Thread core types.

use uuid::Uuid;
use crate::llm::LlmStreamEvent;
use crate::agents::turn::TokenUsage;

/// Unique identifier for a Thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ThreadId(pub Uuid);

impl ThreadId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ThreadId {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThreadState {
    #[default]
    Idle,
    Processing,
}

/// Thread event broadcast to subscribers.
#[derive(Debug, Clone)]
pub enum ThreadEvent {
    /// Turn is processing, streaming LLM/tool events.
    Processing {
        thread_id: ThreadId,
        turn_number: u32,
        event: LlmStreamEvent,
    },
    /// Turn completed successfully.
    TurnCompleted {
        thread_id: ThreadId,
        turn_number: u32,
        token_usage: TokenUsage,
    },
    /// Turn failed.
    TurnFailed {
        thread_id: ThreadId,
        turn_number: u32,
        error: String,
    },
    /// Thread entered idle state.
    Idle {
        thread_id: ThreadId,
    },
    /// Context was compacted.
    Compacted {
        thread_id: ThreadId,
        new_token_count: u32,
    },
}
```

**Step 2: Run format and clippy**

Run: `cargo fmt && cargo clippy --all`
Expected: No errors

**Step 3: Commit**

```bash
git add crates/claw/src/agents/thread/types.rs
git commit -m "feat(thread): add ThreadId, ThreadState, ThreadEvent types"
```

---

## Task 2: Create ThreadError

**Files:**
- Create: `crates/claw/src/agents/thread/error.rs`

**Step 1: Write error types**

```rust
//! Thread error types.

use thiserror::Error;
use crate::agents::turn::TurnError;

/// Errors that can occur during Thread operations.
#[derive(Debug, Error)]
pub enum ThreadError {
    /// Turn execution failed.
    #[error("Turn execution failed: {0}")]
    TurnFailed(#[from] TurnError),

    /// Compact operation failed.
    #[error("Compact failed: {reason}")]
    CompactFailed { reason: String },

    /// Provider not configured.
    #[error("LLM provider not configured")]
    ProviderNotConfigured,

    /// Channel send error.
    #[error("Event channel closed")]
    ChannelClosed,
}
```

**Step 2: Commit**

```bash
git add crates/claw/src/agents/thread/error.rs
git commit -m "feat(thread): add ThreadError type"
```

---

## Task 3: Create ThreadConfig with CompactStrategy

**Files:**
- Create: `crates/claw/src/agents/thread/config.rs`

**Step 1: Write config types**

```rust
//! Thread configuration.

use std::sync::Arc;
use derive_builder::Builder;
use crate::agents::turn::TurnConfig;
use crate::llm::LlmProvider;

/// Strategy for compacting thread context.
#[derive(Debug, Clone)]
pub enum CompactStrategy {
    /// Keep the most recent N messages.
    KeepRecent { count: usize },
    /// Keep messages within N% of token budget.
    KeepTokens { ratio: f32 },
    /// Use LLM to summarize history.
    Summarize {
        max_summary_tokens: u32,
        provider: Arc<dyn LlmProvider>,
    },
}

impl Default for CompactStrategy {
    fn default() -> Self {
        Self::KeepRecent { count: 50 }
    }
}

/// Thread configuration.
#[derive(Debug, Clone, Builder)]
pub struct ThreadConfig {
    /// Token threshold ratio to trigger compact (e.g., 0.8 = 80%).
    #[builder(default = 0.8)]
    pub compact_threshold_ratio: f32,

    /// Strategy for compacting context.
    #[builder(default)]
    pub compact_strategy: CompactStrategy,

    /// Underlying Turn configuration.
    #[builder(default)]
    pub turn_config: TurnConfig,
}

impl Default for ThreadConfig {
    fn default() -> Self {
        ThreadConfigBuilder::default().build().unwrap()
    }
}
```

**Step 2: Commit**

```bash
git add crates/claw/src/agents/thread/config.rs
git commit -m "feat(thread): add ThreadConfig and CompactStrategy"
```

---

## Task 4: Create Thread Module Entry

**Files:**
- Create: `crates/claw/src/agents/thread/mod.rs`
- Modify: `crates/claw/src/agents/mod.rs`

**Step 1: Create mod.rs**

```rust
//! Thread module - multi-turn conversation session management.

mod config;
mod error;
mod thread;
mod types;

pub use config::{CompactStrategy, ThreadConfig, ThreadConfigBuilder};
pub use error::ThreadError;
pub use thread::{Thread, ThreadBuilder, TurnStreamHandle};
pub use types::{ThreadId, ThreadState, ThreadEvent};
```

**Step 2: Update agents/mod.rs**

```rust
// Add to existing file
pub mod thread;

// Re-export
pub use thread::{Thread, ThreadBuilder, ThreadConfig, ThreadError, ThreadEvent, ThreadId, ThreadState};
```

**Step 3: Commit**

```bash
git add crates/claw/src/agents/thread/mod.rs crates/claw/src/agents/mod.rs
git commit -m "feat(thread): create thread module structure"
```

---

## Task 5: Implement Thread Builder

**Files:**
- Create: `crates/claw/src/agents/thread/thread.rs` (part 1)

**Step 1: Write Thread struct and builder**

```rust
//! Thread implementation.

use std::sync::Arc;
use derive_builder::Builder;
use tokio::sync::{broadcast, oneshot};
use uuid::Uuid;

use crate::llm::{ChatMessage, LlmProvider, LlmStreamEvent};
use crate::tool::ToolManager;
use crate::approval::ApprovalManager;
use crate::agents::turn::{TokenUsage, TurnConfig, TurnInputBuilder, TurnOutput, TurnError};

use super::{CompactStrategy, ThreadConfig, ThreadError, ThreadEvent, ThreadId, ThreadState};

/// Handle for receiving Turn execution events.
pub struct TurnStreamHandle {
    pub thread_id: ThreadId,
    pub turn_number: u32,
    /// Raw LLM events during processing.
    pub llm_events: broadcast::Receiver<LlmStreamEvent>,
    /// Final result when Turn completes.
    pub result: oneshot::Receiver<Result<TurnOutput, TurnError>>,
    /// Thread event broadcaster.
    thread_event_sender: broadcast::Sender<ThreadEvent>,
}

/// Thread - multi-turn conversation session.
#[derive(Builder)]
#[builder(pattern = "owned", build_fn(skip))]
pub struct Thread {
    /// Unique identifier.
    #[builder(default = ThreadId::new())]
    pub id: ThreadId,

    /// Initial message history (for restoring sessions).
    #[builder(default)]
    pub messages: Vec<ChatMessage>,

    /// LLM provider (required).
    pub provider: Arc<dyn LlmProvider>,

    /// Tool manager.
    #[builder(default = Arc::new(ToolManager::new()))]
    pub tool_manager: Arc<ToolManager>,

    /// Approval manager (optional).
    #[builder(default, setter(strip_option))]
    pub approval_manager: Option<Arc<ApprovalManager>>,

    /// Persistence repository (optional).
    #[builder(default, setter(strip_option))]
    pub repository: Option<Arc<dyn ThreadRepository>>,

    /// Thread configuration.
    #[builder(default)]
    pub config: ThreadConfig,

    // Internal fields (not in builder)
    #[builder(default = 0)]
    token_count: u32,

    #[builder(default = 0)]
    turn_count: u32,

    #[builder(default = "broadcast::channel(256).0")]
    event_sender: broadcast::Sender<ThreadEvent>,
}

impl ThreadBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build(self) -> Thread {
        let (event_sender, _) = broadcast::channel(256);

        Thread {
            id: self.id.unwrap_or_else(ThreadId::new),
            messages: self.messages.unwrap_or_default(),
            provider: self.provider.expect("provider is required"),
            tool_manager: self.tool_manager
                .unwrap_or_else(|| Arc::new(ToolManager::new())),
            approval_manager: self.approval_manager.flatten(),
            repository: self.repository.flatten(),
            config: self.config.unwrap_or_default(),
            token_count: 0,
            turn_count: 0,
            event_sender,
        }
    }
}
```

**Step 2: Commit**

```bash
git add crates/claw/src/agents/thread/thread.rs
git commit -m "feat(thread): add Thread struct and builder"
```

---

## Task 6: Implement Thread Methods

**Files:**
- Modify: `crates/claw/src/agents/thread/thread.rs`

**Step 1: Add impl block with methods**

```rust
impl Thread {
    /// Subscribe to Thread events.
    pub fn subscribe(&self) -> broadcast::Receiver<ThreadEvent> {
        self.event_sender.subscribe()
    }

    /// Get current state.
    pub fn state(&self) -> ThreadState {
        // Simplified: if turn_count > 0 and last message not from assistant, Processing
        ThreadState::Idle
    }

    /// Get message history (read-only).
    pub fn history(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Check if compact is needed.
    pub fn should_compact(&self) -> bool {
        let context_window = 128_000; // TODO: get from provider.metadata
        let threshold = (context_window as f32 * self.config.compact_threshold_ratio) as u32;
        self.token_count >= threshold
    }

    /// Send user message and execute Turn.
    pub async fn send_message(&mut self, user_input: String) -> TurnStreamHandle {
        // Check compact
        if self.should_compact() {
            // TODO: implement compact
        }

        // Add user message
        self.messages.push(ChatMessage::user(user_input));

        // Execute Turn
        self.execute_turn_streaming().await
    }

    async fn execute_turn_streaming(&mut self) -> TurnStreamHandle {
        self.turn_count += 1;
        let turn_number = self.turn_count;
        let thread_id = self.id;

        // Build TurnInput
        let turn_input = TurnInputBuilder::new()
            .provider(self.provider.clone())
            .messages(self.messages.clone())
            .tool_manager(self.tool_manager.clone())
            .tool_ids(self.tool_manager.list_ids())
            .build();

        // Create channels
        let (event_tx, event_rx) = broadcast::channel(256);
        let (result_tx, result_rx) = oneshot::channel();

        let config = self.config.turn_config.clone();

        tokio::spawn(async move {
            // TODO: actual streaming execution
            // For now, use blocking execute_turn
            let _ = result_tx.send(Ok(TurnOutput {
                messages: vec![],
                token_usage: TokenUsage::default(),
            }));
        });

        TurnStreamHandle {
            thread_id,
            turn_number,
            llm_events: event_rx,
            result: result_rx,
            thread_event_sender: self.event_sender.clone(),
        }
    }
}
```

**Step 2: Commit**

```bash
git add crates/claw/src/agents/thread/thread.rs
git commit -m "feat(thread): add subscribe, send_message methods"
```

---

## Task 7: Implement Compact Strategy

**Files:**
- Modify: `crates/claw/src/agents/thread/thread.rs`

**Step 1: Add compact method**

```rust
impl Thread {
    /// Compact the message history.
    pub async fn compact(&mut self) -> Result<(), ThreadError> {
        match &self.config.compact_strategy {
            CompactStrategy::KeepRecent { count } => {
                let system_msgs: Vec<_> = self.messages.iter()
                    .filter(|m| m.role == crate::llm::Role::System)
                    .cloned()
                    .collect();

                let recent: Vec<_> = self.messages.iter()
                    .rev()
                    .take(*count)
                    .rev()
                    .cloned()
                    .collect();

                self.messages = [system_msgs, recent].concat();
                self.recalculate_token_count();
            }
            CompactStrategy::KeepTokens { ratio } => {
                let target_tokens = (self.token_count as f32 * ratio) as usize;
                self.truncate_to_token_budget(target_tokens);
            }
            CompactStrategy::Summarize { .. } => {
                // TODO: implement LLM summarization
                return Err(ThreadError::CompactFailed {
                    reason: "Summarize strategy not yet implemented".to_string(),
                });
            }
        }

        // Broadcast compacted event
        let _ = self.event_sender.send(ThreadEvent::Compacted {
            thread_id: self.id,
            new_token_count: self.token_count,
        });

        Ok(())
    }

    fn recalculate_token_count(&mut self) {
        // Simple estimation: ~4 chars per token
        self.token_count = self.messages.iter()
            .map(|m| (m.content.len() / 4) as u32)
            .sum();
    }

    fn truncate_to_token_budget(&mut self, target_tokens: usize) {
        // Keep truncating from the front until under budget
        while self.token_count > target_tokens as u32 && self.messages.len() > 1 {
            // Don't remove system messages
            if self.messages.first().map(|m| m.role) == Some(crate::llm::Role::System) {
                if self.messages.len() > 1 && self.messages[1].role != crate::llm::Role::System {
                    self.messages.remove(1);
                } else {
                    break;
                }
            } else {
                self.messages.remove(0);
            }
            self.recalculate_token_count();
        }
    }
}
```

**Step 2: Commit**

```bash
git add crates/claw/src/agents/thread/thread.rs
git commit -m "feat(thread): implement compact strategies"
```

---

## Task 8: Add ToolManager::list_ids Method

**Files:**
- Modify: `crates/claw/src/tool/mod.rs`

**Step 1: Add list_ids method to ToolManager**

Find the ToolManager impl block and add:

```rust
/// List all registered tool IDs.
pub fn list_ids(&self) -> Vec<String> {
    self.tools.iter().map(|entry| entry.key().clone()).collect()
}
```

**Step 2: Commit**

```bash
git add crates/claw/src/tool/mod.rs
git commit -m "feat(tool): add ToolManager::list_ids method"
```

---

## Task 9: Add Unit Tests

**Files:**
- Modify: `crates/claw/src/agents/thread/types.rs`
- Modify: `crates/claw/src/agents/thread/thread.rs`

**Step 1: Add tests to types.rs**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_id_new_creates_unique_ids() {
        let id1 = ThreadId::new();
        let id2 = ThreadId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn thread_state_default_is_idle() {
        assert_eq!(ThreadState::default(), ThreadState::Idle);
    }
}
```

**Step 2: Add tests to thread.rs**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn mock_provider() -> Arc<dyn LlmProvider> {
        // Use a simple mock or stub
        // For now, this will need a real implementation
        unimplemented!("Add mock provider for tests")
    }

    #[test]
    fn thread_builder_requires_provider() {
        let result = std::panic::catch_unwind(|| {
            ThreadBuilder::new().build()
        });
        assert!(result.is_err());
    }
}
```

**Step 3: Run tests**

Run: `cargo test --lib`
Expected: Tests pass (or skip mock-dependent tests)

**Step 4: Commit**

```bash
git add crates/claw/src/agents/thread/types.rs crates/claw/src/agents/thread/thread.rs
git commit -m "test(thread): add unit tests for ThreadId and ThreadState"
```

---

## Task 10: Run Full Test Suite

**Step 1: Format and lint**

Run: `cargo fmt && cargo clippy --all --benches --tests --examples --all-features`
Expected: Zero warnings

**Step 2: Run all tests**

Run: `cargo test`
Expected: All tests pass

**Step 3: Final commit (if any fixes needed)**

```bash
git add -A
git commit -m "fix(thread): address clippy warnings and test failures"
```

---

## Files Summary

### Created Files
- `crates/claw/src/agents/thread/mod.rs`
- `crates/claw/src/agents/thread/types.rs`
- `crates/claw/src/agents/thread/error.rs`
- `crates/claw/src/agents/thread/config.rs`
- `crates/claw/src/agents/thread/thread.rs`

### Modified Files
- `crates/claw/src/agents/mod.rs`
- `crates/claw/src/tool/mod.rs`

---

## Future Enhancements (Out of Scope)

1. **Streaming Turn execution** - Implement `execute_turn_streaming` with actual LLM streaming
2. **Approval Hook integration** - Wire up `ApprovalHookHandler` in Turn execution
3. **Persistence** - Implement `ThreadRepository` trait and SQLite backend
4. **Summarize compact strategy** - Implement LLM-based summarization
5. **Token counting** - Use tiktoken for accurate token counts
