# Provider Editor Unification Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Unify desktop provider creation and editing into a shared full-page editor with the same left-right structure as agent editing.

**Architecture:** Replace the provider dialog flow with route-based pages for create and edit, backed by one `ProviderEditor` component. Move draft connection testing into that editor so create/edit share the same save and test actions, while the provider list keeps its lightweight persisted-status test entrypoint.

**Tech Stack:** Next.js app router, React 19, Tauri v2 invoke bindings, node:test source assertions.

---

### Task 1: Lock the new provider editor routes in tests

**Files:**
- Modify: `crates/desktop/tests/settings-editing-flows.test.mjs`
- Modify: `crates/desktop/tests/provider-connection-flow.test.mjs`

**Step 1: Write the failing test**
- Assert provider add/edit flows render route pages instead of `ProviderFormDialog`.
- Assert the new editor has a two-column shell and draft test action.

**Step 2: Run test to verify it fails**
Run: `node --test crates/desktop/tests/settings-editing-flows.test.mjs crates/desktop/tests/provider-connection-flow.test.mjs`
Expected: FAIL because provider routes and editor component do not exist yet.

**Step 3: Write minimal implementation**
- Add `/settings/providers/new` and `/settings/providers/[id]` pages.
- Add a shared `ProviderEditor` component and point tests at it.

**Step 4: Run test to verify it passes**
Run: `node --test crates/desktop/tests/settings-editing-flows.test.mjs crates/desktop/tests/provider-connection-flow.test.mjs`
Expected: PASS.

### Task 2: Move provider save/test behavior into the shared editor

**Files:**
- Create: `crates/desktop/components/settings/provider-editor.tsx`
- Modify: `crates/desktop/components/settings/index.ts`
- Modify: `crates/desktop/lib/tauri.ts` if typing adjustments are required

**Step 1: Write the failing test**
- Assert the editor owns draft test state and uses `providers.testInput(record, model)`.
- Assert save uses `providers.upsert` and returns to `/settings/providers`.

**Step 2: Run test to verify it fails**
Run: `node --test crates/desktop/tests/provider-connection-flow.test.mjs`
Expected: FAIL on missing editor behavior.

**Step 3: Write minimal implementation**
- Build `ProviderEditor` with left-side form and right-side connection panel.
- Reuse existing type shapes and test-result rendering patterns.

**Step 4: Run test to verify it passes**
Run: `node --test crates/desktop/tests/provider-connection-flow.test.mjs`
Expected: PASS.

### Task 3: Repoint the provider list to route-based editing

**Files:**
- Modify: `crates/desktop/app/settings/providers/page.tsx`
- Modify: `crates/desktop/components/settings/provider-card.tsx`
- Delete or stop using: `crates/desktop/components/settings/provider-form-dialog.tsx`

**Step 1: Write the failing test**
- Assert add/edit actions link to the new provider routes.
- Assert the list page no longer controls provider dialog state.

**Step 2: Run test to verify it fails**
Run: `node --test crates/desktop/tests/settings-editing-flows.test.mjs`
Expected: FAIL because the page still opens dialogs.

**Step 3: Write minimal implementation**
- Replace dialog triggers with links/navigation handlers.
- Keep delete confirmation and persisted connection-status dialog on the list page.

**Step 4: Run test to verify it passes**
Run: `node --test crates/desktop/tests/settings-editing-flows.test.mjs`
Expected: PASS.

### Task 4: Verify the desktop surface end-to-end

**Files:**
- Modify: affected tests only if assertions need current signature updates

**Step 1: Run targeted desktop tests**
Run: `node --test crates/desktop/tests/*.test.mjs`
Expected: PASS.

**Step 2: Run production build**
Run: `pnpm build`
Expected: PASS in `crates/desktop`.
