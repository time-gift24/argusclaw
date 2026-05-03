# RALPLAN-DR Revised Plan: app/web Multi-user PostgreSQL

## Source Requirements

- Input spec: `.omx/specs/deep-interview-app-web-multi-user-postgresql.md`
- Mode: `$plan --consensus --direct`, deliberate because the work touches auth-adjacent request identity, cross-user data isolation, and database backend replacement.
- Stop rule: planning output only; no source implementation in this workflow.

## Requirements Summary

Implement a first-pass multi-user `apps/web` + `argus-server` deployment backed by PostgreSQL. The only user-isolated product data in this pass is chat sessions, threads, and messages. Current chat user identity comes from trusted request headers now; later OAuth2 should populate the same server-side user context. OAuth2, temporary login, frontend user switcher, workspace/team model, user-scoped providers/templates/MCP/jobs/runtime, SQLite data migration, and SQLite runtime support for server/web are out of scope.

Important boundary refinement from consensus review: **PostgreSQL-only applies to `argus-server`/web runtime, not to deleting SQLite from the workspace.** Existing SQLite codepaths such as `argus-repository::sqlite`, `ArgusSqlite`, `argus-wing`, and `TemplateManager` must continue to compile for non-server consumers during this change.

## Evidence Snapshot

- `crates/argus-server/src/db.rs:10-20` treats only `sqlite:` as a URL and otherwise expands to a filesystem path; `crates/argus-server/src/db.rs:43-45` defaults `DATABASE_URL` to `~/.arguswing/sqlite.db`.
- `crates/argus-server/src/server_core.rs:20-27` imports `ArgusSqlite`, SQLite connect/migrate helpers, and `SqlitePool`; `crates/argus-server/src/server_core.rs:136-154` initializes from a SQLite pool; `crates/argus-server/src/server_core.rs:154-214` wires repositories from `ArgusSqlite`.
- `crates/argus-repository/src/lib.rs:1-7` describes repository traits plus SQLite implementations; `crates/argus-repository/src/lib.rs:19-20` re-exports SQLite concrete helpers.
- `crates/argus-repository/src/sqlite/mod.rs:30-73` provides SQLite-only connect/migrate helpers; `crates/argus-repository/src/sqlite/mod.rs:75-83` defines `ArgusSqlite` around `SqlitePool`.
- `Cargo.toml:43` enables workspace `sqlx` with SQLite features; `crates/argus-repository/Cargo.toml:12` consumes workspace `sqlx` features.
- `crates/argus-repository/migrations/20260325120105_init.sql:132-140` defines single-user `accounts` with `CHECK (id = 1)`.
- `crates/argus-repository/migrations/20260325120105_init.sql:143-150` declares `sessions.id` as `INTEGER PRIMARY KEY AUTOINCREMENT`, while `crates/argus-repository/src/sqlite/session.rs:31-50` binds `SessionId` as a UUID string; `crates/argus-repository/src/sqlite/session.rs:89-99` parses session IDs from strings.
- `crates/argus-repository/migrations/20260325120105_init.sql:67-83` defines `threads.session_id` against the SQLite sessions schema, while `crates/argus-repository/src/sqlite/thread.rs:14-18` and `:85-92` bind `SessionId` strings.
- `crates/argus-repository/src/traits/session.rs:18-32` and `crates/argus-repository/src/traits/thread.rs:11-79` have no user parameter.
- `crates/argus-session/src/manager.rs:1121-1147` lists sessions globally; `:1209-1218` creates sessions without owner; `:1541-1568` lists threads by session only; `:1585-1668` sends/reads messages by session/thread only.
- `crates/argus-session/src/manager.rs:1150-1153` returns a cached session before repository validation; `:146-149` and `:634-640` show thread/session caches keyed by thread/session IDs only.
- `crates/argus-server/src/routes/mod.rs:33-73` exposes chat routes without user path segment; `crates/argus-server/src/routes/chat.rs:526-721` passes only path/body fields to `ServerCore`.
- `apps/web/src/lib/api.ts:674-833` calls chat REST/SSE endpoints without user context; `apps/web/src/lib/api.ts:828-860` constructs native `EventSource`, which cannot send arbitrary trusted headers.
- `crates/argus-template/src/manager.rs:5-12`, `:78-80`, and `:107-110` couple `TemplateManager` repair operations to concrete `ArgusSqlite`; `crates/argus-wing/src/lib.rs:49-53`, `:112-141`, and `:207-223` are SQLite-shaped and must still compile.

