# Phase 5A Chat Service API Design

## Summary

Phase 5A starts chat service enablement from the server side only. It adds a narrow REST surface over `ServerCore` and `SessionManager` so `argus-server` can create and inspect chat sessions/threads without changing desktop, adding web chat UI, or introducing thread event streams.

## Scope

- Add server-owned REST endpoints for sessions, threads, messages, send, and cancel.
- Keep `argus-server` independent from `argus-wing`.
- Route handlers call only `ServerCore` narrow methods.
- Preserve existing management APIs and `apps/web` behavior.
- Do not add SSE/thread event streaming in this phase.
- Do not build chat UI in `apps/web`.

## Public API

Base path: `/api/v1/chat`.

- `GET /sessions`
- `POST /sessions`
- `DELETE /sessions/{session_id}`
- `GET /sessions/{session_id}/threads`
- `POST /sessions/{session_id}/threads`
- `DELETE /sessions/{session_id}/threads/{thread_id}`
- `GET /sessions/{session_id}/threads/{thread_id}/messages`
- `POST /sessions/{session_id}/threads/{thread_id}/messages`
- `POST /sessions/{session_id}/threads/{thread_id}/cancel`

Response bodies use existing shared types where practical:

- `SessionSummary`
- `ThreadSummary`
- `ChatMessage`
- `MutationResponse<T>`
- `DeleteResponse`

Create/send request bodies stay minimal and JSON-first:

- Create session: `{ "name": "..." }`
- Create thread: `{ "template_id": 1, "provider_id": "...", "model": "..." }`
- Send message: `{ "message": "..." }`

`provider_id` and `model` are optional on create thread. The existing `SessionManager` resolution rules remain authoritative.

## Architecture

`ServerCore` gains small methods that delegate to `SessionManager`. `routes::chat` owns path parsing, request DTOs, status codes, and `ApiError` mapping. `argus-server` does not reach around `ServerCore` to repositories or lower managers.

This phase intentionally avoids a shared web/desktop frontend core. The API is designed for future web chat, but no frontend chat state model is introduced yet.

## Error Handling

Route handlers map `ArgusError` through the existing API error envelope. Missing sessions/threads currently map to the existing internal-error status until the server error taxonomy is expanded in a later pass.

## Validation

- `cargo test -p argus-server -- --nocapture`
- `cargo tree -p argus-server | rg argus-wing` should have no matches.
- Existing web tests/build should keep passing because no web business logic changes are planned.
