# Phase 5C Web Chat TinyRobot Design

## Summary

Phase 5A/5B completed the server-only chat REST surface. Phase 5C adds the first independent web chat page to `apps/web` using TinyRobot components while keeping desktop untouched.

## Goals

- Add a Chinese `/chat` route to the Vue management console.
- Use TinyRobot for the conversation surface: `TrBubbleList`, `TrSender`, and `TrPrompts`.
- Connect to existing `/api/v1/chat` REST endpoints for sessions, threads, messages, send, cancel, rename, snapshot, activation, and model binding.
- Reuse management console layout, tokens, OpenTiny controls, and `apps/web/DESIGN.md` visual rules.

## Non-Goals

- No desktop rewiring.
- No shared frontend core.
- No SSE or thread event stream for chat.
- No login or multi-user concepts.
- No local-only mock conversation engine as the primary path.

## User Experience

- A new sidebar item, "对话", opens the standalone chat page.
- The page shows sessions and threads in a compact left rail, with the active TinyRobot conversation on the right.
- Users can create a session, create a thread from available templates/providers, select an existing thread, send a message, cancel an active request, refresh messages, and rename/delete session or thread entries.
- Empty states guide users to configure providers/templates or start a new conversation.
- `TrPrompts` provides quick starter prompts only when a thread is active and no user input has been typed.

## API Mapping

- `GET /api/v1/chat/sessions`
- `POST /api/v1/chat/sessions`
- `PATCH /api/v1/chat/sessions/{session_id}`
- `DELETE /api/v1/chat/sessions/{session_id}`
- `GET /api/v1/chat/sessions/{session_id}/threads`
- `POST /api/v1/chat/sessions/{session_id}/threads`
- `GET /api/v1/chat/sessions/{session_id}/threads/{thread_id}`
- `PATCH /api/v1/chat/sessions/{session_id}/threads/{thread_id}`
- `PATCH /api/v1/chat/sessions/{session_id}/threads/{thread_id}/model`
- `POST /api/v1/chat/sessions/{session_id}/threads/{thread_id}/activate`
- `GET /api/v1/chat/sessions/{session_id}/threads/{thread_id}/messages`
- `POST /api/v1/chat/sessions/{session_id}/threads/{thread_id}/messages`
- `POST /api/v1/chat/sessions/{session_id}/threads/{thread_id}/cancel`

## State Model

- `apps/web` owns a small page-local state model.
- Server remains the source of truth for session/thread/message state.
- The frontend does not persist chat state in localStorage.
- After send/cancel/rename/delete, the frontend refreshes the affected server resources.

## Risk Notes

- Without chat SSE, assistant replies may require manual refresh or short post-send refreshes.
- Creating a usable thread requires at least one template and a resolvable provider/model binding.
- TinyRobot dependency compatibility must be verified through `pnpm install`, Vitest, and build.
