# RALPLAN-DR Draft: app/web Multi-user PostgreSQL

## Source Requirements

- Input spec: `.omx/specs/deep-interview-app-web-multi-user-postgresql.md`
- Mode: consensus, direct, deliberate (auth boundary + data isolation + database backend replacement)
- Stop rule: plan only; no implementation in planning mode.

## Requirements Summary

Implement a first-pass multi-user `apps/web` + `argus-server` deployment backed by PostgreSQL. The only user-isolated product data in this pass is chat sessions, threads, and messages. Current user identity is supplied by trusted request headers now, with the same server-side user context intended to be populated by OAuth2 later. OAuth2, temporary login, frontend user switcher, workspace/team model, SQLite compatibility, and legacy SQLite data migration are out of scope.

## Evidence Snapshot

- `crates/argus-server/src/db.rs:10-20` treats only `sqlite:` as a URL and otherwise expands to a filesystem path; `crates/argus-server/src/db.rs:43-45` defaults `DATABASE_URL` to `~/.arguswing/sqlite.db`.
- `crates/argus-server/src/server_core.rs:20-27` imports `ArgusSqlite`, SQLite connect/migrate helpers, and `SqlitePool`.
- `crates/argus-server/src/server_core.rs:136-154` initializes from a SQLite pool; `crates/argus-server/src/server_core.rs:154-214` wires every repository trait from `ArgusSqlite`.
- `crates/argus-repository/src/lib.rs:1-7` describes repository traits plus SQLite implementations; `crates/argus-repository/src/lib.rs:19-20` re-exports only SQLite concrete helpers.
- `crates/argus-repository/src/sqlite/mod.rs:30-73` provides SQLite-only connect and migrate; `crates/argus-repository/src/sqlite/mod.rs:75-83` defines `ArgusSqlite` around `SqlitePool`.
- `Cargo.toml:43` enables `sqlx` with SQLite features; `crates/argus-repository/Cargo.toml:12` uses workspace `sqlx` features.
- `crates/argus-repository/migrations/20260325120105_init.sql:132-140` defines single-user accounts with `CHECK (id = 1)`.
- `crates/argus-repository/migrations/20260325120105_init.sql:143-150` defines sessions without owner, and `:67-83` defines threads scoped only by `session_id`.
- `crates/argus-repository/src/traits/session.rs:18-32` has session repository methods without user context.
- `crates/argus-repository/src/traits/thread.rs:11-79` has thread/message methods without user context except session/thread IDs.
- `crates/argus-session/src/manager.rs:1121-1147` lists all sessions globally; `:1209-1218` creates sessions without owner; `:1541-1568` lists threads by session only; `:1585-1668` sends/reads messages by session/thread only.
- `crates/argus-server/src/routes/mod.rs:33-73` exposes chat routes without user path segment; `crates/argus-server/src/routes/chat.rs:526-721` passes only path/body fields to `ServerCore`.
- `apps/web/src/lib/api.ts:674-833` calls chat REST/SSE endpoints without user context; `apps/web/src/lib/api.ts:859-865` centralizes fetch in one request helper.
- `crates/argus-server/tests/support/mod.rs:18-30` and `crates/argus-server/tests/chat_api.rs` currently use in-memory SQLite server tests.

## RALPLAN-DR Summary

### Principles

1. **Fail-closed tenant boundary:** missing or invalid current-user context must not silently expose global chat data.
2. **Repository owns SQL and isolation predicates:** all SQL and ownership predicates stay in `argus-repository`; upper layers pass typed context, not SQL snippets.
3. **Smallest product scope:** only sessions/threads/messages are isolated; shared providers/templates/MCP/jobs/runtime remain unchanged except compile-time plumbing.
4. **Future OAuth2 seam, no OAuth2 implementation:** request-header identity should enter through a replaceable server extractor/middleware boundary.
5. **PostgreSQL-first fresh deployment:** no SQLite runtime compatibility or data migration is required for this server/web pass.

### Decision Drivers

