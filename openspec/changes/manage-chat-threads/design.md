## Context

Desktop chat currently has no thread management layer. The `ChatStore` holds multiple sessions in memory keyed by `templateId::providerId`, but:
- There's no UI to see or switch between them
- Switching template/provider creates a new session, discarding the old one (only persisted to DB but not restored)
- `SessionManager` already has `list_sessions()`, `delete()`, and `create()` methods in the backend
- SQLite schema already stores sessions and threads with `updated_at` timestamps
- The `ThreadRepository` trait already has `list_threads()` and `delete_thread()`

The frontend uses assistant-ui's `ThreadPrimitive` for chat rendering, embedded in `ChatScreen`. Layout is currently single-column full-screen.

## Goals / Non-Goals

**Goals:**
- Provide a persistent sidebar listing all sessions (threads), updated in real-time
- Allow creating new threads, switching between them, deleting, and renaming
- Automatically clean up threads inactive for 14 days on app startup
- Keep the chat experience unchanged for the active thread

**Non-Goals:**
- Multi-device sync or server-side storage
- Thread search/filtering beyond list view
- Session-level organization (tags, folders)
- Persisting message history across restarts (future work)
- Changing the underlying session:thread relationship (remains 1:1)

## Decisions

### 1. Use a dedicated Zustand store for thread list

**Decision:** Create a separate `useThreadListStore` for managing the global thread list, independent of `useChatStore`.

**Rationale:** `useChatStore` is tightly coupled to the active chat runtime and message streaming. The thread list is a separate concern — listing sessions, tracking which one is active, and managing deletions. Mixing them creates unnecessary coupling.

**Alternative:** Extend `useChatStore` to include thread list state. Rejected — it already has 15+ responsibilities.

### 2. Thread list store talks directly to Tauri commands

**Decision:** The `useThreadListStore` calls Tauri commands (`list_sessions`, `delete_session`, etc.) directly, without going through `useChatStore`.

**Rationale:** Keeps the stores decoupled. The thread list doesn't need to know about chat runtime state, and vice versa.

### 3. Backend adds `update_session_title` to SessionManager

**Decision:** Add `update_session_title(session_id, title)` method to `SessionManager` that updates the session name in the DB.

**Rationale:** Currently sessions are named `Chat-{template_id}` at creation. Users should be able to rename them for easier identification. The DB already has a `name` column on `sessions`.

### 4. Auto cleanup on startup, no background task

**Decision:** Call `cleanup_old_sessions(14)` once during `ArgusWing::init()`, not as a recurring background task.

**Rationale:** Simpler — no tokio interval management, no shutdown handling. App startup is a natural sync point. For a desktop app, this is sufficient.

### 5. Layout: Sidebar + Chat screen in a flex container

**Decision:** Update `app/page.tsx` to use a two-column layout: fixed-width sidebar (w-64, ~256px) on the left, existing `ChatScreen` on the right.

```
┌──────────────────┬─────────────────────────────────┐
│  ThreadSidebar   │         ChatScreen              │
│  (w-64, fixed)  │  (flex-1, min-w-0)            │
│                  │                                  │
│  [+ New Thread]  │  AssistantRuntimeProvider         │
│  ───────────     │    └── ThreadPrimitive           │
│  Thread 1 ★      │         ├── PlanPanel            │
│  Thread 2        │         ├── Messages            │
│  Thread 3        │         └── Composer            │
│  ...             │                                  │
│  ───────────     │                                  │
│  🧹 Cleanup      │                                  │
└──────────────────┴─────────────────────────────────┘
```

**Rationale:** Sidebar is always visible for quick context switching. Width matches Tailwind's standard sidebar widths (`w-64`).

### 6. Thread list refresh on mount and after mutations

**Decision:** Fetch `list_sessions()` when the component mounts, and after create/delete/rename operations.

**Rationale:** No WebSocket or polling — just simple request-response. Acceptable for desktop app with manual interactions.

## Risks / Trade-offs

- **[Risk] Switching thread loses unpersisted messages**: When switching away from an active thread, message history exists only in memory. If the app crashes before persistence, messages are lost.
  - **Mitigation**: Accept as known limitation (persistence is future work). Not blocking for this change.

- **[Risk] Large thread list**: If users accumulate many sessions, the sidebar could become unwieldy.
  - **Mitigation**: Auto-cleanup at 14 days limits growth. No pagination needed at this scale.

- **[Risk] Thread title rename conflicts**: Two threads could end up with the same name.
  - **Mitigation**: Accept — no uniqueness constraint on session names. Names are purely for human readability.

- **[Trade-off] Session vs Thread naming**: Backend uses "session" but user-facing concept is "thread/conversation". Frontend calls it "thread" for UX clarity; backend API uses "session".
  - **Mitigation**: Document the naming in code comments. Frontend types use `ThreadSummary` aliased from backend.

## Open Questions

- Should the active thread auto-select the most recently updated one on first load, or require explicit selection?
  - **Resolved**: Select the most recently updated session on first load (fallback to creating a new one if none exist).

- Does the sidebar need a collapse/expand toggle?
  - **Resolved**: No — per user decision, always expanded.

- Should cleanup be triggered manually as well as on startup?
  - **Resolved**: Just on startup for simplicity. Can add manual trigger later if needed.
