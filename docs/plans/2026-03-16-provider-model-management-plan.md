# Provider Model Management Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Restore multi-model configuration while creating a provider.

**Architecture:** Keep provider save flow unchanged at the page boundary, and make `ProviderFormDialog` own draft-model state for unsaved providers. After a provider save succeeds, flush draft models through the existing Tauri model commands and refresh the persisted model list.

**Tech Stack:** Next.js app router, React 19, TypeScript, Tauri invoke APIs.

---

### Task 1: Lock in the missing capability

**Files:**
- Create: `crates/desktop/tests/provider-model-management.test.mjs`
- Test: `crates/desktop/tests/provider-model-management.test.mjs`

**Step 1:** Write a failing test that asserts `ProviderFormDialog` exposes model management for unsaved providers.

**Step 2:** Run `node --test tests/provider-model-management.test.mjs` and confirm it fails.

### Task 2: Implement unsaved-provider model drafting

**Files:**
- Modify: `crates/desktop/components/settings/provider-form-dialog.tsx`

**Step 1:** Add draft model state and helpers for add/delete/set-default before provider persistence.

**Step 2:** Make the model section render for both unsaved and saved providers.

**Step 3:** After provider save succeeds, persist draft models and refresh the saved model list.

**Step 4:** Surface inline errors if draft model persistence fails.

### Task 3: Verify the flow

**Files:**
- Test: `crates/desktop/tests/provider-model-management.test.mjs`
- Verify: `crates/desktop/components/settings/provider-form-dialog.tsx`

**Step 1:** Re-run `node --test tests/provider-model-management.test.mjs`.

**Step 2:** Run `pnpm exec tsc --noEmit`.

**Step 3:** Run `pnpm exec eslint components/settings/provider-form-dialog.tsx tests/provider-model-management.test.mjs`.

**Step 4:** Run `pnpm build`.