1. Prevent cross-user read/write/SSE access to chat data.
2. Replace SQLite-specific server/repository wiring with PostgreSQL-backed implementation cleanly enough that concrete DB types do not leak upward.
3. Keep the first pass narrow and testable despite touching repository, server, session manager, and web client/test harness.

### Viable Options

#### Option A — PostgreSQL concrete replacement + user-scoped repository traits (favored)

Approach: Add `UserId`/`RequestUser`, PostgreSQL repository implementation, PostgreSQL migrations, and update session/thread traits and managers to take user context for chat operations.

Pros:
- Directly satisfies PostgreSQL-only and per-user isolation requirements.
- Strong fail-closed ownership checks can be placed in repository queries.
- Leaves provider/template/MCP/job/runtime shared by not threading user context there.

Cons:
- Large compile surface because `ServerCore`, tests, and repository constructors are SQLite-shaped today.
- Requires careful in-memory runtime/session cache keying so loaded sessions do not cross users.

#### Option B — User scoping only in server handlers, minimal repository changes

Approach: Extract user headers in routes and filter at service level while keeping existing repository methods mostly unchanged.

Pros:
- Appears smaller initially.
- Fewer trait signature changes.

Cons:
- Violates repository boundary and risks missed ownership predicates across read/write/SSE paths.
- Does not solve PostgreSQL-only backend replacement.
- Higher cross-user leak risk due to service-level filtering gaps.

Invalidation: rejected because it conflicts with fail-closed tenant boundary and SQL-only-in-repository constraints.

#### Option C — Dual backend abstraction before user isolation

Approach: First abstract SQLite/PostgreSQL backend selection, then add user isolation.

Pros:
- Preserves local SQLite tests and desktop possibilities.
- Lower operational disruption for existing dev workflows.

Cons:
- Contradicts clarified PostgreSQL-only and no SQLite compatibility requirements.
- Expands scope and increases branch lifetime.

Invalidation: rejected for first pass because SQLite compatibility was explicitly made a non-goal.

## ADR

### Decision

Implement Option A: PostgreSQL-only server/web repository path with typed request user context and user-scoped chat session/thread/message repository methods.

### Drivers

- The product requirement is per-user chat data isolation, not general attribution.
- PostgreSQL-only/fresh deployment is accepted, so dual-backend abstraction is unnecessary for this pass.
- Existing server/session APIs are global; ownership must become explicit in signatures and tests.

### Alternatives Considered

- Server-layer filtering with minimal repository change: rejected because it makes ownership enforcement easy to bypass.
- Dual backend SQLite+Postgres: rejected because it contradicts scope and adds unnecessary compatibility burden.
- Full workspace/team model: rejected because it exceeds first-pass requirements.

### Why Chosen

It is the narrowest approach that satisfies both hard requirements: PostgreSQL backend and per-user isolation for chat data. It keeps SQL in the repository crate and creates a clean future OAuth2 seam at request user extraction.

### Consequences

- Many compile errors are expected after trait signature changes; implementation should proceed from types/traits outward.
- Server and repository tests must move from SQLite-only harnesses to PostgreSQL-capable harnesses.
- Desktop/local SQLite paths may break if they depend on `argus-repository` concrete exports; execution must either update compile-time consumers or gate server-specific behavior carefully without making dual backend a product goal.

### Follow-ups

- Later OAuth2 integration should replace header extraction while preserving `RequestUser`/`UserContext`.
- Later phases may decide whether providers/templates/MCP/jobs/runtime need user or workspace scoping.
- A later migration story may be planned if existing SQLite data becomes valuable.

## Pre-mortem

1. **Cross-user leak through cache/SSE:** repository checks are correct, but `SessionManager.sessions` or `thread_sessions` caches by `SessionId`/`ThreadId` only and allows a loaded runtime subscription across users. Mitigation: include user context in access checks before load/register/subscribe; add cross-user tests for snapshot, messages, activation, send, cancel, and SSE.
2. **PostgreSQL conversion is incomplete:** some crates/tests still instantiate `SqlitePool`/`ArgusSqlite`, causing compile failures or SQLite-only test paths. Mitigation: inventory all `ArgusSqlite`, `SqlitePool`, `sqlite::memory:` refs and convert server/repository harnesses before broad refactor.
3. **Trusted header behavior is insecure by accident:** missing header maps to default user in production and hides leaks in tests. Mitigation: fail closed by default; allow any development fallback only behind explicit config and test both missing-header rejection and valid-header success.

