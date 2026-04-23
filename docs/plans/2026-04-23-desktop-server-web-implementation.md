# Desktop Server + Web Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Deliver a usable web management console on top of a new Rust `argus-server` without rewiring desktop, extracting a shared frontend core, or migrating chat in the first phase.

**Architecture:** Keep `ArgusWing` as the only business facade. Add `crates/argus-server` as an `axum` REST transport for instance management, and add a standalone `apps/web` Vite app with a new admin information architecture. Defer `SSE`, chat/thread APIs, desktop rewiring, and `packages/app-core` until a later phase.

**Tech Stack:** Rust, `axum`, `tokio`, `serde`, React 19, Vite 8, TypeScript, React Router, Testing Library, `tsx --test`

---

### Task 1: Scaffold `argus-server` with health and shared error handling

**Files:**
- Create: `crates/argus-server/AGENTS.md`
- Create: `crates/argus-server/Cargo.toml`
- Create: `crates/argus-server/src/main.rs`
- Create: `crates/argus-server/src/lib.rs`
- Create: `crates/argus-server/src/app_state.rs`
- Create: `crates/argus-server/src/error.rs`
- Create: `crates/argus-server/src/routes/mod.rs`
- Create: `crates/argus-server/src/routes/health.rs`
- Create: `crates/argus-server/tests/health_api.rs`
- Modify: `Cargo.toml`
- Modify: `README.md`

**Step 1: Write the failing test**

```rust
#[tokio::test]
async fn health_returns_ok() {
    let app = test_app().await;
    let response = app
        .oneshot(Request::get("/api/v1/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-server health_api -- --nocapture`
Expected: FAIL because `argus-server` does not exist yet.

**Step 3: Write minimal implementation**

```rust
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .with_state(state)
}
```

