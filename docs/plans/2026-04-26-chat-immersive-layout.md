# Chat Immersive Layout Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Convert `/chat` from a card-heavy admin page into an immersive chat workspace with a full-height message stage and a viewport-fixed bottom composer.

**Architecture:** Mark the `/chat` route as an immersive layout mode so `AdminLayout` can hide the shared route header and lift width constraints for this one page. Simplify the chat page so the conversation stage becomes the only primary surface, while the composer becomes a fixed bottom dock that owns model/session controls.

**Tech Stack:** Vue 3 SFCs, Vue Router route meta, TinyRobot sender/bubble components, Vitest, Vue Test Utils

---

### Task 1: Route-level immersive mode

**Files:**
- Modify: `apps/web/src/router/index.ts`
- Modify: `apps/web/src/layouts/AdminLayout.vue`
- Test: `apps/web/src/layouts/AdminLayout.test.ts`

**Step 1: Write the failing test**

Assert that the `/chat` route hides the shared route header and applies a chat-specific layout class, while non-chat routes keep the existing header.

**Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/layouts/AdminLayout.test.ts`

**Step 3: Write minimal implementation**

Add route meta such as `immersive: true` / `hideRouteHeader: true`, wire `AdminLayout` to that meta, and relax the width cap for immersive routes.

**Step 4: Run test to verify it passes**

Run: `pnpm exec vitest run src/layouts/AdminLayout.test.ts`

### Task 2: Simplify the chat page chrome

**Files:**
- Modify: `apps/web/src/features/chat/ChatPage.vue`
- Modify: `apps/web/src/features/chat/components/ChatConversationPanel.vue`
- Modify: `apps/web/src/features/chat/chat-page.test.ts`

**Step 1: Write the failing test**

Assert that the chat page no longer renders the conversation header card chrome (`Conversation`, thread title, refresh/activate/cancel actions) and that the message stage remains the main surface.

**Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/features/chat/chat-page.test.ts`

**Step 3: Write minimal implementation**

Remove the top conversation header/actions from `ChatConversationPanel`, flatten the panel into an immersive workspace surface, and keep only inline notices plus the message stage.

**Step 4: Run test to verify it passes**

Run: `pnpm exec vitest run src/features/chat/chat-page.test.ts`

### Task 3: Fix the composer to the viewport bottom

**Files:**
- Modify: `apps/web/src/features/chat/ChatPage.vue`
- Modify: `apps/web/src/features/chat/components/ChatComposerBar.vue`
- Test: `apps/web/src/features/chat/components/ChatComposerBar.test.ts`

**Step 1: Write the failing test**

Assert that the composer uses a dedicated fixed-dock class and still keeps the compact sender mode.

**Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/features/chat/components/ChatComposerBar.test.ts`

**Step 3: Write minimal implementation**

Move the composer into a fixed bottom dock, keep the controls inside that dock, and add page padding so the message list never disappears behind it.

**Step 4: Run test to verify it passes**

Run: `pnpm exec vitest run src/features/chat/components/ChatComposerBar.test.ts`

### Task 4: Verify the integrated chat experience

**Files:**
- Re-run existing tests only

**Step 1: Run focused verification**

Run:

```bash
pnpm exec vitest run src/layouts/AdminLayout.test.ts src/features/chat/components/ChatComposerBar.test.ts src/features/chat/components/ChatMessageStage.test.ts src/features/chat/chat-page.test.ts src/features/chat/composables/useChatPresentation.test.ts src/features/chat/composables/useChatThreadStream.test.ts
```

**Step 2: Run build verification**

Run:

```bash
pnpm build
```
