# thread-management — Desktop Thread Management

## ADDED Requirements

### Requirement: Thread list displays all sessions

The system SHALL display a list of all sessions (threads) in the sidebar, ordered by most recently updated first. Each entry SHALL show the session title and a relative time string (e.g., "2 hours ago").

#### Scenario: List shows all sessions on mount
- **WHEN** the sidebar component mounts
- **THEN** the system fetches all sessions via the `list_sessions` Tauri command and displays them ordered by `updated_at` descending

#### Scenario: Active session is highlighted
- **WHEN** a session is active (matches the current `activeSessionKey`)
- **THEN** its sidebar item is visually highlighted (distinct background color)

#### Scenario: Empty state
- **WHEN** there are no sessions
- **THEN** the sidebar shows a single empty state message: "No conversations yet"

### Requirement: User can create a new thread

The system SHALL allow the user to create a new thread by clicking a "+ New Thread" button. Creating a new thread creates a new session with the current template and provider, then activates it.

#### Scenario: Create thread with current template and provider
- **WHEN** user clicks "+ New Thread" while template T and provider P are selected
- **THEN** the system creates a new session via `create_chat_session(T, P)`, adds it to the thread list, and switches the active session to it

### Requirement: User can switch to a different thread

The system SHALL allow the user to click any thread in the sidebar to switch to it. Switching restores the thread from DB into memory and displays its message history.

#### Scenario: Switch to an existing thread
- **WHEN** user clicks a non-active thread in the sidebar
- **THEN** the system activates that session, loads its snapshot, and updates the chat view

#### Scenario: Switch triggers event subscription
- **WHEN** user switches to a thread
- **THEN** the system starts forwarding thread events for the newly active session

### Requirement: User can delete a thread

The system SHALL allow the user to delete a thread by clicking a delete button on the thread item. Deleting a thread removes it from DB and memory.

#### Scenario: Delete thread with confirmation
- **WHEN** user clicks the delete button on a thread item
- **THEN** the system shows a confirmation dialog
- **AND WHEN** user confirms
- **THEN** the system calls `delete_session` via Tauri, removes the thread from the list, and switches to the most recent remaining thread (or creates a new one if none remain)

#### Scenario: Delete last thread creates new one
- **WHEN** user deletes the last remaining thread
- **THEN** the system creates a new session and activates it

### Requirement: User can rename a thread

The system SHALL allow the user to rename a thread by clicking an edit/rename affordance on the thread item. The new name is persisted to the DB and reflected immediately in the list.

#### Scenario: Rename thread
- **WHEN** user clicks the rename button on a thread item
- **THEN** an inline text input appears with the current name
- **AND WHEN** user presses Enter or blurs the input with a non-empty value
- **THEN** the system calls `update_session_title` via Tauri, updates the display name, and exits edit mode

#### Scenario: Rename with empty value cancels
- **WHEN** user enters an empty name and confirms
- **THEN** the system cancels edit mode without saving

### Requirement: Old threads are automatically cleaned up

The system SHALL delete sessions (threads) that have not been updated in 14 or more days when the application starts.

#### Scenario: Cleanup removes threads older than 14 days
- **WHEN** the application starts
- **THEN** the system calls `cleanup_old_sessions(14)` which deletes all sessions where `updated_at < datetime('now', '-14 days')`
- **AND** deleted sessions' messages are cascade-deleted via the `ON DELETE CASCADE` foreign key constraint

### Requirement: Sidebar layout is always visible

The system SHALL display the thread sidebar alongside the chat area at all times, with a fixed width of 256px (w-64) and the chat area filling the remaining space.

#### Scenario: Layout renders sidebar and chat
- **WHEN** the chat page renders
- **THEN** the layout contains a fixed-width sidebar (w-64) and a flexible chat area (flex-1)