## RALPLAN-DR Summary

### Principles

1. **Fail-closed tenant boundary:** missing or invalid current-user context must not expose global chat data.
2. **Repository owns SQL and ownership predicates:** all SQL and ownership joins/predicates stay in `argus-repository`; upper layers pass typed context.
3. **Smallest product scope:** isolate only sessions/threads/messages; shared providers/templates/MCP/jobs/runtime remain unchanged unless compile-time plumbing requires adjustment.
4. **Future OAuth2 seam, no OAuth2 implementation:** trusted-header identity enters through a replaceable server extractor; no login/session-cookie UX.
5. **Server/web PostgreSQL runtime with workspace SQLite compile coexistence:** `argus-server` runtime becomes Postgres-only, while existing SQLite codepaths continue compiling for non-server consumers.

### Decision Drivers

1. Prevent cross-user read/write/SSE access to chat data.
2. Replace the server/web runtime repository path with PostgreSQL without leaking concrete DB types above repository boundaries.
3. Keep scope narrow and testable while preserving non-server build compatibility.

### Viable Options

#### Option A — PostgreSQL server runtime + user-scoped chat repository traits + SQLite compile coexistence (favored)

Approach: Add `UserId`/`RequestUser`, PostgreSQL migrations and `ArgusPostgres`, user-scoped session/thread repository traits, and server/session propagation for chat operations. `argus-server` rejects non-Postgres runtime DB URLs. Existing SQLite modules remain available so `argus-wing`, `TemplateManager`, and non-server tests still compile.

Pros:
- Satisfies PostgreSQL-only server/web runtime and per-user chat isolation.
- Ownership predicates live in repository queries.
- Avoids workspace-wide SQLite deletion and keeps the first pass bounded.
- Creates a clean OAuth2 replacement seam.

Cons:
- Broader trait and test churn than server-only filtering.
- Requires careful `SessionManager` cache redesign to avoid returning cached sessions before user validation.
- Temporary repository crate complexity: both SQLite and PostgreSQL modules compile, while server runtime uses Postgres.

#### Option B — Server-layer user filtering with minimal repository changes

Approach: Extract trusted headers in routes and filter in `ServerCore`/`SessionManager`, keeping repository traits mostly unchanged.

Pros:
- Smaller initial diff.
- Fewer trait signature changes.

Cons:
- High leak risk because chat methods and caches can bypass filters.
- Violates repository-owned SQL/ownership principle.
- Does not adequately solve PostgreSQL repository replacement.

Invalidation: rejected because fail-closed isolation cannot rely on caller discipline.

#### Option C — Full dual-backend runtime abstraction before user isolation

Approach: Build selectable SQLite/PostgreSQL runtime support for server and desktop, then add user isolation.

Pros:
- Clean long-term backend abstraction.
- Preserves server SQLite runtime for local use.

Cons:
- Contradicts clarified first-pass server/web PostgreSQL-only runtime requirement.
- Expands scope into desktop/server compatibility and delays user isolation.

Invalidation: rejected for this pass. Compile coexistence is required; dual runtime support is not.

## ADR

### Decision

Implement Option A: make `argus-server`/web runtime PostgreSQL-only, introduce a typed trusted-header `RequestUser` seam, and enforce per-user ownership for chat sessions/threads/messages through repository traits and PostgreSQL queries. Preserve SQLite compile-time support for non-server consumers.

### Drivers

- User-facing requirement is per-user chat data isolation.
- PostgreSQL fresh deployment is accepted; SQLite runtime compatibility and migration are non-goals for server/web.
- Existing chat routes, server methods, repository traits, and session caches are global; ownership must be explicit and fail-closed.
- Existing `argus-wing` and `TemplateManager` still depend on `ArgusSqlite`, so deleting SQLite workspace-wide would exceed scope.

### Alternatives Considered

- Server-layer filtering only: rejected due to cache/repository bypass risk.
- Full dual-backend runtime: rejected as too broad for first pass.
- Workspace/team model: rejected by deep-interview non-goals.
- Reusing `accounts`/`AccountManager` as chat user identity: rejected because `/api/v1/account` is single-user/admin auth, not the pre-OAuth multi-user chat identity model.

### Why Chosen

This is the narrowest plan that satisfies both hard requirements: PostgreSQL server/web runtime and per-user isolation for chat data. It keeps SQL in `argus-repository`, preserves non-server compile compatibility, and makes future OAuth2 an extractor replacement rather than a data-model rewrite.

