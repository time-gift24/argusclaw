# Desktop Server + Web Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Split the current desktop-only product into a shared Rust `argus-server`, a reusable frontend core, and a new web client while keeping desktop working and removing login from the shared user flow.

**Architecture:** Keep `ArgusWing` and the existing Rust workspace as the only business core. Add `crates/argus-server` as an `axum` transport layer with `REST + SSE`, extract frontend state and feature UI into `packages/app-core`, keep desktop on a `TauriTransport`, and add `apps/web` on an `HttpSseTransport`.

**Tech Stack:** Rust, `axum`, `tokio`, `serde`, React 19, Vite 8, TypeScript, Zustand, Tauri 2, `tsx --test`

---

### Task 1: Create the frontend workspace and shared transport contract

**Files:**
- Create: `package.json`
- Create: `pnpm-workspace.yaml`
- Create: `tsconfig.base.json`
- Create: `packages/app-core/package.json`
- Create: `packages/app-core/tsconfig.json`
- Create: `packages/app-core/src/index.ts`
- Create: `packages/app-core/src/transport/app-transport.ts`
- Create: `packages/app-core/src/transport/app-transport.test.ts`
- Modify: `crates/desktop/package.json`
- Modify: `crates/desktop/tsconfig.json`
- Modify: `crates/desktop/vite.config.ts`

**Step 1: Write the failing test**

```ts
import test from "node:test";
import assert from "node:assert/strict";

import { createTransportContext } from "./app-transport";

test("transport context exposes typed subscribe hooks", () => {
  const context = createTransportContext({
    subscribeThreadEvents: async () => () => {},
    subscribeMonitorEvents: async () => () => {},
  });

  assert.equal(typeof context.subscribeThreadEvents, "function");
  assert.equal(typeof context.subscribeMonitorEvents, "function");
});
```

**Step 2: Run test to verify it fails**

Run: `pnpm --filter @argus/app-core exec tsx --test src/transport/app-transport.test.ts`
Expected: FAIL because `@argus/app-core` and `createTransportContext` do not exist yet.

**Step 3: Write minimal implementation**

```ts
export interface AppTransport {
  subscribeThreadEvents(): Promise<() => void>;
  subscribeMonitorEvents(): Promise<() => void>;
}

export function createTransportContext(transport: AppTransport) {
  return transport;
}
```

Also create the root frontend workspace files, rename `crates/desktop/package.json` to a workspace-safe package name such as `@argus/desktop`, and make `crates/desktop` resolve imports through the new root TypeScript base config.

**Step 4: Run test to verify it passes**

Run: `pnpm install`
Run: `pnpm --filter @argus/app-core exec tsx --test src/transport/app-transport.test.ts`
Expected: PASS

**Step 5: Commit**

```bash
git add package.json pnpm-workspace.yaml tsconfig.base.json packages/app-core crates/desktop/package.json crates/desktop/tsconfig.json crates/desktop/vite.config.ts
git commit -m "build: add frontend workspace and app transport contract"
```

### Task 2: Extract chat state and runtime into `packages/app-core`

**Files:**
- Create: `packages/app-core/src/features/chat/chat-store.ts`
- Create: `packages/app-core/src/features/chat/chat-runtime.ts`
- Create: `packages/app-core/src/features/chat/index.ts`
- Create: `packages/app-core/src/features/chat/chat-store.test.ts`
- Modify: `packages/app-core/src/index.ts`
- Modify: `crates/desktop/lib/chat-store.ts`
- Modify: `crates/desktop/lib/chat-runtime.ts`
- Modify: `crates/desktop/lib/types/chat.ts`
- Create: `crates/desktop/lib/transport/tauri-transport.ts`
- Modify: `crates/desktop/tests/chat-store-session-model.test.mjs`
- Modify: `crates/desktop/tests/chat-tauri-bindings.test.mjs`

**Step 1: Write the failing test**

