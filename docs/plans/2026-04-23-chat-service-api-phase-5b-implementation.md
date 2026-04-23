# Phase 5B Chat Service API Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Complete the server-only Phase 5 chat service API with rename, model binding, snapshot, activation, and structured error handling.

**Architecture:** `argus-server` stays independent from `argus-wing`. Chat routes parse HTTP requests and call narrow `ServerCore` methods; `ServerCore` delegates to `SessionManager` for all chat/session semantics.

**Tech Stack:** Rust, axum, serde, tokio, `argus-session`, `argus-protocol`, existing `argus-server` integration test support.

---

### Task 1: Document Phase 5B Boundary

**Files:**
- Create: `docs/plans/2026-04-23-chat-service-api-phase-5b-design.md`
- Create: `docs/plans/2026-04-23-chat-service-api-phase-5b-implementation.md`
- Modify: `crates/argus-server/AGENTS.md`

**Step 1: Update server guidance**

Change Phase 5A wording to Phase 5 server-only chat REST wording.

**Step 2: Commit docs**

Run:

```bash
git add crates/argus-server/AGENTS.md docs/plans/2026-04-23-chat-service-api-phase-5b-design.md docs/plans/2026-04-23-chat-service-api-phase-5b-implementation.md
git commit -m "docs(server): plan phase 5b chat api"
```

### Task 2: Add Failing Phase 5B Tests

**Files:**
- Modify: `crates/argus-server/tests/chat_api.rs`

**Step 1: Write failing tests**

Cover:

- `PATCH /api/v1/chat/sessions/{session_id}` renames a session.
- `PATCH /api/v1/chat/sessions/{session_id}/threads/{thread_id}` renames a thread.
- `PATCH /api/v1/chat/sessions/{session_id}/threads/{thread_id}/model` updates provider/model and returns effective binding.
- `POST /api/v1/chat/sessions/{session_id}/threads/{thread_id}/activate` returns effective binding.
- `GET /api/v1/chat/sessions/{session_id}/threads/{thread_id}` returns a snapshot.
- invalid UUID path IDs return `400`.
- unknown thread/session lookups return `404`.

**Step 2: Run the targeted test**

```bash
cargo test -p argus-server --test chat_api -- --nocapture
```

Expected: FAIL because Phase 5B routes and error mapping are not implemented yet.

### Task 3: Add Structured API Errors

**Files:**
- Modify: `crates/argus-server/src/error.rs`

**Step 1: Add variants**

Add `BadRequest`, `NotFound`, and keep `Internal`.

**Step 2: Map `ArgusError`**

Map missing session/thread/template/provider errors to `NotFound`; keep all other errors as `Internal`.

### Task 4: Expose ServerCore Phase 5B Methods

**Files:**
- Modify: `crates/argus-server/src/server_core.rs`

**Step 1: Add methods**

Add:

- `rename_chat_session`
- `rename_chat_thread`
- `update_chat_thread_model`
- `get_chat_thread_snapshot`
- `activate_chat_thread`

**Step 2: Keep delegation narrow**

Each method should delegate to `SessionManager` and return DTO-ready values.

### Task 5: Add Phase 5B Routes

**Files:**
- Modify: `crates/argus-server/src/routes/chat.rs`
- Modify: `crates/argus-server/src/routes/mod.rs`

**Step 1: Add request/response DTOs**

Add:

- `RenameSessionRequest`
- `RenameThreadRequest`
- `UpdateThreadModelRequest`
- `ChatThreadSnapshotResponse`
- `ChatThreadBindingResponse`

**Step 2: Implement handlers**

Implement session rename, thread snapshot, thread rename, model update, and activation handlers.

**Step 3: Register routes**

Add routes under `/api/v1/chat/...` without changing existing Phase 5A paths.

### Task 6: Verify And Commit

**Files:**
- Modify: `crates/argus-server/src/error.rs`
- Modify: `crates/argus-server/src/server_core.rs`
- Modify: `crates/argus-server/src/routes/chat.rs`
- Modify: `crates/argus-server/src/routes/mod.rs`
- Modify: `crates/argus-server/tests/chat_api.rs`

**Step 1: Run targeted tests**

```bash
cargo test -p argus-server --test chat_api -- --nocapture
```

Expected: PASS.

**Step 2: Run full verification**

```bash
cargo test -p argus-server -- --nocapture
cd apps/web && pnpm exec vitest run
cd apps/web && pnpm build
cargo tree -p argus-server | rg argus-wing
```

Expected: server/web tests and build pass; `cargo tree | rg argus-wing` has no matches.

**Step 3: Commit implementation**

```bash
git add crates/argus-server/src/error.rs crates/argus-server/src/server_core.rs crates/argus-server/src/routes/chat.rs crates/argus-server/src/routes/mod.rs crates/argus-server/tests/chat_api.rs
git commit -m "feat(server): complete phase 5 chat api"
```
