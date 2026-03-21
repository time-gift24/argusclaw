# Thread Management in Desktop Chat

## Why

Currently, the desktop chat has no way to manage threads. Each template+provider combination creates a single isolated session/thread stored only in memory — switching away loses the thread with no way to recover it. Users have no visibility into their conversation history and no ability to clean up stale threads. This limits the app's usability for long-term conversational workflows.

## What Changes

- **New Thread Sidebar**: Persistent left sidebar listing all threads (sessions), showing title and last updated time, with the active thread highlighted
- **Create Thread**: Button to start a new thread, which creates a fresh session with the current template and provider
- **Switch Thread**: Click any thread in the sidebar to restore it from DB into memory and resume the conversation
- **Delete Thread**: Delete button on each thread item to remove it from DB (and memory if loaded)
- **Rename Thread**: Inline rename of thread title (persisted to DB)
- **Auto Cleanup**: Background task that automatically deletes threads not updated in 14 days on app startup

## Capabilities

### New Capabilities

- `thread-management`: Desktop-side thread management including sidebar UI, thread CRUD operations exposed through Tauri commands, and automatic cleanup of old threads. Covers: thread listing, creation, deletion, renaming, switching, and 14-day auto-cleanup.

## Impact

### Backend (Rust)

- **New Tauri commands** (`commands.rs`): `list_sessions`, `delete_session`, `update_session_title`, `cleanup_old_sessions`
- **argus-session** (`manager.rs`): `cleanup_old_sessions()` method, `update_session_title()` method
- **argus-wing** (`lib.rs`): Expose cleanup API; call cleanup on startup

### Frontend (TypeScript/React)

- **New API bindings** (`lib/tauri.ts`): `threads.list()`, `threads.delete()`, `threads.updateTitle()`, `threads.cleanup()`
- **Store** (`lib/chat-store.ts`): New thread list state, switch/delete/rename/cleanup actions
- **New component** (`components/chat/thread-sidebar.tsx`): Sidebar listing all sessions/threads
- **Layout** (`app/page.tsx`): Integrate sidebar + chat area
- **New store** (`lib/thread-list-store.ts`): Standalone state for thread list management
