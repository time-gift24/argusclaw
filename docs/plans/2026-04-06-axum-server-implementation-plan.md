# Axum Server Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a new `axum` server product with dev OAuth2, PostgreSQL-backed multi-user chat isolation, and shared-core reuse, while preserving the existing desktop product and login flow.

**Architecture:** Introduce server-specific auth and PostgreSQL persistence beside the existing desktop product, not in place of it. Shared runtime code stays in the existing crates, but gains cleaner service boundaries, user-aware ownership checks, and a dedicated provider token-credential abstraction so server OAuth2 users are no longer coupled to provider token exchange secrets.

**Tech Stack:** Rust, `axum`, `tokio`, `sqlx`, PostgreSQL 13.22, SSE, existing Argus workspace crates

---

### Task 1: Add shared protocol and repository abstractions

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/argus-protocol/src/lib.rs`
- Modify: `crates/argus-protocol/src/agent.rs`
- Create: `crates/argus-protocol/src/user.rs`
- Create: `crates/argus-protocol/src/provider_token_credential.rs`
- Modify: `crates/argus-repository/src/traits/mod.rs`
- Create: `crates/argus-repository/src/traits/user.rs`
- Create: `crates/argus-repository/src/traits/provider_token_credential.rs`
- Modify: `crates/argus-repository/src/types/mod.rs`
- Create: `crates/argus-repository/src/types/user.rs`

**Step 1: Write the failing tests**

Add unit tests that assert:

- `AgentRecord` or its persisted representation exposes `is_enabled`
- `OAuth2Identity`/user records round-trip through serde
- provider token credential types can represent encrypted username/password credentials

Suggested test targets:

- `crates/argus-protocol/src/agent.rs`
- `crates/argus-protocol/src/user.rs`
- `crates/argus-protocol/src/provider_token_credential.rs`

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-protocol`

Expected: FAIL because the new modules, fields, or tests do not exist yet.

**Step 3: Write minimal implementation**

Implement:

- `user` protocol types for server OAuth2 users
- `provider_token_credential` protocol or repository-facing types
- `is_enabled: bool` on the agent template shape with a conservative default for existing records
- repository traits for users and provider token credentials

Keep the new API surface small:

- no production OAuth2 logic yet
- no server handlers yet
- no repository implementation in this task

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-protocol`

Expected: PASS

**Step 5: Commit**

```bash
git add Cargo.toml \
  crates/argus-protocol/src/lib.rs \
  crates/argus-protocol/src/agent.rs \
  crates/argus-protocol/src/user.rs \
  crates/argus-protocol/src/provider_token_credential.rs \
  crates/argus-repository/src/traits/mod.rs \
  crates/argus-repository/src/traits/user.rs \
  crates/argus-repository/src/traits/provider_token_credential.rs \
  crates/argus-repository/src/types/mod.rs \
  crates/argus-repository/src/types/user.rs
git commit -m "feat: add shared server auth and credential abstractions"
```

### Task 2: Decouple provider token exchange from desktop account login

**Files:**
- Modify: `crates/argus-auth/src/token.rs`
- Modify: `crates/argus-auth/src/lib.rs`
- Modify: `crates/argus-llm/src/manager.rs`
- Modify: `crates/argus-auth/src/account.rs`
- Test: `crates/argus-llm/src/test_utils.rs`
- Create: `crates/argus-llm/tests/provider_token_credentials.rs`

**Step 1: Write the failing test**

Create integration tests that prove:

- `TokenLLMProvider` can obtain token-exchange credentials from a dedicated credential source
- `ProviderManager` can still construct token-backed providers without reading desktop login credentials
- existing desktop account login behavior is unchanged by this refactor

Include one regression test covering the old `account_token_source` metadata path and one test covering the new provider credential source path.

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-llm provider_token_credentials -- --nocapture`

Expected: FAIL because `ProviderManager` still depends on `AccountRepository` for token exchange.

**Step 3: Write minimal implementation**

Refactor the dependency chain so that:

- `TokenLLMProvider` keeps token caching and header injection
- `ProviderManager::with_auth(...)` is replaced or supplemented by a more accurate token-credential injection API
- `AccountRepository` is no longer the server-facing credential source
- desktop account logic remains intact

Acceptable intermediate compatibility:

- keep the old metadata key if it reduces churn
- bridge desktop through an adapter if necessary

Do not add server code in this task.

**Step 4: Run test to verify it passes**

Run:

- `cargo test -p argus-llm provider_token_credentials -- --nocapture`
- `cargo test -p argus-auth`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-auth/src/token.rs \
  crates/argus-auth/src/lib.rs \
  crates/argus-llm/src/manager.rs \
  crates/argus-auth/src/account.rs \
  crates/argus-llm/tests/provider_token_credentials.rs
