# Desktop Server + Web Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Deliver a usable `Vue + OpenTiny` web management console on top of a new Rust `argus-server` without rewiring desktop, extracting a shared frontend core, or migrating chat in the first phase.

**Architecture:** Phase 3A supersedes the earlier “server only through `ArgusWing`” rule. `argus-server` and `argus-wing` are peer application entries: desktop continues to use `ArgusWing`, while `argus-server` owns a private `ServerCore` assembly layer over the required lower crates. Add/maintain `crates/argus-server` as an `axum` management transport, and keep `apps/web` as a standalone `Vue 3 + OpenTiny Vue + Vite` admin app. Treat `apps/web/DESIGN.md` as the visual contract for tokens, typography, and component overrides. Defer chat/thread APIs, desktop rewiring, and `packages/app-core` until a later phase.

**Phase 3A boundary update:** Existing Phase 1 / Phase 2 endpoints remain wire-compatible. Do not reintroduce `argus-wing` as an `argus-server` dependency. Route handlers should call narrow `ServerCore` methods; direct lower manager/repository composition belongs only in `ServerCore`.

**Tech Stack:** Rust, `axum`, `tokio`, `serde`, Vue 3, OpenTiny Vue, Vite 8, TypeScript, Vue Router, Vitest, Vue Test Utils

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

Create the crate-local `AGENTS.md` with the required `> 特性：...` sentence, register `crates/argus-server` in the workspace, add a shared JSON error envelope, and wire `main.rs` to boot the server with an `AppState` built from `ServerCore`.

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

Map provider CRUD through `ServerCore`. If the route needs a new management operation, add the smallest narrow method to `ServerCore` instead of letting route handlers reach directly into lower-level managers or repositories.

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
- Create: `apps/web/DESIGN.md`
- Create: `apps/web/tsconfig.json`
- Create: `apps/web/vite.config.ts`
- Create: `apps/web/index.html`
- Create: `apps/web/src/main.ts`
- Create: `apps/web/src/App.vue`
- Create: `apps/web/src/router/index.ts`
- Create: `apps/web/src/layouts/AdminLayout.vue`
- Create: `apps/web/src/app/nav.ts`
- Create: `apps/web/src/styles/tokens.css`
- Create: `apps/web/src/lib/api.ts`
- Create: `apps/web/src/layouts/admin-layout.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";

import AdminLayout from "./AdminLayout.vue";

describe("AdminLayout", () => {
  it("renders management navigation", () => {
    const wrapper = mount(AdminLayout);
    expect(wrapper.text()).toContain("Providers");
  });
});
```

**Step 2: Run test to verify it fails**

Run: `cd apps/web && pnpm exec vitest run src/layouts/admin-layout.test.ts`
Expected: FAIL because the web app does not exist yet.

**Step 3: Write minimal implementation**

```vue
<template>
  <div class="admin-layout">
    <nav>
      <a href="/providers">Providers</a>
    </nav>
    <main />
  </div>
</template>
```

Set up `apps/web` as a standalone `Vue 3 + OpenTiny Vue` app. Create `apps/web/DESIGN.md` from the approved Linear-inspired design brief and wire `src/styles/tokens.css` to start mapping that brief into CSS variables and OpenTiny theme overrides. Do not pull in `crates/desktop` code, do not create `packages/app-core`, and do not change desktop build configuration in this task.

**Step 4: Run test to verify it passes**

Run: `cd apps/web && pnpm install`
Run: `cd apps/web && pnpm exec vitest run src/layouts/admin-layout.test.ts`
Expected: PASS

**Step 5: Commit**

```bash
git add apps/web
git commit -m "feat: scaffold web admin shell"
```

### Task 6: Implement the first real admin flow with provider management

**Files:**
- Create: `apps/web/src/features/providers/ProvidersPage.vue`
- Create: `apps/web/src/features/providers/ProviderForm.vue`
- Create: `apps/web/src/features/providers/providers-page.test.ts`
- Modify: `apps/web/src/router/index.ts`
- Modify: `apps/web/src/lib/api.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";

import ProvidersPage from "./ProvidersPage.vue";

describe("ProvidersPage", () => {
  it("loads provider rows from the server", async () => {
    const wrapper = mount(ProvidersPage);
    await flushPromises();
    expect(wrapper.text()).toContain("OpenAI");
  });
});
```

**Step 2: Run test to verify it fails**

Run: `cd apps/web && pnpm exec vitest run src/features/providers/providers-page.test.ts`
Expected: FAIL because the providers page is not implemented yet.

**Step 3: Write minimal implementation**

```vue
<script setup lang="ts">
import { onMounted, ref } from "vue";

const providers = ref<ProviderSummary[]>([]);

onMounted(async () => {
  providers.value = await api.listProviders();
});
</script>

<template>
  <div>
    <div v-for="provider in providers" :key="provider.id">{{ provider.name }}</div>
  </div>
</template>
```

