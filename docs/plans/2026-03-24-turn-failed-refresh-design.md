# Turn Failed Refresh Preservation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Preserve frontend error state when a turn fails while still refreshing the thread snapshot.

**Architecture:** Keep using the existing `turn_failed` event. Extend `refreshSnapshot` with an optional mode that preserves session error state/status when invoked from failure handling, while normal `idle` refreshes continue resetting the session to idle.

**Tech Stack:** Tauri, Zustand, TypeScript, node:test

---

### Task 1: Lock the regression with a store test

**Files:**
- Modify: `crates/desktop/tests/chat-store-session-model.test.mjs`
- Modify: `crates/desktop/lib/chat-store.ts`

**Step 1: Write the failing test**
- Assert the `turn_failed` branch refreshes with a preserve-error flag.
- Assert `refreshSnapshot` can keep `status: "error"` instead of always forcing `"idle"`.

**Step 2: Run test to verify it fails**
- Run: `node --test crates/desktop/tests/chat-store-session-model.test.mjs`

**Step 3: Write minimal implementation**
- Add an optional `preserveError` parameter to `refreshSnapshot`.
- Use it from `turn_failed`.
- Keep existing `idle` behavior unchanged.

**Step 4: Run test to verify it passes**
- Run: `node --test crates/desktop/tests/chat-store-session-model.test.mjs`

