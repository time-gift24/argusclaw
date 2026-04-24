# Phase 8 Web Chat Architecture Hardening

## Summary

Phase 8 keeps the Phase 5 chat API and Phase 7 runtime activity behavior unchanged, then hardens the independent `apps/web` chat page by separating orchestration, presentation mapping, message stage rendering, and runtime activity rendering.

## Scope

- Do not change desktop.
- Do not add or change REST/SSE contracts.
- Do not introduce shared frontend core.
- Keep TinyRobot for the chat surface and OpenTiny for management controls.
- Preserve Chinese labels and the current Phase 7 visual direction.

## Changes

- Move message-to-TinyRobot presentation mapping into `useChatPresentation`.
- Move the runtime activity card list into `ChatRuntimeActivityPanel`.
- Move the message timeline and starter prompts into `ChatMessageStage`.
- Move the chat article shell, actions, notices, runtime activity, and message stage composition into `ChatConversationPanel`.
- Keep `ChatPage` focused on API loading, session/thread orchestration, and wiring child components.

## Verification

- Add unit coverage for presentation mapping:
  - pending assistant bubble during streaming
  - empty assistant tool-call summary
  - starter prompt draft text mapping
- Keep existing ChatPage integration coverage green.
- Run:

```bash
cd apps/web && pnpm exec vitest run src/features/chat/composables/useChatPresentation.test.ts src/features/chat/chat-page.test.ts
cd apps/web && pnpm exec vitest run
cd apps/web && pnpm build
```