```ts
import test from "node:test";
import assert from "node:assert/strict";

import { createChatStore } from "./chat-store";

test("sendMessage keeps pending state until snapshot refresh", async () => {
  const store = createChatStore({
    transport: {
      async sendMessage() {},
      async getThreadSnapshot() {
        return { messages: [], token_count: 0 };
      },
    },
  });

  await store.getState().sendMessage("hello");
  assert.equal(store.getState().activeSession?.pendingUserMessage, "hello");
});
```

**Step 2: Run test to verify it fails**

Run: `pnpm --filter @argus/app-core exec tsx --test src/features/chat/chat-store.test.ts`
Expected: FAIL because `createChatStore` and the shared chat feature do not exist yet.

**Step 3: Write minimal implementation**

```ts
export function createChatStore({ transport }: { transport: AppTransport }) {
  return create<ChatState>((set) => ({
    async sendMessage(content) {
      set({ activeSession: { pendingUserMessage: content } });
      await transport.sendMessage(/* ... */);
    },
  }));
}
```

Move the reusable logic out of `crates/desktop/lib/chat-store.ts` and `crates/desktop/lib/chat-runtime.ts`, keep only the desktop-specific Tauri adapter in `crates/desktop/lib/transport/tauri-transport.ts`, and update the existing desktop tests to import the shared feature instead of the old local implementation.

**Step 4: Run test to verify it passes**

Run: `pnpm --filter @argus/app-core exec tsx --test src/features/chat/chat-store.test.ts`
Run: `pnpm --filter @argus/desktop test -- chat-store-session-model.test.mjs chat-tauri-bindings.test.mjs`
Expected: PASS

**Step 5: Commit**

```bash
git add packages/app-core/src/features/chat packages/app-core/src/index.ts crates/desktop/lib/chat-store.ts crates/desktop/lib/chat-runtime.ts crates/desktop/lib/types/chat.ts crates/desktop/lib/transport/tauri-transport.ts crates/desktop/tests/chat-store-session-model.test.mjs crates/desktop/tests/chat-tauri-bindings.test.mjs
git commit -m "refactor: share chat store and runtime across clients"
```

### Task 3: Extract settings and thread monitor, and remove login from the shared shell

**Files:**
- Create: `packages/app-core/src/features/settings/index.ts`
- Create: `packages/app-core/src/features/settings/settings-routes.tsx`
- Create: `packages/app-core/src/features/thread-monitor/index.ts`
- Create: `packages/app-core/src/features/thread-monitor/thread-monitor-screen.tsx`
- Create: `packages/app-core/src/layout/app-shell.tsx`
- Create: `packages/app-core/src/layout/app-shell.test.tsx`
- Modify: `crates/desktop/router.tsx`
- Modify: `crates/desktop/app/layout.tsx`
- Modify: `crates/desktop/components/shadcn-studio/blocks/dashboard-shell-05/index.tsx`
- Modify: `crates/desktop/components/shadcn-studio/blocks/dropdown-profile.tsx`
- Modify: `crates/desktop/tests/router-smoke.test.tsx`
- Modify: `crates/desktop/tests/profile-dropdown-logout-confirm.test.mjs`

**Step 1: Write the failing test**

```tsx
import test from "node:test";
import assert from "node:assert/strict";
import { render, screen } from "@testing-library/react";

import { AppShell } from "./app-shell";

test("shared app shell renders without auth bootstrap", () => {
  render(<AppShell navigationItems={[]}><div>chat</div></AppShell>);
  assert.equal(screen.getByText("chat").textContent, "chat");
});
```

**Step 2: Run test to verify it fails**

Run: `pnpm --filter @argus/app-core exec tsx --test src/layout/app-shell.test.tsx`
Expected: FAIL because `AppShell` does not exist yet.

**Step 3: Write minimal implementation**

```tsx
export function AppShell(props: AppShellProps) {
  return (
    <TooltipProvider>
      <ToastProvider>{props.children}</ToastProvider>
    </TooltipProvider>
  );
}
```

Then move the common shell and page composition into `packages/app-core`, update `crates/desktop/router.tsx` to consume those shared routes, and remove the shared dependency on `useAuthStore` / login dialogs from `crates/desktop/app/layout.tsx` and the dashboard/profile shell. Do not delete `components/auth/*` in this pass; just stop routing the main product shell through them.

