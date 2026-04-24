# Phase 7 Web Chat Runtime Activity

## Summary

Phase 7 keeps the independent `apps/web` chat workspace and adds visible runtime activity for streamed chat turns. The goal is to make Codex-style streaming more inspectable without changing server contracts or desktop behavior.

## Scope

- Do not change desktop.
- Do not add new chat REST/SSE endpoints.
- Keep TinyRobot as the message/composer surface.
- Keep REST snapshots and messages as the source of truth for completed conversation history.
- Use thread-event SSE only for live deltas and in-flight runtime activity.

## Changes

- `tool_started` displays a running tool card with argument preview.
- `tool_completed` updates the same card to completed or failed and shows result preview.
- `retry_attempt` displays a Chinese retry notice for the current turn.
- `turn_failed` displays a runtime failure notice while preserving the existing error path.
- The activity panel resets when switching session/thread, creating a draft, or sending a new message.

## Verification

- Add ChatPage coverage for streamed tool activity, retry status, and completed tool result.
- Run `cd apps/web && pnpm exec vitest run src/features/chat/chat-page.test.ts`.
- Run `cd apps/web && pnpm exec vitest run`.
- Run `cd apps/web && pnpm build`.
