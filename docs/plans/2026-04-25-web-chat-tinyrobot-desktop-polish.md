# Web Chat TinyRobot Desktop Polish Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Upgrade `apps/web` chat to a desktop-inspired TinyRobot experience with Markdown-rendered assistant content and visible reasoning blocks while keeping the existing REST/SSE pipeline.

**Architecture:** The implementation stays fully inside `apps/web` and preserves the current `argus-server` REST/SSE contract. We will extend the presentation mapping and TinyRobot rendering hooks so streamed and settled assistant messages can carry `reasoning_content`, then restyle the conversation shell and composer to visually align with the desktop chat.

**Tech Stack:** Vue 3, Vite, Vitest, `@opentiny/tiny-robot`, existing `apps/web` chat composables and components.

---

### Task 1: Lock reasoning and Markdown behavior with failing tests

**Files:**
- Modify: `apps/web/src/features/chat/composables/useChatPresentation.test.ts`
- Modify: `apps/web/src/features/chat/composables/useChatThreadStream.test.ts`
- Modify: `apps/web/src/features/chat/chat-page.test.ts`

**Step 1: Write the failing test**

- Add a presentation test asserting assistant messages preserve `reasoning_content` instead of collapsing it into placeholder-only text.
- Add a stream test asserting `reasoning_delta` contributes to pending assistant state that can be rendered.
- Add a page-level test asserting the chat page passes message data and rendering hooks needed for Markdown/reasoning.

**Step 2: Run test to verify it fails**

Run:

```bash
cd apps/web && pnpm exec vitest run src/features/chat/composables/useChatPresentation.test.ts src/features/chat/composables/useChatThreadStream.test.ts src/features/chat/chat-page.test.ts
```

Expected: FAIL on the newly added reasoning/Markdown assertions.

**Step 3: Write minimal implementation**

- Update only the smallest mapping/rendering code necessary to satisfy the new tests.

**Step 4: Run test to verify it passes**

Run the same command and expect the new tests to pass.

**Step 5: Commit**

```bash
git add apps/web/src/features/chat/composables/useChatPresentation.test.ts apps/web/src/features/chat/composables/useChatThreadStream.test.ts apps/web/src/features/chat/chat-page.test.ts
git commit -m "test: cover web chat reasoning and markdown rendering"
```

### Task 2: Carry reasoning through the TinyRobot presentation model

**Files:**
- Modify: `apps/web/src/features/chat/composables/useChatPresentation.ts`
- Modify: `apps/web/src/features/chat/ChatPage.vue`

**Step 1: Write the failing test**

- Add or refine tests to show the TinyRobot-facing message model includes:
  - visible assistant `content`
  - `reasoning_content` for settled assistant messages
  - `reasoning_content` for the pending assistant message during streaming

**Step 2: Run test to verify it fails**

Run:

```bash
cd apps/web && pnpm exec vitest run src/features/chat/composables/useChatPresentation.test.ts
```

Expected: FAIL because the current model only exposes `role` and `content`.

**Step 3: Write minimal implementation**

- Extend `ChatRobotMessage` so it can carry `reasoning_content`, `loading`, and any minimal state needed for TinyRobot reasoning rendering.
- Update `toRobotMessages` to preserve persisted `reasoning_content` and map pending reasoning to the pending assistant bubble.
- Keep the existing fallback copy only for truly empty assistant states.

**Step 4: Run test to verify it passes**

Run the same command and expect PASS.

**Step 5: Commit**

```bash
git add apps/web/src/features/chat/composables/useChatPresentation.ts apps/web/src/features/chat/ChatPage.vue apps/web/src/features/chat/composables/useChatPresentation.test.ts
git commit -m "feat: map reasoning into web chat robot messages"
```

### Task 3: Enable Markdown and reasoning rendering in the message stage

**Files:**
- Modify: `apps/web/src/features/chat/components/ChatMessageStage.vue`
- If needed, modify: `apps/web/src/features/chat/components/ChatConversationPanel.vue`
- Reference: `.agents/skills/tiny-robot-skill/components/bubble.md`

**Step 1: Write the failing test**

- Add a component or page assertion that assistant bubbles are rendered with the Markdown/reasoning-capable TinyRobot configuration rather than plain default text only.

**Step 2: Run test to verify it fails**

Run:

```bash
cd apps/web && pnpm exec vitest run src/features/chat/chat-page.test.ts
```

Expected: FAIL because the current stage does not expose the intended rendering configuration.

**Step 3: Write minimal implementation**

- Wire the TinyRobot stage to use Markdown rendering for assistant content.
- Ensure messages with `reasoning_content` render through TinyRobot reasoning support.
- Keep user/tool message handling stable.

**Step 4: Run test to verify it passes**

Run the same command and expect PASS.

**Step 5: Commit**

```bash
git add apps/web/src/features/chat/components/ChatMessageStage.vue apps/web/src/features/chat/components/ChatConversationPanel.vue apps/web/src/features/chat/chat-page.test.ts
git commit -m "feat: enable markdown and reasoning rendering in web chat"
```

### Task 4: Polish the conversation shell toward the desktop look

**Files:**
- Modify: `apps/web/src/features/chat/components/ChatConversationPanel.vue`
- Modify: `apps/web/src/features/chat/components/ChatComposerBar.vue`
- Modify: `apps/web/src/features/chat/components/ChatRuntimeActivityPanel.vue`
- Modify: `apps/web/src/features/chat/components/ChatMessageStage.vue`

**Step 1: Write the failing test**

- Add or update structural tests only where useful for stable visual hooks, such as class names or expected section presence.
- Prefer minimal tests that lock intended structure instead of brittle snapshot styling.

**Step 2: Run test to verify it fails**

Run:

```bash
cd apps/web && pnpm exec vitest run src/features/chat/chat-page.test.ts
```

Expected: FAIL on the newly asserted structural hooks.

**Step 3: Write minimal implementation**

- Apply desktop-inspired shell styling:
  - stronger message-stage atmosphere
  - cleaner header hierarchy
  - floating composer shell
  - more polished prompt and runtime-activity cards
- Preserve mobile behavior and current actions.

**Step 4: Run test to verify it passes**

Run the same command and expect PASS.

**Step 5: Commit**

```bash
git add apps/web/src/features/chat/components/ChatConversationPanel.vue apps/web/src/features/chat/components/ChatComposerBar.vue apps/web/src/features/chat/components/ChatRuntimeActivityPanel.vue apps/web/src/features/chat/components/ChatMessageStage.vue apps/web/src/features/chat/chat-page.test.ts
git commit -m "style: polish web chat toward desktop design"
```

### Task 5: Verify the full web chat slice

**Files:**
- Verify only

**Step 1: Run targeted tests**

Run:

```bash
cd apps/web && pnpm exec vitest run src/features/chat/composables/useChatPresentation.test.ts src/features/chat/composables/useChatThreadStream.test.ts src/features/chat/chat-page.test.ts
```

Expected: PASS.

**Step 2: Run broader feature tests if needed**

Run:

```bash
cd apps/web && pnpm exec vitest run
```

Expected: PASS or known unrelated failures explicitly called out.

**Step 3: Run production build**

Run:

```bash
cd apps/web && pnpm build
```

Expected: successful production build.

**Step 4: Commit verification-safe final state**

```bash
git add apps/web docs/plans/2026-04-25-web-chat-tinyrobot-desktop-polish-design.md docs/plans/2026-04-25-web-chat-tinyrobot-desktop-polish.md
git commit -m "feat: polish web chat reasoning markdown and desktop styling"
```