**Step 4: Run test to verify it passes**

Run: `pnpm --filter @argus/app-core exec tsx --test src/layout/app-shell.test.tsx`
Run: `pnpm --filter @argus/desktop test -- router-smoke.test.tsx profile-dropdown-logout-confirm.test.mjs`
Expected: PASS, and the desktop shell no longer requires login to render the main routes.

**Step 5: Commit**

```bash
git add packages/app-core/src/features/settings packages/app-core/src/features/thread-monitor packages/app-core/src/layout crates/desktop/router.tsx crates/desktop/app/layout.tsx crates/desktop/components/shadcn-studio/blocks/dashboard-shell-05/index.tsx crates/desktop/components/shadcn-studio/blocks/dropdown-profile.tsx crates/desktop/tests/router-smoke.test.tsx crates/desktop/tests/profile-dropdown-logout-confirm.test.mjs
git commit -m "refactor: share shell settings and thread monitor without login"
```

### Task 4: Scaffold `argus-server` and expose instance management REST endpoints

**Files:**
- Create: `crates/argus-server/AGENTS.md`
- Create: `crates/argus-server/Cargo.toml`
- Create: `crates/argus-server/src/main.rs`
- Create: `crates/argus-server/src/lib.rs`
- Create: `crates/argus-server/src/app_state.rs`
- Create: `crates/argus-server/src/error.rs`
- Create: `crates/argus-server/src/routes/mod.rs`
- Create: `crates/argus-server/src/routes/health.rs`
- Create: `crates/argus-server/src/routes/providers.rs`
- Create: `crates/argus-server/src/routes/templates.rs`
- Create: `crates/argus-server/tests/providers_api.rs`
- Modify: `Cargo.toml`
- Modify: `README.md`

**Step 1: Write the failing test**

```rust
#[tokio::test]
async fn list_providers_returns_ok() {
    let app = test_app().await;
    let response = app
        .oneshot(Request::get("/api/v1/providers").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-server providers_api -- --nocapture`
Expected: FAIL because `argus-server` does not exist yet.

**Step 3: Write minimal implementation**

```rust
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/providers", get(list_providers))
        .route("/api/v1/agents/templates", get(list_templates))
        .with_state(state)
}
```