## Implementation Plan

### Phase 0 — Branch/setup and build baseline

Files: repository root, `.worktrees/app-web-multi-user-postgresql`, `Cargo.toml`, `crates/*/Cargo.toml`, `apps/web/package.json`.

1. Work only inside `.worktrees/app-web-multi-user-postgresql`.
2. Run required setup if not already present: `cargo install prek && prek install`.
3. Capture baseline commands and expected current failures before source changes:
   - `cargo test -p argus-repository --no-default-features` if feature refactor is introduced; otherwise `cargo test -p argus-repository`.
   - `cargo test -p argus-server`.
   - `cd apps/web && pnpm exec vitest run src/lib/api.test.ts src/features/chat/chat-page.test.ts`.

### Phase 1 — Define typed user context and fail-closed extraction

Files:
- `crates/argus-protocol/src/ids.rs`
- `crates/argus-protocol/src/lib.rs`
- `crates/argus-server/src/routes/*` or new `crates/argus-server/src/user_context.rs`
- `crates/argus-server/src/error.rs`

Steps:
1. Add a strongly typed `UserId` (prefer UUID/string newtype based on header stability) and optionally `RequestUser { id, display_name }` in the lowest crate that does not create business orchestration. If `argus-protocol` is used, keep it as shared boundary type only.
2. Add server-side extractor/middleware for trusted headers, with documented names such as `X-Argus-User-Id` and `X-Argus-User-Name`.
3. Make missing/invalid user header return 401/400 for chat routes by default. If a dev fallback exists, guard it with explicit env/config and do not enable it by default.
4. Unit-test extractor behavior for valid, missing, malformed, and optional display-name headers.

Acceptance:
- Chat handlers can receive `RequestUser` without adding path/query user IDs.
- Missing/invalid user context fails closed.
- Non-chat routes remain unchanged unless compile-time plumbing requires a shared extractor utility.

### Phase 2 — PostgreSQL repository foundation

Files:
- `Cargo.toml`
- `crates/argus-repository/Cargo.toml`
- `crates/argus-repository/src/lib.rs`
- new `crates/argus-repository/src/postgres/mod.rs`
- new `crates/argus-repository/postgres_migrations/*` or equivalent sqlx migrator path
- `crates/argus-server/src/db.rs`
- `crates/argus-server/src/server_core.rs`

Steps:
1. Update workspace `sqlx` features from SQLite-only to PostgreSQL runtime/migrate/macros as needed.
2. Add `ArgusPostgres` with `PgPool`, key/cipher handling parallel to `ArgusSqlite` where secret fields remain shared/global.
3. Add PostgreSQL `connect`/`migrate` helpers with explicit `postgres://` / `postgresql://` handling.
4. Update `ServerCore::init` to require PostgreSQL URL for server/web; remove filesystem fallback for this path or make non-Postgres `DATABASE_URL` fail with clear error.
5. Replace `ServerCore::with_pool(SqlitePool)` with a PostgreSQL-aware test constructor, or a generic constructor that accepts already-built `Arc<dyn ...>` repositories if that reduces concrete DB leakage.
6. Keep SQL only in `argus-repository`; route/server code must not build SQL.

Acceptance:
- Empty PostgreSQL DB migrates successfully.
- `argus-server` no longer imports `SqlitePool`/`ArgusSqlite` on the server runtime path.
- Existing shared/global repository traits still use trait objects above `argus-repository`.

### Phase 3 — PostgreSQL schema for users + chat ownership

Files:
- PostgreSQL migration files under `crates/argus-repository/*migrations*`
- `crates/argus-repository/src/types/thread.rs`
- potentially new `crates/argus-repository/src/types/user.rs`