This is the first required real management loop. Use OpenTiny form and table primitives rather than bespoke controls, and make sure the page can read provider data and persist at least one edit or create path back to the server.

**Step 4: Run test to verify it passes**

Run: `cd apps/web && pnpm exec vitest run src/features/providers/providers-page.test.ts`
Expected: PASS

**Step 5: Commit**

```bash
git add apps/web/src/features/providers apps/web/src/router/index.ts apps/web/src/lib/api.ts
git commit -m "feat: add provider management flow to web admin"
```

### Task 7: Implement templates, MCP, settings, and health pages

**Files:**
- Create: `apps/web/src/features/templates/TemplatesPage.vue`
- Create: `apps/web/src/features/templates/templates-page.test.ts`
- Create: `apps/web/src/features/mcp/McpPage.vue`
- Create: `apps/web/src/features/mcp/mcp-page.test.ts`
- Create: `apps/web/src/features/settings/SettingsPage.vue`
- Create: `apps/web/src/features/settings/settings-page.test.ts`
- Create: `apps/web/src/features/health/HealthPage.vue`
- Create: `apps/web/src/features/health/health-page.test.ts`
- Modify: `apps/web/src/router/index.ts`
- Modify: `apps/web/src/lib/api.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";

import HealthPage from "./HealthPage.vue";

describe("HealthPage", () => {
  it("shows service status", async () => {
    const wrapper = mount(HealthPage);
    await flushPromises();
    expect(wrapper.text()).toContain("Healthy");
  });
});
```

**Step 2: Run test to verify it fails**

Run: `cd apps/web && pnpm exec vitest run src/features/health/health-page.test.ts`
Expected: FAIL because the health and remaining admin pages do not exist yet.

**Step 3: Write minimal implementation**

```vue
<script setup lang="ts">
import { onMounted, ref } from "vue";

const status = ref("Loading");

onMounted(async () => {
  const result = await api.getHealth();
  status.value = result.status;
});
</script>

<template>
  <div>{{ status }}</div>
</template>
```

Complete the remaining management pages using the REST routes added in earlier tasks. Keep the UI focused on instance administration, and express the DESIGN.md rules through OpenTiny theme overrides plus local token CSS rather than ad-hoc inline styling. Do not add chat routes or runtime event subscriptions.

**Step 4: Run test to verify it passes**

Run: `cd apps/web && pnpm exec vitest run src/features/templates/templates-page.test.ts`
Run: `cd apps/web && pnpm exec vitest run src/features/mcp/mcp-page.test.ts`
Run: `cd apps/web && pnpm exec vitest run src/features/settings/settings-page.test.ts`
Run: `cd apps/web && pnpm exec vitest run src/features/health/health-page.test.ts`
Expected: PASS

**Step 5: Commit**

```bash
git add apps/web/src/features/templates apps/web/src/features/mcp apps/web/src/features/settings apps/web/src/features/health apps/web/src/router/index.ts apps/web/src/lib/api.ts apps/web/src/styles/tokens.css
git commit -m "feat: add remaining admin console pages"
```

### Task 8: Add a usable-console smoke path and document deferred work

**Files:**
- Create: `apps/web/src/app/admin-console.smoke.test.ts`
- Modify: `docs/plans/2026-04-23-desktop-server-web-design.md`
- Modify: `README.md`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";

import App from "../App.vue";

describe("admin console", () => {
  it("exposes core management entry points", () => {
    const wrapper = mount(App);
    expect(wrapper.text()).toContain("Providers");
    expect(wrapper.text()).toContain("Templates");
    expect(wrapper.text()).toContain("MCP Servers");
    expect(wrapper.text()).toContain("Settings");
  });
});
```

**Step 2: Run test to verify it fails**

Run: `cd apps/web && pnpm exec vitest run src/app/admin-console.smoke.test.ts`
Expected: FAIL until the full admin shell is wired together.

**Step 3: Write minimal implementation**

```vue
<template>
  <RouterView />