Add `axum` to workspace dependencies, register `crates/argus-server` in the root workspace, create a crate-local `AGENTS.md` with the required `> 特性：...` sentence, and map `ArgusWing` provider/template calls into JSON response types plus a shared error envelope.

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-server providers_api -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add Cargo.toml README.md crates/argus-server
git commit -m "feat: scaffold argus server instance management api"
```

### Task 5: Add session, thread, message, snapshot, and SSE event routes

**Files:**
- Create: `crates/argus-server/src/routes/sessions.rs`
- Create: `crates/argus-server/src/routes/threads.rs`
- Create: `crates/argus-server/src/routes/messages.rs`
- Create: `crates/argus-server/src/routes/events.rs`
- Create: `crates/argus-server/src/response.rs`
- Create: `crates/argus-server/tests/sessions_api.rs`
- Create: `crates/argus-server/tests/thread_events_sse.rs`
- Modify: `crates/argus-server/src/routes/mod.rs`
- Modify: `crates/argus-server/src/lib.rs`

**Step 1: Write the failing test**

```rust
#[tokio::test]
async fn event_stream_responds_with_sse_content_type() {
    let app = test_app().await;
    let response = app
        .oneshot(Request::get("/api/v1/events").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-server thread_events_sse -- --nocapture`
Expected: FAIL because the SSE route does not exist yet.

**Step 3: Write minimal implementation**

```rust
pub async fn stream_events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(state.events.subscribe()).filter_map(to_sse_event);
    Sse::new(stream)
}
```

Also add REST handlers for:

- list/create sessions
- list/create/delete threads
- send message
- get thread snapshot
- thread pool snapshot
- job runtime snapshot

Keep the event envelope aligned with the current desktop `thread:event` handling so the shared frontend store can reuse its reducer logic.

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-server sessions_api -- --nocapture`
Run: `cargo test -p argus-server thread_events_sse -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-server/src/routes crates/argus-server/src/lib.rs crates/argus-server/src/response.rs crates/argus-server/tests/sessions_api.rs crates/argus-server/tests/thread_events_sse.rs
git commit -m "feat: add argus server chat routes and sse events"
```

### Task 6: Add the browser-facing HTTP + SSE transport

**Files:**
- Create: `packages/app-core/src/transport/http-sse-transport.ts`
- Create: `packages/app-core/src/transport/http-sse-transport.test.ts`
- Modify: `packages/app-core/src/transport/app-transport.ts`
- Modify: `packages/app-core/src/index.ts`

**Step 1: Write the failing test**

```ts
import test from "node:test";
import assert from "node:assert/strict";

import { createHttpSseTransport } from "./http-sse-transport";

test("http transport opens an EventSource for monitor events", () => {
  const transport = createHttpSseTransport({
    baseUrl: "http://127.0.0.1:9000",
    eventSource: class FakeEventSource {},
  });

  assert.equal(typeof transport.subscribeMonitorEvents, "function");
});
```

**Step 2: Run test to verify it fails**

Run: `pnpm --filter @argus/app-core exec tsx --test src/transport/http-sse-transport.test.ts`
Expected: FAIL because the HTTP transport does not exist yet.

**Step 3: Write minimal implementation**

```ts
export function createHttpSseTransport({ baseUrl, eventSource = EventSource }) {
  return {
    async subscribeMonitorEvents() {
      const source = new eventSource(`${baseUrl}/api/v1/events?channel=monitor`);
      return () => source.close();
    },
  } satisfies AppTransport;
}
```

Then implement the full fetch-based REST calls plus SSE subscriptions for thread and monitor channels, keeping the return types aligned with the shared DTOs from `packages/app-core`.

**Step 4: Run test to verify it passes**

Run: `pnpm --filter @argus/app-core exec tsx --test src/transport/http-sse-transport.test.ts`
Expected: PASS

**Step 5: Commit**

```bash
git add packages/app-core/src/transport/app-transport.ts packages/app-core/src/transport/http-sse-transport.ts packages/app-core/src/transport/http-sse-transport.test.ts packages/app-core/src/index.ts
git commit -m "feat: add http sse transport for shared frontend"
```

### Task 7: Scaffold `apps/web` and wire the shared features without login

**Files:**
- Create: `apps/web/package.json`
- Create: `apps/web/tsconfig.json`
- Create: `apps/web/vite.config.ts`
- Create: `apps/web/index.html`
- Create: `apps/web/src/main.tsx`
- Create: `apps/web/src/router.tsx`
- Create: `apps/web/src/app/layout.tsx`
- Create: `apps/web/src/app/page.tsx`
- Create: `apps/web/src/app/settings/page.tsx`
- Create: `apps/web/tests/router-smoke.test.tsx`
- Modify: `packages/app-core/src/index.ts`

**Step 1: Write the failing test**

```tsx
import test from "node:test";
import assert from "node:assert/strict";
import { render, screen } from "@testing-library/react";

import { createWebRouter } from "../src/router";

test("web router renders chat without login gate", async () => {
  render(<RouterProvider router={createWebRouter()} />);
  assert.ok(await screen.findByText("聊天"));
});
```

**Step 2: Run test to verify it fails**

Run: `pnpm --filter @argus/web exec tsx --test tests/router-smoke.test.tsx`
Expected: FAIL because `apps/web` does not exist yet.

**Step 3: Write minimal implementation**

```tsx
export function createWebRouter() {
  return createBrowserRouter([
    { path: "/", element: <SharedChatPage /> },
    { path: "/settings", element: <SharedSettingsPage /> },
  ]);
}
```

Wire `apps/web` to `createHttpSseTransport`, render the shared shell from `packages/app-core`, and keep the app unauthenticated. The first successful web shell only needs chat, settings, and thread monitor routes plus a configurable server base URL.

**Step 4: Run test to verify it passes**

Run: `pnpm --filter @argus/web exec tsx --test tests/router-smoke.test.tsx`
Run: `pnpm --filter @argus/web build`
Expected: PASS

**Step 5: Commit**

```bash
git add apps/web packages/app-core/src/index.ts
git commit -m "feat: add web shell for chat settings and thread monitor"
```

### Task 8: Rewire desktop to shared routes and isolate Tauri-only concerns

**Files:**
- Modify: `crates/desktop/main.tsx`
- Modify: `crates/desktop/router.tsx`
- Modify: `crates/desktop/app/page.tsx`
- Modify: `crates/desktop/app/settings/page.tsx`
- Modify: `crates/desktop/lib/tauri.ts`
- Modify: `crates/desktop/src-tauri/src/commands.rs`
- Modify: `crates/desktop/src-tauri/src/lib.rs`
- Modify: `crates/desktop/tests/router-smoke.test.tsx`
- Modify: `crates/desktop/tests/chat-page-runtime-integration.test.mjs`

**Step 1: Write the failing test**

```tsx
import test from "node:test";
import assert from "node:assert/strict";
import { render, screen } from "@testing-library/react";

import { createDesktopRouter } from "../router";

test("desktop router renders shared chat routes through tauri transport", async () => {
  render(<RouterProvider router={createDesktopRouter()} />);
  assert.ok(await screen.findByText("聊天"));
});
```

**Step 2: Run test to verify it fails**

Run: `pnpm --filter @argus/desktop test -- router-smoke.test.tsx chat-page-runtime-integration.test.mjs`
Expected: FAIL until the desktop router is pointed at the shared feature composition.

**Step 3: Write minimal implementation**

```tsx
export function createDesktopRouter() {
  const transport = createTauriTransport();
  return createSharedRouter({ transport, mode: "desktop" });
}
```

Keep Tauri-only concerns in desktop-owned files:

- `crates/desktop/lib/transport/tauri-transport.ts`
- `crates/desktop/src-tauri/*`
- any future local capability-node bootstrap

Do not move `@tauri-apps/api` imports into `packages/app-core`.

**Step 4: Run test to verify it passes**

Run: `pnpm --filter @argus/desktop test -- router-smoke.test.tsx chat-page-runtime-integration.test.mjs`
Run: `pnpm --filter @argus/desktop build`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/desktop/main.tsx crates/desktop/router.tsx crates/desktop/app/page.tsx crates/desktop/app/settings/page.tsx crates/desktop/lib/tauri.ts crates/desktop/src-tauri/src/commands.rs crates/desktop/src-tauri/src/lib.rs crates/desktop/tests/router-smoke.test.tsx crates/desktop/tests/chat-page-runtime-integration.test.mjs
git commit -m "refactor: point desktop shell at shared frontend core"
```

## Final Verification

Run these after Task 8:

```bash
cargo fmt --all
cargo test -p argus-server -- --nocapture
pnpm --filter @argus/app-core exec tsx --test src/transport/app-transport.test.ts src/transport/http-sse-transport.test.ts src/features/chat/chat-store.test.ts src/layout/app-shell.test.tsx
pnpm --filter @argus/desktop test
pnpm --filter @argus/desktop build
pnpm --filter @argus/web exec tsx --test tests/router-smoke.test.tsx
pnpm --filter @argus/web build
```

Expected:

- `argus-server` REST + SSE tests pass
- desktop still passes its existing frontend regression suite
- web renders `chat + settings + thread monitor` without any login gate
- shared frontend logic is exercised from both transports

## Notes for Execution

- Keep `argus-auth` in place unless a task is explicitly removing dead code; the first milestone only removes login from the user-facing flow.
- Prefer copying behavior-preserving tests from `crates/desktop/tests/*` into `packages/app-core` rather than inventing new assertions.
- Do not introduce `WebSocket` in the first pass; if an execution task starts to need bidirectional transport, stop and reopen design.
- Keep commits small and aligned to the task boundaries above.