### Consequences

- Trait signature changes will cause broad compile churn; implementation must proceed from shared types and repository traits outward.
- PostgreSQL schema must normalize UUID-backed Rust IDs rather than syntax-porting SQLite integer-ish session definitions.
- `SessionManager` cache/load/subscribe semantics must become user-safe by design.
- SQLite code remains in the workspace for compile compatibility but is not the server/web runtime path.

### Follow-ups

- OAuth2 should later populate the same `RequestUser`/`UserId` context.
- Later phases may decide whether providers/templates/MCP/jobs/runtime require user/workspace scoping.
- Legacy SQLite data migration can be planned separately if it becomes necessary.

## Pre-mortem

1. **Cross-user leak through cache/SSE/trace recovery**: repository checks are correct, but `SessionManager::load` returns cached sessions before user validation or `subscribe` registers a thread across users. Mitigation: require user-aware cache keys or `load_for_user`/`subscribe_for_user` that validates ownership before cache return/reuse, then test list/snapshot/messages/send/cancel/SSE across users.
2. **PostgreSQL schema type mismatch**: migrations accidentally copy SQLite `INTEGER` session IDs while Rust uses UUID-backed `SessionId`/`ThreadId`, causing migration/runtime failures or broken joins. Mitigation: require UUID-compatible Postgres columns and repository round-trip tests for session/thread IDs and FKs.
3. **Backend scope overreach breaks desktop/non-server crates**: removing or renaming SQLite concrete APIs breaks `argus-wing`/`TemplateManager`. Mitigation: keep SQLite compile paths and add `cargo test -p argus-wing` / `cargo test -p argus-template` gates.
4. **Trusted header behavior is insecure by accident**: missing headers map to a default user or browser-sent dev headers are treated as production trust. Mitigation: fail closed by default; production assumes reverse-proxy/header injection; any dev fallback is explicit and tested as non-production.

## Implementation Plan

### Phase 0 — Branch/setup and baseline

Files: repository root, `.worktrees/app-web-multi-user-postgresql`, `Cargo.toml`, `crates/*/Cargo.toml`, `apps/web/package.json`.

1. Work only inside `.worktrees/app-web-multi-user-postgresql`.
2. Run required setup if not already present: `cargo install prek && prek install`.
3. Capture baseline:
   - `cargo test -p argus-repository`
   - `cargo test -p argus-server`
   - `cargo test -p argus-template`
   - `cargo test -p argus-wing`
   - `cd apps/web && pnpm exec vitest run src/lib/api.test.ts src/features/chat/chat-page.test.ts`

### Phase 1 — Typed chat user context and fail-closed header extraction

Files:
- `crates/argus-protocol/src/ids.rs`
- `crates/argus-protocol/src/lib.rs`
- new `crates/argus-server/src/user_context.rs` or equivalent
- `crates/argus-server/src/error.rs`
- `crates/argus-server/src/routes/chat.rs`

Steps:
1. Add a strongly typed `UserId` shared boundary type, preferably UUID-backed if trusted header IDs are expected to be stable UUIDs; otherwise use a validated string newtype and document constraints.
2. Keep `RequestUser` and trusted-header extraction in `argus-server`; do not place HTTP/header semantics in `argus-protocol`.
3. Use headers such as `X-Argus-User-Id` and optional `X-Argus-User-Name`.
4. Missing/malformed headers must fail closed for chat routes. Any dev fallback requires explicit config and must be off by default.
5. Explicitly separate this chat ownership user model from existing `/api/v1/account` and `AccountManager`; the `accounts` table remains the existing single-user/admin credential path and is not reused as chat identity.

Acceptance:
- Every chat route can receive `RequestUser` without adding user IDs to URLs.
- Missing/invalid user headers fail closed.
- `/api/v1/account` behavior is not expanded into multi-user auth.

### Phase 2 — PostgreSQL repository foundation with SQLite compile coexistence

Files:
- `Cargo.toml`
- `crates/argus-repository/Cargo.toml`
- `crates/argus-repository/src/lib.rs`
- new `crates/argus-repository/src/postgres/mod.rs`
- PostgreSQL migration directory or sqlx migrator path
- `crates/argus-server/src/db.rs`
- `crates/argus-server/src/server_core.rs`
- `crates/argus-template/src/manager.rs` only if needed to remove server's concrete SQLite dependency while preserving SQLite support