</template>
```

Then update the docs to explicitly record what was deferred from phase 1: `SSE`, `thread monitor`, chat routes, shared frontend core, and desktop rewiring. Also keep `apps/web/DESIGN.md` in sync if implementation constraints change.

**Step 4: Run test to verify it passes**

Run: `cd apps/web && pnpm exec vitest run src/app/admin-console.smoke.test.ts`
Run: `cargo test -p argus-server -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add apps/web/src/app/admin-console.smoke.test.ts docs/plans/2026-04-23-desktop-server-web-design.md README.md apps/web/DESIGN.md
git commit -m "test: lock usable admin console phase one scope"
```

### Phase 2A: Add runtime snapshot monitoring without SSE

**Intent:** Start phase 2 by exposing existing runtime telemetry through a narrow REST surface and a web monitor page. Keep `SSE`, timeline streaming, and thread-level drilldown for a later batch.

**Files (expected):**
- Create: `crates/argus-server/src/routes/runtime.rs`
- Create: `crates/argus-server/tests/runtime_api.rs`
- Create: `apps/web/src/features/runtime/RuntimePage.vue`
- Create: `apps/web/src/features/runtime/runtime-page.test.ts`
- Modify: `crates/argus-server/src/routes/mod.rs`
- Modify: `apps/web/src/lib/api.ts`
- Modify: `apps/web/src/router/index.ts`
- Modify: `apps/web/src/app/nav.ts`

**Expected API shape:**

```text
GET /api/v1/runtime
```

Response should contain:

- `thread_pool`: `ServerCore::thread_pool_state()`
- `job_runtime`: `ServerCore::job_runtime_state()`

**Expected UI shape:**

- New “运行状态”页面
- Summary cards for active / queued / running / cooling / evicted counts
- A runtime list for thread pool entries
- A runtime list for background job entries
- Client-side polling for refresh, without `SSE`

### Phase 2B: Add runtime SSE updates with polling fallback

**Intent:** Replace always-on polling with an event-stream-first runtime monitor. Keep the stream narrow by emitting runtime snapshots through `ServerCore`; defer per-thread timelines and chat event subscriptions.

### Phase 3A: Decouple argus-server from ArgusWing

**Intent:** Make `argus-server` a peer application entry with private assembly, while keeping the existing management HTTP/SSE contract stable.

**Expected server shape:**

- `AppState` holds `Arc<ServerCore>`
- `ServerCore::init(database_path)` resolves database path, connects, migrates, assembles managers/runtimes, and seeds builtin templates
- `ServerCore::with_pool(pool)` supports in-memory SQLite tests
- `argus-server` depends directly on the required lower crates and does not depend on `argus-wing`
- Phase 3B supersedes the initial settings note: `AdminSettings` is persisted through repository-backed SQLite storage with default `instance_name = "ArgusWing"`

**Expected validation:**

- `cargo test -p argus-server -- --nocapture`
- `cargo tree -p argus-server | rg argus-wing` has no matches
- `rg 'argus_wing|ArgusWing' crates/argus-server` only shows architecture assertions or user-visible brand/default test text

**Files (expected):**
- Modify: `crates/argus-server/Cargo.toml`
- Modify: `crates/argus-server/src/routes/runtime.rs`
- Modify: `crates/argus-server/src/routes/mod.rs`
- Modify: `crates/argus-server/tests/runtime_api.rs`
- Modify: `apps/web/src/lib/api.ts`
- Modify: `apps/web/src/features/runtime/RuntimePage.vue`
- Modify: `apps/web/src/features/runtime/runtime-page.test.ts`

**Expected API shape:**

```text
GET /api/v1/runtime/events
```

The endpoint should return `text/event-stream` and emit `runtime.snapshot` events whose payload matches `GET /api/v1/runtime`.

**Expected UI shape:**

- The “运行状态” page uses `EventSource` when available
- The page shows the event-stream connection state
- Incoming `runtime.snapshot` events update the summary cards and runtime lists
- Stream errors fall back to 5-second REST polling

### Phase 3B: Productize admin management loops

**Intent:** Keep web and desktop independent while making the standalone management console usable for real instance administration.

**Expected server shape:**

- `argus-repository` owns SQLite-backed single-row admin settings storage
- `ServerCore` loads settings on startup and persists `PUT /settings`
- Providers, templates, and MCP expose delete operations through narrow `ServerCore` methods
- Providers and MCP expose connection test routes
- MCP exposes discovered tools for an existing server
- Server bind address reads `ARGUS_SERVER_ADDR`, defaulting to `127.0.0.1:3000`

**Expected API shape:**

```text
DELETE /api/v1/providers/{provider_id}
POST /api/v1/providers/{provider_id}/test
POST /api/v1/providers/test
DELETE /api/v1/agents/templates/{template_id}
DELETE /api/v1/mcp/servers/{server_id}
POST /api/v1/mcp/servers/{server_id}/test
POST /api/v1/mcp/servers/test
GET /api/v1/mcp/servers/{server_id}/tools
```

**Expected UI shape:**

- Chinese light-mode admin pages show delete, test, save, error, empty, and refresh feedback
- Provider management has a complete add/edit/test/delete loop
- MCP management can test servers and inspect discovered tools
- Templates can be removed from the admin console

### Phase 4A: Improve MCP operations visibility

**Intent:** Improve the management console's MCP operations view without adding new server contracts.

**Expected UI shape:**

- MCP page shows service, ready, attention, and discovered-tool summary cards
- Each MCP server card shows transport target, timeout, last check, last success, and last error diagnostics
- Existing discovered-tools API renders tool names, descriptions, and schema previews
- Refresh, test, tools, and delete actions keep the existing REST contract unchanged