git commit -m "refactor: separate provider token credentials from account login"
```

### Task 3: Add PostgreSQL support to argus-repository

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/argus-repository/Cargo.toml`
- Modify: `crates/argus-repository/src/lib.rs`
- Create: `crates/argus-repository/src/postgres/mod.rs`
- Create: `crates/argus-repository/src/postgres/user.rs`
- Create: `crates/argus-repository/src/postgres/session.rs`
- Create: `crates/argus-repository/src/postgres/thread.rs`
- Create: `crates/argus-repository/src/postgres/job.rs`
- Create: `crates/argus-repository/src/postgres/llm_provider.rs`
- Create: `crates/argus-repository/src/postgres/provider_token_credential.rs`
- Create: `crates/argus-repository/src/postgres/mcp.rs`
- Create: `crates/argus-repository/src/postgres/agent.rs`
- Create: `crates/argus-repository/migrations/20260406xxxxxx_add_server_users_and_provider_credentials.sql`
- Create: `crates/argus-repository/tests/postgres_user_isolation.rs`

**Step 1: Write the failing test**

Write repository integration tests that prove:

- users can be upserted by `external_subject`
- sessions and threads are only listed for the owning user
- provider token credentials can be read back for a provider
- `agent_templates.is_enabled` defaults correctly for migrated rows

Use a PostgreSQL test database configured through environment variables. Keep setup code in the test module, not duplicated across each test.

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-repository postgres_user_isolation -- --nocapture`

Expected: FAIL because PostgreSQL modules, migrations, and implementations do not exist yet.

**Step 3: Write minimal implementation**

Implement:

- Postgres connection/export path alongside existing SQLite support
- user and provider token credential repositories
- owner-aware session/thread/job queries
- `is_enabled` persistence for agent templates

Keep all SQL inside `argus-repository`.

If both SQLite and PostgreSQL use shared row-mapping logic, extract helpers only when it clearly reduces duplication.

**Step 4: Run test to verify it passes**

Run:

- `cargo test -p argus-repository postgres_user_isolation -- --nocapture`
- `cargo test -p argus-repository`

Expected: PASS

**Step 5: Commit**

```bash
git add Cargo.toml \
  crates/argus-repository/Cargo.toml \
  crates/argus-repository/src/lib.rs \
  crates/argus-repository/src/postgres \
  crates/argus-repository/migrations \
  crates/argus-repository/tests/postgres_user_isolation.rs
git commit -m "feat: add postgres repositories for server runtime"
```

### Task 4: Extract shared user chat services from desktop-shaped entrypoints

**Files:**
- Modify: `crates/argus-wing/src/lib.rs`
- Modify: `crates/argus-wing/src/db.rs`
- Modify: `crates/argus-session/src/manager.rs`
- Modify: `crates/argus-session/src/lib.rs`
- Create: `crates/argus-session/src/user_chat_services.rs`
- Create: `crates/argus-session/tests/user_chat_services.rs`

**Step 1: Write the failing test**

Add service-level tests that verify:

- listing sessions/threads respects a passed-in user context
- sending a message only succeeds when the thread belongs to that user
- enabled-agent filtering happens at the service boundary used by server

Use focused tests against the service API rather than full HTTP.

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-session user_chat_services -- --nocapture`

Expected: FAIL because the user-aware service boundary does not exist yet.

**Step 3: Write minimal implementation**

Extract or add a service that accepts an authenticated principal or user context for:

- list enabled agents
- create session
- list sessions
- list threads in session
- inspect thread snapshot
- send message
- cancel work

Requirements:

- desktop call paths must continue to work
- avoid forcing OAuth2 abstractions into desktop
- avoid expanding `ArgusWing` with server-only auth logic

**Step 4: Run test to verify it passes**

Run:

- `cargo test -p argus-session user_chat_services -- --nocapture`
- `cargo test -p argus-wing`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-wing/src/lib.rs \
  crates/argus-wing/src/db.rs \
  crates/argus-session/src/manager.rs \
  crates/argus-session/src/lib.rs \
  crates/argus-session/src/user_chat_services.rs \
  crates/argus-session/tests/user_chat_services.rs