Steps:
1. Add PostgreSQL `sqlx` features without removing SQLite features required by non-server crates.
2. Add `ArgusPostgres` around `PgPool`, with secret encryption/decryption behavior parallel to SQLite for shared/global secret tables.
3. Add PostgreSQL connect/migrate helpers; `argus-server` accepts only `postgres://` or `postgresql://` runtime URLs and gives a clear error otherwise.
4. Keep `argus-repository::sqlite`, `ArgusSqlite`, and SQLite migrations compiling for non-server consumers.
5. Refactor `ServerCore` assembly so server uses `ArgusPostgres` for repository traits. Avoid broad generic repository injection unless necessary; assembly remains centralized in `ServerCore`.
6. Address `TemplateManager` concrete `ArgusSqlite` coupling by introducing the narrowest trait/helper required for repair/bootstrap, while preserving existing SQLite constructor support for `argus-wing`.

Acceptance:
- `argus-server` runtime path no longer imports/uses `SqlitePool` or `ArgusSqlite`.
- `cargo test -p argus-template` and `cargo test -p argus-wing` still compile/pass or have documented unrelated failures.
- New Postgres migration helpers run on an empty database.

### Phase 3 — PostgreSQL schema with UUID-backed IDs and chat ownership

Files:
- PostgreSQL migrations under `crates/argus-repository/*migrations*`
- `crates/argus-repository/src/types/thread.rs`
- optional `crates/argus-repository/src/types/user.rs`

Steps:
1. Add `users` table for header-derived chat ownership only. It is not the `accounts` auth table.
2. Use UUID-compatible PostgreSQL columns for UUID-backed Rust IDs: `users.id`, `sessions.id`, `threads.id`, `threads.session_id`, `messages.thread_id`, and related FKs/joins. Do not syntax-port SQLite `INTEGER PRIMARY KEY AUTOINCREMENT` for `sessions.id`.
3. Add `sessions.user_id NOT NULL REFERENCES users(id)` and indexes such as `(user_id, updated_at DESC)`.
4. Keep messages linked through threads. Do not duplicate `user_id` in messages unless a measured query need appears.
5. Port shared/global tables to PostgreSQL syntax without adding user ownership to providers/templates/MCP/jobs/agent_runs/runtime.
6. Add repository tests that assert UUID round trips for sessions/threads and that FK joins work with UUID-backed IDs.

Acceptance:
- Empty PostgreSQL schema migrates and stores UUID-backed `SessionId`/`ThreadId` values without type casts or string/integer mismatches.
- Global tables remain global.

### Phase 4 — User-scoped repository traits and Postgres implementations

Files:
- `crates/argus-repository/src/traits/session.rs`
- `crates/argus-repository/src/traits/thread.rs`
- `crates/argus-repository/src/postgres/session.rs`
- `crates/argus-repository/src/postgres/thread.rs`
- related types/tests

Steps:
1. Update chat-facing session trait methods to require `UserId`: list/get/create/rename/delete.
2. Update chat-facing thread/message trait methods to require `UserId` or enforce ownership through `user_id + session_id + thread_id` joins.
3. Split any truly internal raw thread methods into explicitly named raw/trusted methods; chat paths must not call raw methods.
4. Implement Postgres queries with ownership predicates in every critical read/write/delete/update/message operation.
5. Repository tests must prove user B cannot list/get/rename/delete/read/write user A chat records.

Acceptance:
- Removing a `user_id` predicate from critical queries would fail tests.
- Chat-facing repository methods cannot operate with only `SessionId`/`ThreadId`.

### Phase 5 — User-safe SessionManager and ServerCore propagation

Files:
- `crates/argus-session/src/manager.rs`
- `crates/argus-server/src/server_core.rs`
- optionally `crates/argus-session/src/session.rs`

Steps:
1. Add `UserId`/`RequestUser` parameters to public chat methods in `SessionManager` and `ServerCore`.
2. Replace current cache behavior with a required invariant: either cache keys include `(UserId, SessionId)` / `(UserId, ThreadId)` where applicable, or all cache access goes through `load_for_user` / `subscribe_for_user` that validates ownership before returning a cached session/runtime.
3. Apply the invariant to `load`, runtime registration, trace recovery, `ensure_thread_runtime_with_mcp`, `send_message`, `cancel_thread`, `get_thread_messages`, `get_thread_snapshot`, `activate_thread`, and `subscribe` equivalents.
4. Keep provider/template resolution global in `create_thread`.
5. Trace files may remain under session/thread paths, but trace recovery must occur only after repository ownership validation.

