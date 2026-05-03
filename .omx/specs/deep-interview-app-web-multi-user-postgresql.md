# Deep Interview Spec: app-web multi-user PostgreSQL

## Metadata

- Profile: standard
- Context type: brownfield
- Final ambiguity: ~12%
- Threshold: <=20%
- Rounds: 5
- Context snapshot: `.omx/context/app-web-multi-user-postgresql-20260502T042528Z.md`
- Transcript: `.omx/interviews/app-web-multi-user-postgresql-20260502T043133Z.md`
- Initial context summary status: not needed

## Clarity Breakdown

| Dimension | Score | Notes |
| --- | ---: | --- |
| Intent | 0.90 | Multi-user goal is per-user chat data isolation. |
| Outcome | 0.90 | Fresh PostgreSQL-backed server/web where user A cannot see or mutate user B chat sessions/threads/messages. |
| Scope | 0.90 | Scope is intentionally narrow: chat sessions, threads, and messages only. |
| Constraints | 0.85 | PostgreSQL only for server/web; trusted request headers for pre-OAuth user context; no OAuth2 implementation. |
| Success Criteria | 0.85 | Acceptance can be tested through fresh DB init and cross-user REST/API isolation tests. |
| Brownfield Context | 0.85 | SQLite-only and single-user boundaries identified in repository/server/auth/web client. |

## Intent

Convert `apps/web` + `argus-server` from a single-user web console/chat surface into a first-pass multi-user server/web deployment where chat data is isolated per user, while moving the backend repository to PostgreSQL.

## Desired Outcome

A fresh PostgreSQL deployment of server/web supports multiple users identified by trusted request headers. For the first version, each user can only list, create, read, update, delete, stream, and send messages for their own chat sessions/threads/messages.

## In Scope

1. PostgreSQL-backed repository/runtime path for server/web.
2. New or adjusted schema to represent users and attach ownership to chat sessions, threads, and messages as needed.
3. Server request user-context extraction from trusted headers, e.g. `X-Argus-User-Id` and optionally `X-Argus-User-Name`.
4. Thread/session/message repository and service API changes needed to enforce per-user isolation.
5. REST/SSE route behavior for chat sessions/threads/messages must be scoped by current user.
6. Fresh PostgreSQL initialization and migrations.
7. Tests proving cross-user isolation for chat sessions, threads, messages, and send/stream/cancel paths where applicable.

## Out of Scope / Non-goals

1. OAuth2 / company SSO integration.
2. Temporary local login, session cookies, password auth, or frontend user switcher.
3. SQLite compatibility for the server/web first pass.
4. Migration/import of existing local SQLite data.
5. User-level isolation of LLM providers, API keys, agent templates, MCP servers/tools, agent runs/jobs, runtime snapshots, or traces.
6. Workspace/team/organization model.
7. Desktop rewiring unless unavoidable for compilation boundaries.

## Decision Boundaries

OMX may decide without further confirmation:

- Exact request header names, as long as they are documented and map cleanly to a future OAuth2-derived user context.
- Internal user ID type and record shape, as long as it supports stable per-user chat ownership.
- Whether to introduce a `RequestUser` / `UserContext` type in `argus-server` and pass it through managers/repositories.
- How to refactor repository constructors/names away from `ArgusSqlite` when switching server/web to PostgreSQL.
- Test harness details for PostgreSQL, including Docker/testcontainers or environment-driven `DATABASE_URL`, provided CI/local failure modes are clear.

OMX should ask before:

- Expanding isolation beyond sessions/threads/messages.
- Reintroducing SQLite dual-backend support.
- Adding login/OAuth/session-cookie UX.
- Adding workspace/team concepts.
- Implementing legacy SQLite data migration.

## Constraints

- SQL must remain inside `crates/argus-repository`.
- `apps/web` must continue using `argus-server` REST/SSE APIs and must not reuse desktop store.
- `argus-server` route handlers should stay narrow; orchestration belongs in `ServerCore` / managers and repository traits.
- Existing architecture uses trait/`Arc<dyn ...>` injection for persistence; avoid leaking concrete PostgreSQL implementation upward.
- Fresh Postgres deployment is the acceptance baseline.

## Testable Acceptance Criteria

1. Server starts against a PostgreSQL `DATABASE_URL` and runs PostgreSQL migrations from an empty database.
2. Creating/listing chat sessions under user A does not expose sessions created under user B.
3. Thread creation, activation, rename/delete, model binding, and snapshots are scoped so user B cannot access user A thread IDs.
4. Message listing/sending for a thread rejects or hides cross-user access.
5. SSE thread events do not allow subscribing to another user's thread.
6. `apps/web` chat/session API calls continue to work when the trusted user headers are supplied by deployment/test harness.
7. Providers/templates/MCP/jobs/runtime remain shared/global or otherwise unchanged by this pass, except for compile-time adjustments required by the repository switch.
8. Tests document that OAuth2 is deferred and that missing/invalid user headers fail closed or map to an explicitly documented development/default behavior.

## Assumptions Exposed + Resolutions

- Assumption: “Multi-user” might mean all resources become per-user. Resolution: first version isolates only chat sessions/threads/messages.
- Assumption: OAuth2 deferral leaves no current-user source. Resolution: use trusted request headers as the pre-OAuth boundary.
- Assumption: PostgreSQL switch might need SQLite compatibility. Resolution: server/web first pass is PostgreSQL only.
- Assumption: Existing SQLite data must be preserved. Resolution: no migration; fresh PostgreSQL deployment is enough.

## Brownfield Evidence vs Inference

Evidence:

- Current account/auth implementation is single-user (`accounts.id = 1`, single-user auth manager, no-op logout).
- Current server repository wiring is SQLite-specific (`SqlitePool`, `ArgusSqlite`, SQLite migrations).
- Web client talks only to server REST/SSE APIs and currently has no current-user API path segment.

Inference:

- The clean boundary for future OAuth2 is a server-side request user context populated today by trusted headers and later by OAuth2 middleware/adapter.
- The highest-risk code path is not `apps/web` UI but repository/server/session/thread ownership propagation.

## Recommended Handoff

Use `$ralplan` next:

```bash
$plan --consensus --direct .omx/specs/deep-interview-app-web-multi-user-postgresql.md
```

Reason: repository backend replacement plus per-user isolation crosses `argus-repository`, `argus-server`, and session/thread managers, so a consensus architecture/test plan should precede implementation.
