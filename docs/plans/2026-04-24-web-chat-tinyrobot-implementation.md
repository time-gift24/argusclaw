# Phase 5C Web Chat TinyRobot Implementation Plan

**Goal:** Add an independent TinyRobot-powered web chat page to `apps/web` on top of the completed Phase 5 chat REST API.

**Architecture:** `argus-server` remains a peer server application entry and is not changed unless a contract bug is discovered. `apps/web` calls chat REST endpoints through `src/lib/api.ts`. The UI uses TinyRobot for chat primitives and OpenTiny for management controls.

## Task 1: Document The New Web Boundary

- Create this implementation plan.
- Create `docs/plans/2026-04-24-web-chat-tinyrobot-design.md`.
- Update `apps/web/DESIGN.md` with the `/chat` page rules.

Verification:

```bash
git diff -- docs/plans/2026-04-24-web-chat-tinyrobot-design.md docs/plans/2026-04-24-web-chat-tinyrobot-implementation.md apps/web/DESIGN.md
```

Commit:

```bash
git add docs/plans/2026-04-24-web-chat-tinyrobot-design.md docs/plans/2026-04-24-web-chat-tinyrobot-implementation.md apps/web/DESIGN.md
git commit -m "docs(web): plan tinyrobot chat console"
```

## Task 2: Add Failing Web Chat Tests

- Create `apps/web/src/features/chat/chat-page.test.ts`.
- Cover loading sessions/templates/providers, creating a session/thread, sending a message, cancelling, refreshing, and visible error/empty states.
- Update nav/smoke expectations for the new "对话" entry.

Verification:

```bash
cd apps/web && pnpm exec vitest run apps/web/src/features/chat/chat-page.test.ts
```

Expected: fail before the route/component/API client exists.

## Task 3: Add TinyRobot Dependencies And App Wiring

- Add `@opentiny/tiny-robot`, `@opentiny/tiny-robot-kit`, and `@opentiny/tiny-robot-svgs`.
- Import `@opentiny/tiny-robot/dist/style.css` in `src/main.ts`.
- Add `/chat` route and nav item.
- Add Vitest stubs/aliases only if the real TinyRobot package is not test-friendly in jsdom.

Verification:

```bash
cd apps/web && pnpm install
cd apps/web && pnpm exec vitest run
```

## Task 4: Add Chat API Client

- Add TypeScript types for chat sessions, threads, messages, thread snapshots, bindings, and action responses.
- Add `ApiClient` methods and `HttpApiClient` implementations for the Phase 5 chat endpoints.
- Keep response envelope handling consistent with existing mutation APIs.

Verification:

```bash
cd apps/web && pnpm exec vitest run apps/web/src/features/chat/chat-page.test.ts
```

## Task 5: Implement The Chat Page

- Create `apps/web/src/features/chat/ChatPage.vue`.
- Use `TrBubbleList`, `TrSender`, and `TrPrompts`.
- Shape the page after opencode desktop / Codex chat behavior: left context rail, primary message timeline, bottom composer, and pending assistant feedback while the backend is generating.
- Use OpenTiny controls for session/thread operations, selectors, refresh, delete, and status actions.
- Keep all labels and feedback in Chinese.
- Avoid SSE assumptions; refresh affected state after mutations and use short post-send polling to approximate streaming until Phase 5 adds a chat event stream.

Verification:

```bash
cd apps/web && pnpm exec vitest run
cd apps/web && pnpm build
```

Commit:

```bash
git add apps/web/package.json apps/web/pnpm-lock.yaml apps/web/src/main.ts apps/web/src/app/nav.ts apps/web/src/router/index.ts apps/web/src/lib/api.ts apps/web/src/features/chat apps/web/src/app/admin-console.smoke.test.ts apps/web/src/layouts/admin-layout.test.ts apps/web/src/styles/tokens.css
git commit -m "feat(web): add tinyrobot chat console"
```

## Task 6: Full Phase 5 Regression

Run fresh verification:

```bash
cargo test -p argus-server -- --nocapture
cd apps/web && pnpm exec vitest run
cd apps/web && pnpm build
cargo tree -p argus-server | rg argus-wing
```

Expected:

- Rust server tests pass.
- Web tests pass.
- Web build passes.
- `cargo tree -p argus-server | rg argus-wing` returns no matches.