Acceptance:
- A cached session/runtime cannot be returned to another user by `SessionId` or `ThreadId` alone.
- Cross-user access fails before runtime load, trace recovery, or SSE subscription.

### Phase 6 — Chat routes, API tests, and SSE authorization

Files:
- `crates/argus-server/src/routes/chat.rs`
- `crates/argus-server/src/routes/mod.rs`
- `crates/argus-server/src/error.rs`
- `crates/argus-server/tests/chat_api.rs`
- `crates/argus-server/tests/support/mod.rs`

Steps:
1. Inject `RequestUser` into every chat route while keeping route paths stable.
2. Pass `RequestUser`/`UserId` through all chat operations.
3. Add user A/B tests for list/create/rename/delete session, list/create/rename/delete/update/activate thread, snapshot, message list/send, cancel, and SSE subscription.
4. Missing trusted header must fail closed for all chat routes.
5. Prefer 404 for cross-user resource existence hiding unless existing API error conventions force 403.

Acceptance:
- User B cannot observe or mutate user A chat resources via REST or SSE.
- Missing header behavior is covered and documented.

### Phase 7 — apps/web and deployment/header behavior

Files:
- `apps/web/src/lib/api.ts`
- `apps/web/src/lib/api.test.ts`
- `apps/web/src/features/chat/*`
- `apps/web/vite.config.ts` or deployment docs if applicable

Steps:
1. Production assumption: trusted user headers are injected by reverse proxy / company gateway / future OAuth2 adapter before requests reach `argus-server`.
2. Do not add login UI or user switcher.
3. Because native `EventSource` cannot set arbitrary headers, do not rely on browser JS to attach trusted headers for production SSE.
4. For local/dev/tests, define one explicit path: run through a dev proxy that injects headers, or configure server test harness headers directly. If `fetch` header injection is added for tests, document that it does not solve native SSE headers.
5. Add/adjust web tests only for stable API behavior and any explicit dev/test header helper; do not expand product UI.

Acceptance:
- `apps/web` works behind trusted header injection.
- SSE header behavior is documented and testable.
- No frontend user switcher/login UI is introduced.

### Phase 8 — Verification and regression hardening

Files:
- `crates/argus-repository/tests/*`
- `crates/argus-server/tests/*`
- `crates/argus-session` tests if added
- `apps/web/src/**/*.test.ts`
- docs/config as needed

Steps:
1. Run repository Postgres migration and UUID/user isolation tests.
2. Run server chat user A/B tests with PostgreSQL `DATABASE_URL`.
3. Run compile/coexistence checks for non-server SQLite consumers.
4. Run web targeted tests.
5. Run full gates once targeted tests pass.

Acceptance:
- Targeted Postgres/user isolation tests pass.
- SQLite compile coexistence checks pass for non-server consumers.
- Full gates pass or unrelated failures are documented with evidence.

## Expanded Test Plan

### Unit

- `UserId` parse/serialize round trips and rejects malformed values.
- `RequestUser` extractor accepts valid trusted headers and rejects missing/malformed headers.
- Chat error mapping is fail-closed and consistent.
- If a dev/test fetch header helper is added, it applies only to `fetch` and explicitly does not claim to configure native `EventSource` headers.

### Integration

- PostgreSQL migrations run on an empty DB.
- Migration/schema tests prove UUID-compatible `users.id`, `sessions.id`, `threads.id`, `threads.session_id`, `messages.thread_id`, and joins/FKs round trip with Rust IDs.
- Repository user A/B tests cover session list/get/create/rename/delete, thread list/get/update/delete, and messages.
- Server user A/B tests cover all chat REST routes and SSE denial.
- Missing-header tests cover every chat route.
- `cargo test -p argus-template` and `cargo test -p argus-wing` verify SQLite compile coexistence.

### E2E / Smoke

- Start server with PostgreSQL `DATABASE_URL` behind a header-injecting test proxy or equivalent harness.
- Use `apps/web` as user A and user B through the harness and confirm each sees only their own chat sessions/threads/messages.
- Confirm providers/templates/MCP management pages still behave as shared/global resources.

### Observability / Ops

- Log missing/invalid trusted header without leaking message content or secrets.
- Log cross-user denial/not-found attempts with safe identifiers and request correlation if available.
- Log PostgreSQL database target and migration success/failure at startup.
- Document production requirement that trusted headers must be stripped/set by the proxy/gateway, not accepted blindly from the public internet.