git commit -m "refactor: add shared user chat services for server"
```

### Task 5: Implement server auth abstractions and dev OAuth2 flow

**Files:**
- Create: `crates/argus-server/Cargo.toml`
- Create: `crates/argus-server/CLAUDE.md`
- Create: `crates/argus-server/src/lib.rs`
- Create: `crates/argus-server/src/main.rs`
- Create: `crates/argus-server/src/config.rs`
- Create: `crates/argus-server/src/state.rs`
- Create: `crates/argus-server/src/auth/mod.rs`
- Create: `crates/argus-server/src/auth/provider.rs`
- Create: `crates/argus-server/src/auth/dev_oauth.rs`
- Create: `crates/argus-server/src/auth/session.rs`
- Create: `crates/argus-server/src/auth/routes.rs`
- Create: `crates/argus-server/tests/dev_oauth_flow.rs`
- Modify: `Cargo.toml`

**Step 1: Write the failing test**

Create auth integration tests covering:

- `GET /auth/login` redirects to the dev authorize route
- authorize form submission leads to callback with a code and state
- callback upserts the user and establishes a cookie session
- `GET /api/me` returns the authenticated user

Keep tests end-to-end at the router layer using `axum` test helpers or an in-process HTTP client.

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-server dev_oauth_flow -- --nocapture`

Expected: FAIL because the crate and auth routes do not exist yet.

**Step 3: Write minimal implementation**

Implement:

- server crate wiring
- `OAuth2AuthProvider` trait and `DevOAuth2Provider`
- cookie-backed auth session handling
- `/auth/login`
- `/dev-oauth/authorize`
- `/auth/callback`
- `/auth/logout`
- `/api/me`

Keep the dev OAuth2 page intentionally simple. Do not build a production UI here.

**Step 4: Run test to verify it passes**

Run:

- `cargo test -p argus-server dev_oauth_flow -- --nocapture`

Expected: PASS

**Step 5: Commit**

```bash
git add Cargo.toml \
  crates/argus-server
git commit -m "feat: add axum server auth and dev oauth flow"
```

### Task 6: Implement user chat HTTP API and SSE

**Files:**
- Create: `crates/argus-server/src/routes/mod.rs`
- Create: `crates/argus-server/src/routes/agents.rs`
- Create: `crates/argus-server/src/routes/sessions.rs`
- Create: `crates/argus-server/src/routes/threads.rs`
- Create: `crates/argus-server/src/routes/events.rs`
- Create: `crates/argus-server/src/http/error.rs`
- Create: `crates/argus-server/tests/chat_api.rs`
- Modify: `crates/argus-server/src/lib.rs`
- Modify: `crates/argus-server/src/state.rs`

**Step 1: Write the failing test**

Add API tests covering:

- `GET /api/agents` only returns enabled agents
- `POST /api/sessions` creates a user-owned session
- `GET /api/sessions` and `GET /api/sessions/:id/threads` only show owned data
- `POST /api/threads/:thread_id/messages` starts work
- `GET /api/threads/:thread_id/events` streams at least one event for owned threads
- cross-user access is rejected

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-server chat_api -- --nocapture`

Expected: FAIL because the chat routes do not exist yet.

**Step 3: Write minimal implementation**

Wire the HTTP layer to the shared user chat services:

- convert authenticated request state into a user context
- keep handler code thin
- map service errors to HTTP responses consistently
- implement SSE using the existing `ThreadEvent` stream

Do not add admin/provider/MCP routes in this task.

**Step 4: Run test to verify it passes**

Run:

- `cargo test -p argus-server chat_api -- --nocapture`
- `cargo test -p argus-server`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-server/src \
  crates/argus-server/tests/chat_api.rs
git commit -m "feat: add server chat api and event streaming"
```

### Task 7: Verify desktop regressions and document server startup

**Files:**
- Modify: `README.md`
- Modify: `crates/desktop/README.md`
- Create: `crates/argus-server/README.md`
- Create: `docs/plans/2026-04-06-axum-server-rollout-checklist.md`

**Step 1: Write the failing test or checklist**

Create a rollout checklist that explicitly requires verification of:

- desktop login still works
- desktop provider token flow still works
- desktop provider/MCP/agent management remains available
- server dev OAuth2 login works
- server user isolation works

If documentation examples are executable, add at least one smoke test command to the checklist.

**Step 2: Run verification commands**

Run:

- `cargo test -q`
- any focused server smoke command added in docs

Expected: PASS

**Step 3: Write minimal implementation**

Update docs so a new engineer can:

- run PostgreSQL locally
- configure server env vars
- start `argus-server`
- exercise the dev OAuth2 flow
- understand that desktop and server are separate products with shared core

**Step 4: Re-run verification**

Run:

- `cargo test -q`

Expected: PASS

**Step 5: Commit**

```bash
git add README.md \
  crates/desktop/README.md \
  crates/argus-server/README.md \
  docs/plans/2026-04-06-axum-server-rollout-checklist.md
git commit -m "docs: add server rollout and product boundary guidance"
```
