# MCP Server Connection Status & Tools Display — Design Spec

## Overview

Display real-time MCP server connection status and available tools on the MCP server settings pages (list and edit). Status is held in-memory in `ArgusWing` and refreshed in the background.

## Architecture

### Data Flow

```
McpServerStatus (in-memory, ArgusWing)
    │
    ├── Background refresh task (every 30s for enabled servers)
    │
    └── Tauri Commands
            ├── list_mcp_server_statuses() → Record<i64, McpServerStatus>
            └── test_mcp_server(id: i64) → McpServerStatus

Frontend
    ├── Fetches statuses on mount
    ├── Polls every 30s for live updates
    └── Manual "Test" button triggers immediate refresh
```

### Backend

#### 1. In-Memory State (`argus-wing`)

Add to `ArgusWing`:
```rust
connection_states: Arc<RwLock<HashMap<i64, McpServerStatus>>>
```

#### 2. Background Refresh Task

- Runs on `ArgusWing` init
- Every 30 seconds, iterates enabled servers from DB and tests each one
- Updates `connection_states` with results
- `McpClientRuntime` already exists and can be reused for testing

#### 3. New Tauri Commands

| Command | Signature | Description |
|---------|-----------|-------------|
| `list_mcp_server_statuses` | `() → Record<i64, McpServerStatus>` | Returns all cached connection statuses |
| `test_mcp_server` | `(id: i64) → McpServerStatus` | Triggers fresh connection test for one server |

### Frontend

#### 1. Type Definitions (`lib/tauri.ts`)

```typescript
type McpServerStatus =
  | { status: "disconnected" }
  | { status: "connecting" }
  | { status: "connected"; tools: string[]; connected_at: string }
  | { status: "failed"; error: string; failed_at: string }

export const mcpServers = {
  // ...existing...
  getStatuses: () => invoke<Record<number, McpServerStatus>>("list_mcp_server_statuses"),
  testServer: (id: number) => invoke<McpServerStatus>("test_mcp_server", { id }),
}
```

#### 2. List Page (`/settings/mcp/page.tsx`)

**Changes per card:**
- Add status badge (colored per status, see below)
- If `status === "failed"`: show error message below badge
- If `status === "connected"`: show tool pills in a wrapping row below server info
- Add "Test" button that calls `testServer(id)` and refreshes local state

**Status badge styles:**

| Status | Style |
|--------|-------|
| `connected` | Green badge: `bg-green-100 text-green-800` |
| `connecting` | Yellow badge: `bg-yellow-100 text-yellow-800` |
| `disconnected` | Gray badge: `bg-gray-100 text-gray-600` |
| `failed` | Red badge: `bg-red-100 text-red-800` |

**Tool pills:** Small rounded badges (`bg-muted text-muted-foreground`), wrapping row, max-height with scroll if many tools.

#### 3. Edit Page (`/settings/mcp/[id]/page.tsx`)

**Two-column layout:**
- Left (60%): existing form
- Right (40%): connection status card

**Right panel:**
- Same status badge + error message + tool pills as list page
- "Refresh" button to trigger manual re-test
- Auto-tests on mount (shows "Connecting..." initially)

## Files to Change

### Backend
| File | Change |
|------|--------|
| `crates/argus-wing/src/lib.rs` | Add `connection_states` field + background refresh |
| `crates/argus-tool/src/mcp/client.rs` | Already exists, may need minor refactor for testability |
| `crates/desktop/src-tauri/src/commands.rs` | Add `list_mcp_server_statuses` and `test_mcp_server` commands |

### Frontend
| File | Change |
|------|--------|
| `crates/desktop/lib/tauri.ts` | Add `getStatuses`, `testServer`, `McpServerStatus` type |
| `crates/desktop/app/settings/mcp/page.tsx` | Add status badge, error, tool pills, test button per card |
| `crates/desktop/app/settings/mcp/[id]/page.tsx` | Two-column layout with status panel on right |
| `crates/desktop/app/settings/mcp/new/page.tsx` | No changes needed |

## Constraints

- Status is **never persisted** to the database — it lives only in memory
- If no status is cached for a server, treat as `disconnected`
- Background refresh skips servers that are disabled
- Background refresh runs independently of frontend polling
- Tool pills truncate with "+N more" if more than 10 tools (to avoid card overflow)