Steps:
1. Add `users` table keyed by typed `UserId`, with optional display name and timestamps. Since OAuth2 is deferred, allow upsert-on-seen from trusted headers.
2. Add `user_id` ownership to `sessions`. Prefer `sessions.user_id NOT NULL REFERENCES users(id)` because threads/messages inherit ownership through session.
3. Keep `threads.session_id` required in the Postgres schema for chat threads; ensure indexes support `(user_id, updated_at)` on sessions and `(session_id, updated_at)` on threads.
4. Keep messages tied to threads; do not duplicate `user_id` in messages unless tests/performance justify it.
5. Port existing shared tables to PostgreSQL syntax: providers, agents, mcp, jobs, agent_runs. Do not add user ownership to providers/templates/MCP/jobs in this pass.
6. Avoid SQLite migration compatibility and old-data import paths.

Acceptance:
- Fresh schema contains users and session ownership.
- Global tables remain global.
- Foreign keys and indexes support ownership queries and deletion cascades.

### Phase 4 — User-scoped repository traits and PostgreSQL implementations

Files:
- `crates/argus-repository/src/traits/session.rs`
- `crates/argus-repository/src/traits/thread.rs`
- `crates/argus-repository/src/postgres/session.rs`
- `crates/argus-repository/src/postgres/thread.rs`
- related `types/*`

Steps:
1. Update session trait methods to require user context for list/get/create/rename/delete, e.g. `list_with_counts(user_id)`, `get(user_id, session_id)`, `create(user_id, session_id, name)`.
2. Update thread trait methods that are reachable from chat APIs to require user context or enforce ownership through `session_id + user_id` joins.
3. Ensure `delete_thread`, `get_thread`, and message reads/writes cannot succeed by thread ID alone for chat paths. If global/job internals still need raw thread access, split raw methods with names that make trust boundary explicit.
4. Implement PostgreSQL versions with ownership predicates in every query.
5. Add repository integration tests that create user A and user B, then prove cross-user list/get/rename/delete/message operations return not found or empty.

Acceptance:
- There is no chat-facing repository method that can read/write sessions/threads/messages without user ownership context.
- Repository tests fail if a `WHERE user_id = ...` predicate is removed from critical queries.

### Phase 5 — Propagate user context through SessionManager and ServerCore

Files:
- `crates/argus-session/src/manager.rs`
- `crates/argus-server/src/server_core.rs`
- potentially `crates/argus-session/src/session.rs`

Steps:
1. Add `RequestUser`/`UserId` parameters to `SessionManager` public chat methods: list/create/delete/rename session, create/list/delete/rename/update/activate thread, get snapshot/messages, send/cancel/subscribe.
2. Key loaded session/runtime bookkeeping safely. At minimum, validate ownership before every load/register/subscribe; consider keying in-memory sessions by `(UserId, SessionId)` if `SessionId` alone is insufficient for future safety.
3. Update `ServerCore` chat methods to take `RequestUser` and forward only `user.id` where display name is not needed.
4. Keep provider/template resolution global in `create_thread`; do not add user ownership to providers/templates.
5. Ensure trace path handling cannot cross users accidentally. If traces stay on disk under session/thread IDs, access must still be guarded by repository ownership before trace recovery.

Acceptance:
- Cross-user `SessionId`/`ThreadId` calls fail before runtime load or trace recovery.
- No public server chat method operates on session/thread/message without user context.

### Phase 6 — Wire routes and SSE to trusted user context

Files:
- `crates/argus-server/src/routes/chat.rs`
- `crates/argus-server/src/routes/mod.rs`
- `crates/argus-server/src/error.rs`
- `crates/argus-server/tests/chat_api.rs`
- `crates/argus-server/tests/support/mod.rs`

