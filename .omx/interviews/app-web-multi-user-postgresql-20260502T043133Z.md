# Deep Interview Transcript: app-web multi-user PostgreSQL

- Profile: standard
- Context type: brownfield
- Final ambiguity: ~12%
- Threshold: <=20%
- Context snapshot: `.omx/context/app-web-multi-user-postgresql-20260502T042528Z.md`
- Worktree: `.worktrees/app-web-multi-user-postgresql`

## Initial Request

> 现在 app/web 的版本是单用户的；现在要实现多用户的版本，而且后端的 repository 切换成 postgresql；我们后续会接入 oauth2 的鉴权（公司内部的）这一点你先不用管

## Brownfield Evidence

- `apps/web/src/lib/api.ts` calls `argus-server` REST/SSE APIs under `/api/v1`; no current user scoping appears in chat/session paths.
- `crates/argus-server/src/db.rs` defaults to `DATABASE_URL` or `~/.arguswing/sqlite.db`, and only treats `sqlite:` strings as database URLs.
- `crates/argus-server/src/server_core.rs` wires managers through `SqlitePool` and `ArgusSqlite` trait objects.
- `crates/argus-repository/src/sqlite/mod.rs` exposes SQLite-only `connect`, `connect_path`, `migrate`, and `ArgusSqlite`.
- `crates/argus-repository/migrations/20260325120105_init.sql` documents accounts as single-user and enforces `accounts.id = 1`.
- `crates/argus-auth/src/account.rs` is explicitly single-user local authentication and `logout` is a no-op.

## Rounds

### Round 1 — Intent / outcome boundary

Question: 在暂不接 OAuth2 的前提下，多用户第一版最核心要达成哪一种产品语义？

Answer: `per-user-isolation` — 用户数据隔离。

Impact: Multi-user is about data isolation, not merely user attribution, workspace/team modeling, or PostgreSQL-only groundwork.

### Round 2 — Scope / non-goals

Question: 第一版里哪些资源必须按用户隔离？未选中默认共享/全局或延期。

Answer: `chat-sessions-threads-messages` only.

Impact: First-pass isolation is limited to chat sessions, threads, and messages. Providers/API keys, templates, MCP, jobs/runs, runtime/traces are non-goals unless later revisited.

### Round 3 — Decision boundary / identity source

Question: OAuth2 暂不做时，第一版 current user 从哪里来？

Answer: `trusted-header-user-id` — 可信请求头用户 ID。

Impact: Server should derive current user from trusted request headers such as `X-Argus-User-Id` and optionally `X-Argus-User-Name`. Later OAuth2 adapter should populate the same user-context boundary. Do not build temporary local login or frontend user switcher.

### Round 4 — PostgreSQL strategy

Question: 第一版 PostgreSQL 切换应该怎么处理 SQLite？

Answer: `postgres-only` — PostgreSQL only.

Impact: First version should switch server/web repository backend to PostgreSQL only. SQLite compatibility is not a target for this server/web path.

### Round 5 — Data migration / success boundary

Question: 第一版是否必须迁移既有本地 SQLite 数据？

Answer: `fresh-postgres-only` — 全新部署即可。

Impact: No SQLite migration tool, compatibility reader, or legacy data import is required. Acceptance is based on fresh PostgreSQL initialization, per-user chat data isolation, and API tests.

## Pressure Pass Findings

- Round 2 revisited Round 1 with scope/tradeoff pressure: user isolation was narrowed to only sessions/threads/messages.
- Round 3 probed the hidden dependency behind user isolation before OAuth2: current-user identity comes from trusted request headers.
- Round 5 forced the migration boundary: fresh Postgres deployment is enough; no SQLite data migration.
