# Phase 5A Chat Service API Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add the first server-only chat REST API over existing `SessionManager` capabilities.

**Architecture:** `argus-server` remains a peer application entry independent of `argus-wing`. Route handlers only call `ServerCore` methods, and `ServerCore` delegates to `SessionManager` for session/thread/message operations. The phase deliberately excludes web chat UI and thread event SSE.

**Tech Stack:** Rust, axum, serde, tokio, `argus-session`, `argus-protocol`, existing `argus-server` test support.

---

### Task 1: Document Phase 5A Boundary

**Files:**
- Modify: `crates/argus-server/AGENTS.md`
- Create: `docs/plans/2026-04-23-chat-service-api-design.md`
- Create: `docs/plans/2026-04-23-chat-service-api-implementation.md`

**Step 1: Update boundary text**

Replace the old blanket “do not expand chat/thread/message API” wording with a Phase 5A-specific allowance for narrow chat REST routes.

**Step 2: Commit docs**

Run:

```bash
git add crates/argus-server/AGENTS.md docs/plans/2026-04-23-chat-service-api-design.md docs/plans/2026-04-23-chat-service-api-implementation.md
git commit -m "docs(server): plan phase 5a chat api"
```

### Task 2: Add Failing Chat API Tests

**Files:**
- Create: `crates/argus-server/tests/chat_api.rs`

**Step 1: Write failing tests**

Cover:

- `POST /api/v1/chat/sessions` returns `201` with a session summary-like mutation response.
- `GET /api/v1/chat/sessions` returns the created session.
- `GET /api/v1/chat/sessions/{session_id}/threads` returns an empty list for a new session.
- `GET /api/v1/chat/sessions/{session_id}/threads/{thread_id}/messages` returns an error for an unknown thread.

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p argus-server --test chat_api -- --nocapture
```

Expected: FAIL because chat routes do not exist yet.

### Task 3: Expose ServerCore Chat Methods

**Files:**
- Modify: `crates/argus-server/src/server_core.rs`

**Step 1: Add narrow methods**

Add methods for:

- `list_chat_sessions`
- `create_chat_session`
- `delete_chat_session`
- `list_chat_threads`
- `create_chat_thread`
- `delete_chat_thread`
- `get_chat_messages`
- `send_chat_message`
- `cancel_chat_thread`

**Step 2: Keep methods delegated**

Each method should call the corresponding `SessionManager` method. Do not access repositories directly from chat routes.

### Task 4: Add Chat Routes

**Files:**
- Create: `crates/argus-server/src/routes/chat.rs`
- Modify: `crates/argus-server/src/routes/mod.rs`

**Step 1: Add route DTOs**

Request bodies:

- `CreateSessionRequest { name: String }`
- `CreateThreadRequest { template_id: i64, provider_id: Option<i64>, model: Option<String> }`
- `SendMessageRequest { message: String }`

**Step 2: Implement handlers**

Use existing `MutationResponse` and `DeleteResponse`. Return `201` for create session and create thread.

**Step 3: Register routes**

Add routes under `/api/v1/chat/...` in `routes::router`.

### Task 5: Verify And Commit

**Files:**
- Modify: `crates/argus-server/src/server_core.rs`
- Modify: `crates/argus-server/src/routes/mod.rs`
- Create: `crates/argus-server/src/routes/chat.rs`
- Create: `crates/argus-server/tests/chat_api.rs`

**Step 1: Run targeted tests**

```bash
cargo test -p argus-server --test chat_api -- --nocapture
```

Expected: PASS.

**Step 2: Run package tests**

```bash
cargo test -p argus-server -- --nocapture
```

Expected: PASS.

**Step 3: Check server independence**

```bash
cargo tree -p argus-server | rg argus-wing
```

Expected: no output.

**Step 4: Commit implementation**

```bash
git add crates/argus-server/src/server_core.rs crates/argus-server/src/routes/mod.rs crates/argus-server/src/routes/chat.rs crates/argus-server/tests/chat_api.rs
git commit -m "feat(server): add phase 5a chat api"
```