Steps:
1. Inject `RequestUser` into every chat route in `routes/chat.rs`.
2. Keep route paths stable; do not add user IDs to URLs.
3. Update list/create/rename/delete/snapshot/message/send/cancel/activate/SSE calls to pass user context.
4. Add tests using headers for user A and user B:
   - user A creates session/thread; user B list does not include it.
   - user B cannot fetch/rename/delete user A session/thread.
   - user B cannot list/send messages to user A thread.
   - user B cannot open SSE for user A thread.
   - missing header fails closed for chat routes.
5. Ensure non-chat server APIs still pass existing tests or document intentional Postgres harness changes.

Acceptance:
- HTTP behavior matches deep-interview acceptance criteria.
- Cross-user attempts return 404/403 consistently; prefer 404 for resource existence hiding unless error conventions require otherwise.

### Phase 7 — apps/web client and test harness adjustments

Files:
- `apps/web/src/lib/api.ts`
- `apps/web/src/lib/api.test.ts`
- `apps/web/src/features/chat/*`
- `apps/web/vite.config.ts` if dev proxy injection is selected
- docs/config file if present

Steps:
1. Decide the browser deployment header source without adding a visible user switcher. Preferred: deployment/reverse proxy injects trusted headers; `apps/web` need not send user headers in production.
2. If local dev/tests require client-sent headers, add a narrow API client header hook/config that is not a product UI and can be disabled in production.
3. Update `fetch` and `EventSource` handling if the client is responsible for headers. Note: native `EventSource` cannot set arbitrary headers, so prefer proxy/header injection for SSE. If client-sent headers are unavoidable, replace SSE construction with a supported mechanism only after explicit risk review.
4. Update web tests to assert API paths remain stable and optional configured headers are applied only where technically possible.

Acceptance:
- `apps/web` chat still works when served behind a trusted header-injecting proxy/test harness.
- No frontend user switcher/login UI is introduced.
- SSE design is documented so trusted headers are available to server.

### Phase 8 — Verification and regression hardening

Files:
- `crates/argus-repository/tests/*`
- `crates/argus-server/tests/*`
- `apps/web/src/**/*.test.ts`
- build config / CI docs as needed

Steps:
1. Run targeted repository PostgreSQL integration tests.
2. Run server chat API cross-user isolation tests with PostgreSQL database URL.
3. Run web API/chat tests.
4. Run full workspace checks once targeted tests pass:
   - `prek`
   - `cargo test`
   - `cargo deny check`
   - `cd apps/web && pnpm exec vitest run`
   - `cd apps/web && pnpm build`
5. Document required test database setup and environment variables.

Acceptance:
- Targeted cross-user tests pass.
- Workspace checks pass or any remaining failures are unrelated and documented with evidence.

## Expanded Test Plan

### Unit

- Header extractor parses valid trusted headers and rejects missing/malformed values.
- `UserId` parsing/serialization round trips.
- Error mapping returns consistent fail-closed responses for chat routes.
- API client header helper, if added, applies configured headers to `fetch` and documents SSE limitations.

### Integration

- PostgreSQL migrations run on an empty database.
- Repository user A/B isolation tests for sessions, thread lookup/update/delete, and messages.
- Server chat API user A/B tests for list/create/rename/delete/snapshot/activate/model/messages/send/cancel.
- SSE subscription rejects user B for user A thread.
- Missing trusted header rejected for every chat route.

### E2E / Smoke

- Start server with PostgreSQL `DATABASE_URL` and header-injecting proxy/test harness.
- Use `apps/web` to create two users' sessions via harness headers and confirm UI shows only the active user's sessions.
- Confirm shared provider/template pages still work as global resources.

### Observability

- Log missing/invalid user header at debug/warn without leaking secrets.
- Log cross-user not-found/denied attempts with request correlation if available, not with full message contents.
- Add migration startup logs showing PostgreSQL target and migration success/failure.

## Risks and Mitigations