Create the crate-local `AGENTS.md` with the required `> 特性：...` sentence, register `crates/argus-server` in the workspace, add a shared JSON error envelope, and wire `main.rs` to boot the server with an `AppState` built from `ArgusWing`.

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-server health_api -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add Cargo.toml README.md crates/argus-server
git commit -m "feat: scaffold argus server health api"
```

### Task 2: Add bootstrap and settings REST routes for the admin console

**Files:**
- Create: `crates/argus-server/src/routes/bootstrap.rs`
- Create: `crates/argus-server/src/routes/settings.rs`
- Create: `crates/argus-server/src/response.rs`
- Create: `crates/argus-server/tests/bootstrap_api.rs`
- Create: `crates/argus-server/tests/settings_api.rs`
- Modify: `crates/argus-server/src/routes/mod.rs`
- Modify: `crates/argus-server/src/lib.rs`

**Step 1: Write the failing test**

```rust
#[tokio::test]
async fn bootstrap_returns_instance_summary() {
    let app = test_app().await;
    let response = app
        .oneshot(Request::get("/api/v1/bootstrap").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-server bootstrap_api -- --nocapture`
Expected: FAIL because the bootstrap route does not exist yet.

**Step 3: Write minimal implementation**

```rust
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/bootstrap", get(get_bootstrap))
        .route("/api/v1/settings", get(get_settings).put(update_settings))
        .with_state(state)
}
```

Make `bootstrap` return only the minimum data the web shell needs to render instance-level navigation and status. Keep settings scoped to instance management; do not add auth or per-user profile fields.

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-server bootstrap_api -- --nocapture`
Run: `cargo test -p argus-server settings_api -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-server/src/routes/bootstrap.rs crates/argus-server/src/routes/settings.rs crates/argus-server/src/response.rs crates/argus-server/tests/bootstrap_api.rs crates/argus-server/tests/settings_api.rs crates/argus-server/src/routes/mod.rs crates/argus-server/src/lib.rs
git commit -m "feat: add bootstrap and settings admin routes"
```

### Task 3: Add provider management REST routes

**Files:**
- Create: `crates/argus-server/src/routes/providers.rs`
- Create: `crates/argus-server/tests/providers_api.rs`
- Modify: `crates/argus-server/src/routes/mod.rs`
- Modify: `crates/argus-server/src/lib.rs`

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
Expected: FAIL because provider management routes do not exist yet.

**Step 3: Write minimal implementation**

```rust
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/providers", get(list_providers).post(create_provider))
        .route("/api/v1/providers/:provider_id", patch(update_provider))
        .with_state(state)
}
```

Map provider CRUD through `ArgusWing`. If the facade is missing exactly the right entry points, add the smallest missing facade methods there instead of bypassing into lower-level managers from `argus-server`.

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-server providers_api -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-server/src/routes/providers.rs crates/argus-server/tests/providers_api.rs crates/argus-server/src/routes/mod.rs crates/argus-server/src/lib.rs
git commit -m "feat: add provider management api"
```

### Task 4: Add template and MCP management REST routes

**Files:**
- Create: `crates/argus-server/src/routes/templates.rs`
- Create: `crates/argus-server/src/routes/mcp.rs`
- Create: `crates/argus-server/tests/templates_api.rs`
- Create: `crates/argus-server/tests/mcp_api.rs`
- Modify: `crates/argus-server/src/routes/mod.rs`
- Modify: `crates/argus-server/src/lib.rs`

**Step 1: Write the failing test**

```rust
#[tokio::test]
async fn list_templates_returns_ok() {
    let app = test_app().await;
    let response = app
        .oneshot(Request::get("/api/v1/agents/templates").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p argus-server templates_api -- --nocapture`
Expected: FAIL because template and MCP routes do not exist yet.

**Step 3: Write minimal implementation**

```rust
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/agents/templates", get(list_templates).post(create_template))
        .route("/api/v1/agents/templates/:template_id", patch(update_template))
        .route("/api/v1/mcp/servers", get(list_mcp_servers).post(create_mcp_server))
        .route("/api/v1/mcp/servers/:server_id", patch(update_mcp_server))
        .with_state(state)
}
```

Keep the route surface narrow and instance-focused. Do not add session, thread, message, or event routes in this task.

**Step 4: Run test to verify it passes**

Run: `cargo test -p argus-server templates_api -- --nocapture`
Run: `cargo test -p argus-server mcp_api -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/argus-server/src/routes/templates.rs crates/argus-server/src/routes/mcp.rs crates/argus-server/tests/templates_api.rs crates/argus-server/tests/mcp_api.rs crates/argus-server/src/routes/mod.rs crates/argus-server/src/lib.rs
git commit -m "feat: add template and mcp management api"
```

### Task 5: Scaffold a standalone `apps/web` admin shell

**Files:**
- Create: `apps/web/package.json`
- Create: `apps/web/tsconfig.json`
- Create: `apps/web/vite.config.ts`
- Create: `apps/web/index.html`
- Create: `apps/web/src/main.tsx`
- Create: `apps/web/src/app/router.tsx`
- Create: `apps/web/src/app/layout.tsx`
- Create: `apps/web/src/app/nav.ts`
- Create: `apps/web/src/lib/api.ts`
- Create: `apps/web/src/app/layout.test.tsx`

**Step 1: Write the failing test**

```tsx
import test from "node:test";
import assert from "node:assert/strict";
import { render, screen } from "@testing-library/react";

import { AppLayout } from "./layout";

test("admin shell renders management navigation", () => {
  render(<AppLayout />);
  assert.equal(screen.getByText("Providers").textContent, "Providers");
});
```

**Step 2: Run test to verify it fails**

Run: `cd apps/web && pnpm exec tsx --test src/app/layout.test.tsx`
Expected: FAIL because the web app does not exist yet.

**Step 3: Write minimal implementation**

```tsx
export function AppLayout() {
  return (
    <div>
      <nav>
        <a href="/providers">Providers</a>
      </nav>
      <main />
    </div>
  );
}
```

Set up `apps/web` as a standalone app. Do not pull in `crates/desktop` code, do not create `packages/app-core`, and do not change desktop build configuration in this task.

**Step 4: Run test to verify it passes**

Run: `cd apps/web && pnpm install`
Run: `cd apps/web && pnpm exec tsx --test src/app/layout.test.tsx`
Expected: PASS

**Step 5: Commit**

```bash
git add apps/web
git commit -m "feat: scaffold web admin shell"
```

### Task 6: Implement the first real admin flow with provider management

**Files:**
- Create: `apps/web/src/features/providers/providers-page.tsx`
- Create: `apps/web/src/features/providers/provider-form.tsx`
- Create: `apps/web/src/features/providers/providers-page.test.tsx`
- Modify: `apps/web/src/app/router.tsx`
- Modify: `apps/web/src/lib/api.ts`

**Step 1: Write the failing test**

```tsx
import test from "node:test";
import assert from "node:assert/strict";
import { render, screen } from "@testing-library/react";

import { ProvidersPage } from "./providers-page";

test("providers page loads provider rows from the server", async () => {
  render(<ProvidersPage />);
  assert.ok(await screen.findByText("OpenAI"));
});
```

**Step 2: Run test to verify it fails**

Run: `cd apps/web && pnpm exec tsx --test src/features/providers/providers-page.test.tsx`
Expected: FAIL because the providers page is not implemented yet.

**Step 3: Write minimal implementation**

```tsx
export function ProvidersPage() {
  const [providers, setProviders] = useState<ProviderSummary[]>([]);

  useEffect(() => {
    api.listProviders().then(setProviders);
  }, []);

  return <div>{providers.map((provider) => <div key={provider.id}>{provider.name}</div>)}</div>;
}
```

This is the first required real management loop. Make sure the page can read provider data and persist at least one edit or create path back to the server.

**Step 4: Run test to verify it passes**

Run: `cd apps/web && pnpm exec tsx --test src/features/providers/providers-page.test.tsx`
Expected: PASS

**Step 5: Commit**

```bash
git add apps/web/src/features/providers apps/web/src/app/router.tsx apps/web/src/lib/api.ts
git commit -m "feat: add provider management flow to web admin"
```

### Task 7: Implement templates, MCP, settings, and health pages

**Files:**
- Create: `apps/web/src/features/templates/templates-page.tsx`
- Create: `apps/web/src/features/templates/templates-page.test.tsx`
- Create: `apps/web/src/features/mcp/mcp-page.tsx`
- Create: `apps/web/src/features/mcp/mcp-page.test.tsx`
- Create: `apps/web/src/features/settings/settings-page.tsx`
- Create: `apps/web/src/features/settings/settings-page.test.tsx`
- Create: `apps/web/src/features/health/health-page.tsx`
- Create: `apps/web/src/features/health/health-page.test.tsx`
- Modify: `apps/web/src/app/router.tsx`
- Modify: `apps/web/src/lib/api.ts`

**Step 1: Write the failing test**

```tsx
import test from "node:test";
import assert from "node:assert/strict";
import { render, screen } from "@testing-library/react";

import { HealthPage } from "./health-page";

test("health page shows service status", async () => {
  render(<HealthPage />);
  assert.ok(await screen.findByText("Healthy"));
});
```

**Step 2: Run test to verify it fails**

Run: `cd apps/web && pnpm exec tsx --test src/features/health/health-page.test.tsx`
Expected: FAIL because the health and remaining admin pages do not exist yet.

**Step 3: Write minimal implementation**

```tsx
export function HealthPage() {
  const [status, setStatus] = useState("Loading");

  useEffect(() => {
    api.getHealth().then((result) => setStatus(result.status));
  }, []);

  return <div>{status}</div>;
}
```

Complete the remaining management pages using the REST routes added in earlier tasks. Keep the UI focused on instance administration; do not add chat routes or runtime event subscriptions.

**Step 4: Run test to verify it passes**

Run: `cd apps/web && pnpm exec tsx --test src/features/templates/templates-page.test.tsx`
Run: `cd apps/web && pnpm exec tsx --test src/features/mcp/mcp-page.test.tsx`
Run: `cd apps/web && pnpm exec tsx --test src/features/settings/settings-page.test.tsx`
Run: `cd apps/web && pnpm exec tsx --test src/features/health/health-page.test.tsx`
Expected: PASS

**Step 5: Commit**

```bash
git add apps/web/src/features/templates apps/web/src/features/mcp apps/web/src/features/settings apps/web/src/features/health apps/web/src/app/router.tsx apps/web/src/lib/api.ts
git commit -m "feat: add remaining admin console pages"
```

### Task 8: Add a usable-console smoke path and document deferred work

**Files:**
- Create: `apps/web/src/app/admin-console.smoke.test.tsx`
- Modify: `docs/plans/2026-04-23-desktop-server-web-design.md`
- Modify: `README.md`

**Step 1: Write the failing test**

```tsx
import test from "node:test";
import assert from "node:assert/strict";
import { render, screen } from "@testing-library/react";

import { AppRouter } from "./router";

test("admin console exposes core management entry points", async () => {
  render(<AppRouter />);
  assert.ok(await screen.findByText("Providers"));
  assert.ok(await screen.findByText("Templates"));
  assert.ok(await screen.findByText("MCP Servers"));
  assert.ok(await screen.findByText("Settings"));
});
```

**Step 2: Run test to verify it fails**

Run: `cd apps/web && pnpm exec tsx --test src/app/admin-console.smoke.test.tsx`
Expected: FAIL until the full admin shell is wired together.

**Step 3: Write minimal implementation**

```tsx
export function AppRouter() {
  return <RouterProvider router={router} />;
}
```

Then update the docs to explicitly record what was deferred from phase 1: `SSE`, `thread monitor`, chat routes, shared frontend core, and desktop rewiring.

**Step 4: Run test to verify it passes**

Run: `cd apps/web && pnpm exec tsx --test src/app/admin-console.smoke.test.tsx`
Run: `cargo test -p argus-server -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add apps/web/src/app/admin-console.smoke.test.tsx docs/plans/2026-04-23-desktop-server-web-design.md README.md
git commit -m "test: lock usable admin console phase one scope"
```
