# Phase 6 Web Chat Quality Hardening

## Summary

Phase 6 keeps the independent `apps/web` chat workspace and `argus-server` Phase 5 API boundary, then fixes the first real usability gaps found during manual testing.

## Scope

- Preserve desktop independence and do not introduce shared frontend core.
- Keep `argus-server` routes calling narrow `ServerCore` methods.
- Keep TinyRobot as the chat surface and OpenTiny as the management control layer.
- Do not add new chat concepts beyond session/thread/message usability fixes.

## Changes

- `POST /api/v1/chat/sessions/with-thread` accepts an optional `name`.
- `ServerCore::create_chat_session_with_thread` creates the session with a non-empty name, defaulting to `Web Chat` when older clients omit the field.
- `/chat` sends the current draft session name when first materializing a session + thread.
- Legacy sessions with blank names render as `会话 <id-prefix>` instead of an empty list item.
- Assistant messages with tool calls and empty text render a tool-call summary instead of the generic empty-message fallback.

## Verification

- Add server integration coverage for session names created through `sessions/with-thread`.
- Add ChatPage coverage for named first-send materialization, legacy blank session labels, and empty assistant tool-call display.
- Run `cargo test -p argus-server -- --nocapture`.
- Run `cd apps/web && pnpm exec vitest run`.
- Run `cd apps/web && pnpm build`.