| Risk | Mitigation |
| --- | --- |
| Trait signature churn creates broad compile failures | Change protocol/types first, repository traits second, then managers/routes; keep phases small and run targeted `cargo check -p` after each. |
| Cross-user leakage through thread runtime cache or traces | Validate ownership before `load`, `ensure_thread_runtime_with_mcp`, trace recovery, and `subscribe`; add user A/B tests for all live and recovered paths. |
| EventSource cannot send custom headers | Prefer reverse-proxy/header injection; do not build frontend user switcher. If client headers are unavoidable, pause for a design decision before replacing SSE transport. |
| Shared global resources accidentally become user-scoped | Add tests that providers/templates/MCP routes are unchanged; keep user context out of those traits. |
| PostgreSQL test setup is flaky | Use a clearly documented `DATABASE_URL` contract; if testcontainers are added, keep fallback/skip behavior explicit. |
| SQL leaks outside repository | Review with `rg -n "SELECT|INSERT|UPDATE|DELETE|sqlx::query" crates --glob '*.rs'` and ensure new SQL only exists in `crates/argus-repository`. |

## Verification Commands

```bash
# Rust targeted
cargo test -p argus-repository
cargo test -p argus-server chat_api -- --nocapture
cargo test -p argus-session

# Frontend targeted
cd apps/web && pnpm exec vitest run src/lib/api.test.ts src/features/chat/chat-page.test.ts src/features/chat/composables/useChatThreadStream.test.ts

# Full gates
prek
cargo test
cargo deny check
cd apps/web && pnpm exec vitest run
cd apps/web && pnpm build
```

## Available-Agent-Types Roster

Known usable roles for follow-up execution/review include: `planner`, `architect`, `critic`, `executor`, `explore`, `debugger`, `build-fixer`, `dependency-expert`, `security-reviewer`, `test-engineer`, `verifier`, `code-reviewer`, `code-simplifier`, `writer`, and default/worker-style agents.

## Follow-up Staffing Guidance

### `$ralph` path

Recommended when one persistent owner should drive the refactor sequentially until verified.

- `architect` high reasoning: confirm final ownership propagation and PostgreSQL constructor boundaries before coding.
- `executor` medium reasoning: implement phases 1-6 in order.
- `test-engineer` medium reasoning: build PostgreSQL/user isolation tests in parallel only after trait shapes stabilize.
- `security-reviewer` medium reasoning: review fail-closed current-user extraction and cross-user denial semantics.
- `build-fixer` high reasoning: resolve sqlx feature, migration, and compile failures after major edits.
- `verifier` high reasoning: verify acceptance criteria and command evidence before completion.

Suggested command:

```bash
$ralph .omx/plans/ralplan-app-web-multi-user-postgresql.md
```

### `$team` path

Recommended for faster execution because repository backend, server user context, session ownership, tests, and web harness are separable after interfaces are agreed.

Suggested staffing:

1. Repository/PostgreSQL lane — `executor` or `dependency-expert`, high reasoning. Owns `crates/argus-repository`, migrations, sqlx features.
2. Server/user-context lane — `executor`, medium/high reasoning. Owns `crates/argus-server` extractors/routes/ServerCore.
3. Session isolation lane — `executor`, high reasoning. Owns `crates/argus-session` ownership propagation and runtime/trace guards.
4. Test lane — `test-engineer`, medium reasoning. Owns cross-user repository/server/web tests and PostgreSQL harness.
5. Security verification lane — `security-reviewer`, medium reasoning. Owns fail-closed checks, header trust boundary, cross-user denial review.
6. Build integration lane — `build-fixer`, high reasoning. Owns cargo/sqlx feature integration and final compile/test failures.

Launch hints:

```bash
$team .omx/plans/ralplan-app-web-multi-user-postgresql.md
# or, if using OMX CLI directly:
omx team --task-file .omx/plans/ralplan-app-web-multi-user-postgresql.md --agents 6
```

Team verification path:

- Before shutdown, team must prove: PostgreSQL migrations run, cross-user isolation tests pass, missing header fails closed, shared resources remain shared, and web chat tests pass with documented header injection/proxy behavior.
- After team completion, hand to `$ralph` or `verifier` to rerun full gates and inspect any residual security/data-isolation risk.

## Draft Changelog

- Initial deliberate draft created from deep-interview spec and brownfield evidence.
