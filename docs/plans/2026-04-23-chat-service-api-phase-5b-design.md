# Phase 5B Chat Service API Design

## Summary

Phase 5A made the first server-only chat REST surface available. Phase 5B completes the server service boundary needed by a future web chat client without adding that client yet.

## Scope

- Keep `argus-server` independent from `argus-wing`.
- Keep route handlers limited to `ServerCore` narrow methods.
- Continue using existing `SessionManager` behavior instead of duplicating session/thread logic.
- Add missing service operations that already exist in `SessionManager`: rename session, rename thread, update thread model, fetch thread snapshot, and activate an existing thread binding.
- Improve chat API request/lookup errors to use the existing JSON envelope with `400` and `404` where appropriate.

## Non-goals

- No web chat UI.
- No desktop rewiring.
- No thread event SSE.
- No new shared frontend core.
- No lower-level repository access from chat routes.

## Public API Additions

Base path remains `/api/v1/chat`.

- `PATCH /sessions/{session_id}` with `{ "name": "..." }`
- `GET /sessions/{session_id}/threads/{thread_id}`
- `PATCH /sessions/{session_id}/threads/{thread_id}` with `{ "title": "..." }`
- `PATCH /sessions/{session_id}/threads/{thread_id}/model` with `{ "provider_id": 1, "model": "..." }`
- `POST /sessions/{session_id}/threads/{thread_id}/activate`

Thread snapshot responses include:

- `session_id`
- `thread_id`
- `messages`
- `turn_count`
- `token_count`
- `plan_item_count`

Thread binding responses include:

- `session_id`
- `thread_id`
- `template_id`
- `effective_provider_id`
- `effective_model`

## Error Handling

`ApiError` gains structured variants while keeping the current envelope shape:

- `bad_request` -> `400`
- `not_found` -> `404`
- `internal_error` -> `500`

`ArgusError::SessionNotFound`, `ThreadNotFound`, `TemplateNotFound`, and `ProviderNotFound` map to `404`. Invalid path IDs and empty required fields map to `400`.

## Testing

Add integration coverage in `crates/argus-server/tests/chat_api.rs` for:

- session rename round-trip
- thread rename round-trip
- thread model update and activate response
- thread snapshot response
- invalid path IDs return `400`
- unknown session/thread returns `404`

Run existing server and web verification to confirm the server-only work does not regress the management console.
