## 1. Backend — SessionManager

- [x] 1.1 Add `cleanup_old_sessions(days: u32) -> Result<u64>` to `SessionManager` that deletes sessions where `updated_at < datetime('now', '-{days} days')` via direct SQL
- [x] 1.2 Add `update_session_title(session_id: SessionId, title: &str) -> Result<()>` to `SessionManager` that updates the session name in DB

## 2. Backend — argus-wing

- [x] 2.1 Add `cleanup_old_sessions(days: u32)` method to `ArgusWing` delegating to `SessionManager`
- [x] 2.2 Call `cleanup_old_sessions(14)` in `ArgusWing::init()` after pool setup and migration

## 3. Backend — Tauri Commands

- [x] 3.1 Add `list_sessions` Tauri command exposing `SessionSummary` list (id, name, thread_count, updated_at)
- [x] 3.2 Add `delete_session` Tauri command delegating to `SessionManager::delete`
- [x] 3.3 Add `update_session_title` Tauri command delegating to `ArgusWing::update_session_title`
- [x] 3.4 Add `cleanup_old_sessions` Tauri command delegating to `ArgusWing::cleanup_old_sessions`
- [x] 3.5 Register all new commands in the Tauri invoke handler in `lib.rs`

## 4. Frontend — TypeScript API Bindings

- [x] 4.1 Add `threads` namespace to `lib/tauri.ts` with `list()`, `delete()`, `updateTitle()`, `cleanup()` functions

## 5. Frontend — Thread List Store

- [x] 5.1 Create `lib/thread-list-store.ts` (Zustand store) with state: `sessions`, `isLoading`, `error`, `activeSessionId`
- [x] 5.2 Add `fetchSessions()` action to load session list on mount
- [x] 5.3 Add `deleteSession(id)` action to delete and refresh list
- [x] 5.4 Add `updateTitle(id, title)` action to rename and update local state
- [x] 5.5 Add `selectSession(id)` action to set active session
- [x] 5.6 Add `cleanup()` action to trigger backend cleanup and refresh

## 6. Frontend — ThreadSidebar Component

- [x] 6.1 Create `components/chat/thread-sidebar.tsx` using existing shadcn primitives (`collapsible`, `button`, `dropdown-menu`, `dialog`)
- [x] 6.2 Render "+ New Thread" button at top
- [x] 6.3 Render session list with title, relative time, active indicator, delete and rename buttons
- [x] 6.4 Render cleanup button at bottom
- [x] 6.5 Implement inline rename with input field and save/cancel on blur/Enter
- [x] 6.6 Implement delete confirmation dialog
- [x] 6.7 Wire up all actions to `useThreadListStore`

## 7. Frontend — Layout Integration

- [x] 7.1 Update `app/page.tsx` to use flex layout: sidebar (w-64) + chat area (flex-1)
- [x] 7.2 Update `ChatScreen` to accept `sessionKey` prop so parent can control which session is active
- [x] 7.3 Integrate sidebar with `useChatStore` for session activation and `useThreadListStore` for list management
- [x] 7.4 On mount: fetch session list, select most recent (or create new if empty)