## Risks and Mitigations

| Risk | Mitigation |
| --- | --- |
| Cross-user leak through cache or trace recovery | User-aware cache/load invariant; cross-user tests for live and recovered paths. |
| UUID-backed Rust IDs mismatch Postgres schema | Explicit UUID-compatible columns and FK round-trip tests. |
| Server Postgres-only wording causes workspace SQLite deletion | ADR/Phase 2 require SQLite compile coexistence; add `argus-wing`/`argus-template` checks. |
| `accounts` auth becomes confused with chat users | Separate `users` table and `RequestUser` model; `/api/v1/account` remains existing admin/single-user path. |
| SSE lacks JS header support | Production reverse-proxy/header injection; local/dev proxy or server harness; explicit tests for SSE denial. |
| SQL leaks outside repository | Use grep verification for new SQL outside `crates/argus-repository`. |

## Verification Commands

```bash
# Required setup
cargo install prek && prek install

# Rust targeted
cargo test -p argus-repository
cargo test -p argus-server chat_api -- --nocapture
cargo test -p argus-session
cargo test -p argus-template
cargo test -p argus-wing

# Frontend targeted
cd apps/web && pnpm exec vitest run src/lib/api.test.ts src/features/chat/chat-page.test.ts src/features/chat/composables/useChatThreadStream.test.ts

# SQL boundary check
rg -n "SELECT|INSERT|UPDATE|DELETE|sqlx::query" crates --glob '*.rs'

# Full gates
prek
cargo test
cargo deny check
cd apps/web && pnpm exec vitest run
cd apps/web && pnpm build
```

## Available-Agent-Types Roster

Known usable roles for follow-up execution/review: `planner`, `architect`, `critic`, `executor`, `explore`, `debugger`, `build-fixer`, `dependency-expert`, `security-reviewer`, `test-engineer`, `verifier`, `code-reviewer`, `code-simplifier`, `writer`, and default/worker agents.

## Follow-up Staffing Guidance

### `$ralph` path

Use when one persistent owner should drive the refactor sequentially.

- `architect` high reasoning: confirm final Postgres/runtime/coexistence/cache boundaries before coding.
- `executor` medium/high reasoning: implement phases 1-7 in order.
- `test-engineer` medium reasoning: build Postgres/user isolation and SSE tests once interfaces stabilize.
- `security-reviewer` medium reasoning: review trusted-header, fail-closed, and cross-user denial semantics.
- `build-fixer` high reasoning: resolve sqlx features, migration, compile failures.
- `verifier` high reasoning: rerun acceptance and full gates.

Suggested command:

```bash
$ralph .omx/plans/ralplan-app-web-multi-user-postgresql.md
```

### `$team` path

Use for faster execution after interfaces are agreed.

1. Repository/PostgreSQL lane — `executor` or `dependency-expert`, high reasoning. Owns `crates/argus-repository`, migrations, sqlx features, UUID schema.
2. Server/user-context lane — `executor`, medium/high reasoning. Owns `crates/argus-server` extractor/routes/ServerCore.
3. Session isolation lane — `executor`, high reasoning. Owns `crates/argus-session` user-safe cache/load/subscribe/trace guards.
4. Compile coexistence lane — `build-fixer`, high reasoning. Owns `argus-wing`, `argus-template`, SQLite compile preservation.
5. Test lane — `test-engineer`, medium reasoning. Owns user A/B repository/server/web/SSE tests.
6. Security lane — `security-reviewer`, medium reasoning. Owns fail-closed and trusted-header boundary review.

Launch hints:

```bash
$team .omx/plans/ralplan-app-web-multi-user-postgresql.md
# or:
omx team --task-file .omx/plans/ralplan-app-web-multi-user-postgresql.md --agents 6
```

Team verification path:

- Team proves: Postgres migrations run, UUID FKs round trip, user A/B isolation tests pass, missing header fails closed, SSE cross-user denial works, shared resources remain shared, SQLite compile coexistence passes, web chat tests pass with documented header injection behavior.
- After team completion, hand to `$ralph` or `verifier` for full gates and residual security/data-isolation review.

## Changelog

- Rev2 applies Architect/Critic feedback: explicit UUID-backed Postgres schema, server runtime vs workspace compile coexistence, required user-safe `SessionManager` invariant, chat users separate from `accounts`, and stronger SSE/proxy verification.
