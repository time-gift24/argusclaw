# MCP Connection Status & Tools Display — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Display real-time MCP server connection status and available tools on the settings list and edit pages. Status lives in-memory in `ArgusWing`, refreshed automatically in the background.

**Architecture:** `ArgusWing` holds an in-memory `HashMap<i64, McpServerStatus>`. A background task runs every 30s, testing all enabled servers. Tauri commands expose status to the frontend, which polls every 30s.

**Tech Stack:** Rust (argus-wing, argus-tool, argus-protocol), TypeScript/React (Next.js), Tailwind CSS, Tauri 2.0

---

## Chunk 1: Backend — In-Memory State & Background Refresh

### Files

- Modify: `crates/argus-wing/src/lib.rs`
- Modify: `crates/desktop/src-tauri/src/commands.rs`
- Modify: `crates/argus-protocol/src/mcp.rs` (verify `McpServerStatus` is serde-serializable)

### Steps

- [ ] **Step 1: Verify `McpServerStatus` serialization**

Check that `McpServerStatus` in `crates/argus-protocol/src/mcp.rs` has `#[derive(Serialize, Deserialize)]`. It should already be there from the existing definition.

Run: `grep -A5 "pub enum McpServerStatus" crates/argus-protocol/src/mcp.rs`

- [ ] **Step 2: Add in-memory state to `ArgusWing`**

In `crates/argus-wing/src/lib.rs`, add a new field to `ArgusWing`:

```rust
use std::collections::HashMap;
use argus_protocol::mcp::McpServerStatus;
use tokio::sync::RwLock;

// In struct ArgusWing:
mcp_connection_states: Arc<RwLock<HashMap<i64, McpServerStatus>>>,
```

Add import at top of lib.rs:
```rust
use std::collections::HashMap;
use argus_protocol::mcp::McpServerStatus;
use tokio::sync::RwLock;
```

In `ArgusWing::init()` body, after the existing fields are initialized:
```rust
let mcp_connection_states = Arc::new(RwLock::new(HashMap::new()));
```

Add to the returned struct:
```rust
Ok(Arc::new(Self {
    // ... existing fields ...
    mcp_connection_states,
    account_manager,
    credential_store,
}))
```

Also update `ArgusWing::with_pool()` similarly — add the field and initialize it.

- [ ] **Step 3: Add getter for connection states**

Add a public method to `ArgusWing`:

```rust
/// Get all cached MCP server connection statuses.
pub async fn list_mcp_connection_states(
    &self,
) -> HashMap<i64, McpServerStatus> {
    self.mcp_connection_states.read().await.clone()
}
```

- [ ] **Step 4: Add `test_mcp_connection` method**

Add to `ArgusWing`:

```rust
use argus_tool::mcp::McpClientRuntime;

/// Test connection to an MCP server and return its status.
pub async fn test_mcp_connection(
    &self,
    server_id: i64,
) -> Result<McpServerStatus> {
    use argus_repository::traits::McpServerRepository;

    let config = self.mcp_repository.get(server_id).await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?
        .ok_or_else(|| ArgusError::DatabaseError {
            reason: format!("MCP server {} not found", server_id),
        })?;

    if !config.enabled {
        return Ok(McpServerStatus::Disconnected);
    }

    // Set status to Connecting
    {
        let mut states = self.mcp_connection_states.write().await;
        states.insert(server_id, McpServerStatus::Connecting);
    }

    // Attempt connection
    let result = match McpClientRuntime::new(&config).await {
        Ok(client) => {
            match client.list_tools().await {
                Ok(tools) => {
                    let tool_names: Vec<String> = tools.into_iter().map(|t| t.name).collect();
                    Ok(McpServerStatus::Connected {
                        tools: tool_names,
                        connected_at: chrono::Utc::now(),
                    })
                }
                Err(e) => Ok(McpServerStatus::Failed {
                    error: e.to_string(),
                    failed_at: chrono::Utc::now(),
                }),
            }
        }
        Err(e) => Ok(McpServerStatus::Failed {
            error: e.to_string(),
            failed_at: chrono::Utc::now(),
        }),
    };

    // Cache the result
    {
        let mut states = self.mcp_connection_states.write().await;
        match &result {
            Ok(status) => states.insert(server_id, status.clone()),
            Err(_) => {} // shouldn't happen, all paths return Ok
        }
    }

    result
}
```

Note: The return type `Result<McpServerStatus>` uses the type alias `pub type Result<T> = std::result::Result<T, ArgusError>` from `argus-protocol/src/lib.rs`. Use `ArgusError` for all error cases. The "not found" case uses `ArgusError::DatabaseError` with a descriptive reason string.

- [ ] **Step 5: Add background refresh task**

Add a `start_mcp_connection_monitor` method to `ArgusWing`:

```rust
/// Start the background MCP connection monitor.
/// This task periodically tests all enabled MCP servers and updates their cached statuses.
pub fn start_mcp_connection_monitor(self: &Arc<Self>) {
    let wing = Arc::clone(self);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;

            let servers = match wing.list_mcp_servers().await {
                Ok(servers) => servers,
                Err(e) => {
                    tracing::warn!("MCP monitor: failed to list servers: {}", e);
                    continue;
                }
            };

            for server in servers {
                if !server.enabled {
                    continue;
                }
                if let Err(e) = wing.test_mcp_connection(server.id).await {
                    tracing::warn!(
                        "MCP monitor: failed to test server {}: {}",
                        server.id,
                        e
                    );
                }
            }
        }
    });
}
```

Call this at the end of `ArgusWing::init()`:
```rust
wing_clone.start_mcp_connection_monitor();
```

- [ ] **Step 6: Add Tauri commands**

In `crates/desktop/src-tauri/src/commands.rs`, add after the existing MCP commands (around line 581):

```rust
#[tauri::command]
pub async fn list_mcp_server_statuses(
    wing: State<'_, Arc<ArgusWing>>,
) -> Result<std::collections::HashMap<i64, argus_protocol::mcp::McpServerStatus>, String> {
    Ok(wing.list_mcp_connection_states().await)
}

#[tauri::command]
pub async fn test_mcp_server(
    wing: State<'_, Arc<ArgusWing>>,
    id: i64,
) -> Result<argus_protocol::mcp::McpServerStatus, String> {
    wing.test_mcp_connection(id)
        .await
        .map_err(|e| e.to_string())
}
```

- [ ] **Step 7: Register commands in Tauri app**

Check `crates/desktop/src-tauri/src/lib.rs` and ensure both new commands are registered in the builder. Look for existing MCP commands registration and add the new ones alongside.

- [ ] **Step 8: Verify backend builds**

Run: `cargo build -p argus-wing -p desktop 2>&1 | tail -20`

Expected: Both crates compile successfully with no errors.

- [ ] **Step 9: Commit**

```bash
git add crates/argus-wing/src/lib.rs crates/desktop/src-tauri/src/commands.rs crates/desktop/src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(backend): add MCP connection status state and background monitor

- Add in-memory HashMap<i64, McpServerStatus> to ArgusWing
- Add background task testing enabled servers every 30s
- Add list_mcp_server_statuses and test_mcp_server Tauri commands

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Chunk 2: Shared — MCP Status Utilities

### Files

- Create: `crates/desktop/lib/mcp-status.ts`

### Steps

- [ ] **Step 1: Create shared utility**

Create `crates/desktop/lib/mcp-status.ts`:

```typescript
import type { McpServerStatus } from "./tauri";

export const STATUS_COLORS: Record<string, string> = {
  connected: "bg-green-100 text-green-800",
  connecting: "bg-yellow-100 text-yellow-800",
  disconnected: "bg-gray-100 text-gray-600",
  failed: "bg-red-100 text-red-800",
};

export const STATUS_LABELS: Record<string, string> = {
  connected: "已连接",
  connecting: "连接中",
  disconnected: "未连接",
  failed: "连接失败",
};

export function getStatusColor(status: McpServerStatus): string {
  return STATUS_COLORS[status.status] ?? "bg-gray-100 text-gray-600";
}

export function getStatusLabel(status: McpServerStatus): string {
  return STATUS_LABELS[status.status] ?? "未知";
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/desktop/lib/mcp-status.ts
git commit -m "$(cat <<'EOF'
feat(frontend): add shared MCP status utility

- Extract statusColors and statusLabels to lib/mcp-status.ts
- Provides getStatusColor and getStatusLabel helpers

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Chunk 3: Frontend — TypeScript API & List Page

### Files

- Modify: `crates/desktop/lib/tauri.ts`
- Modify: `crates/desktop/app/settings/mcp/page.tsx`

### Steps

- [ ] **Step 1: Add TypeScript types and API calls**

In `crates/desktop/lib/tauri.ts`, add after the existing `McpServerConfig` interface and `mcpServers` export:

```typescript
// MCP Server Status
export type McpServerStatus =
  | { status: "disconnected" }
  | { status: "connecting" }
  | { status: "connected"; tools: string[]; connected_at: string }
  | { status: "failed"; error: string; failed_at: string };

export const mcpServers = {
  // ...existing methods (list, get, upsert, delete)...

  getStatuses: () =>
    invoke<Record<number, McpServerStatus>>("list_mcp_server_statuses"),

  testServer: (id: number) =>
    invoke<McpServerStatus>("test_mcp_server", { id }),
};
```

- [ ] **Step 2: Read the current list page**

Full file: `crates/desktop/app/settings/mcp/page.tsx`

- [ ] **Step 3: Add status badge, tools, and test button to cards**

In `page.tsx`, add these imports at the top:
```typescript
import { mcpServers, type McpServerStatus } from "@/lib/tauri";
import { STATUS_COLORS } from "@/lib/mcp-status";
```

Add state and polling near the top of the component:
```typescript
const [statuses, setStatuses] = useState<Record<number, McpServerStatus>>({});

useEffect(() => {
  async function loadStatuses() {
    try {
      const s = await mcpServers.getStatuses();
      setStatuses(s);
    } catch (e) {
      console.error("Failed to load MCP statuses", e);
    }
  }
  loadStatuses();
  const interval = setInterval(loadStatuses, 30000);
  return () => clearInterval(interval);
}, []);
```

Add `handleTest`:
```typescript
async function handleTest(id: number) {
  try {
    const status = await mcpServers.testServer(id);
    setStatuses((prev) => ({ ...prev, [id]: status }));
  } catch (e) {
    console.error("Test failed", e);
  }
}
```

In each card's JSX, add after the existing content (e.g., after the server type badge):

```tsx
{/* Connection Status */}
{(() => {
  const s = statuses[server.id];
  if (!s) return null;
  return (
    <div className="mt-3 space-y-2">
      <div className="flex items-center gap-2 flex-wrap">
        <span className={`text-xs px-2 py-0.5 rounded-full font-medium ${STATUS_COLORS[s.status]}`}>
          {s.status === "connected" ? "已连接" :
           s.status === "connecting" ? "连接中" :
           s.status === "disconnected" ? "未连接" :
           "连接失败"}
        </span>
        <button
          onClick={() => void handleTest(server.id)}
          className="text-xs px-2 py-0.5 rounded border hover:bg-muted"
        >
          测试
        </button>
      </div>

      {s.status === "failed" && (
        <p className="text-xs text-red-600">{s.error}</p>
      )}

      {s.status === "connected" && s.tools.length > 0 && (
        <div className="flex flex-wrap gap-1 mt-1">
          {s.tools.slice(0, 10).map((tool) => (
            <span
              key={tool}
              className="bg-muted text-muted-foreground text-xs px-2 py-0.5 rounded-full"
            >
              {tool}
            </span>
          ))}
          {s.tools.length > 10 && (
            <span className="text-xs text-muted-foreground px-1 py-0.5">
              +{s.tools.length - 10} 更多
            </span>
          )}
        </div>
      )}
    </div>
  );
})()}
```

- [ ] **Step 4: Verify the list page builds**

Run: `cd crates/desktop && pnpm build 2>&1 | tail -20`

Expected: No TypeScript errors.

- [ ] **Step 5: Commit**

```bash
git add crates/desktop/lib/tauri.ts crates/desktop/app/settings/mcp/page.tsx
git commit -m "$(cat <<'EOF'
feat(frontend): add MCP connection status and tools to list page

- Add McpServerStatus type and getStatuses/testServer APIs
- Cards show colored status badge, error message, tool pills
- Polling every 30s + manual Test button per card

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Chunk 4: Frontend — Edit Page Status Panel

### Files

- Create: `crates/desktop/components/settings/mcp-status-panel.tsx`
- Modify: `crates/desktop/components/settings/mcp-server-editor.tsx`
- Modify: `crates/desktop/components/settings/index.ts`
- Modify: `crates/desktop/app/settings/mcp/[id]/page.tsx`

### Context

`McpServerEditor` already has a `grid grid-cols-2 gap-6` layout with the form on the left and an info panel ("服务器类型说明") on the right. For the edit page, replace the info panel with `McpStatusPanel`. For the new page, keep the info panel.

**Integration strategy**: Modify `McpServerEditor` to accept an optional `rightPanel` React node prop. When `serverId` is provided (editing), pass `McpStatusPanel` as `rightPanel`. When creating a new server, `rightPanel` is omitted and the info panel renders.

### Steps

- [ ] **Step 1: Read `mcp-server-editor.tsx`**

Full file: `crates/desktop/components/settings/mcp-server-editor.tsx` — focus on the `grid grid-cols-2` section (lines 149-348)

- [ ] **Step 2: Add `rightPanel` prop to `McpServerEditor`**

Update the interface:
```typescript
interface McpServerEditorProps {
  serverId?: number;
  rightPanel?: React.ReactNode;
}
```

Update the destructuring:
```typescript
export function McpServerEditor({ serverId, rightPanel }: McpServerEditorProps) {
```

In the JSX, replace the right column (lines 326-348+) with:
```tsx
{/* Right: Info or Status Panel */}
<div className="space-y-4">
  {rightPanel ?? (
    <div className="rounded-lg border bg-muted/30 p-4">
      {/* ... existing info panel content ... */}
    </div>
  )}
</div>
```

- [ ] **Step 3: Create `McpStatusPanel` component**

Create: `crates/desktop/components/settings/mcp-status-panel.tsx`

```tsx
"use client";

import { useEffect, useState } from "react";
import { mcpServers, type McpServerStatus } from "@/lib/tauri";
import { STATUS_COLORS } from "@/lib/mcp-status";

interface McpStatusPanelProps {
  serverId: number;
}

export function McpStatusPanel({ serverId }: McpStatusPanelProps) {
  const [status, setStatus] = useState<McpServerStatus>({
    status: "disconnected",
  });
  const [loading, setLoading] = useState(false);

  async function loadStatus() {
    try {
      const all = await mcpServers.getStatuses();
      setStatus(all[serverId] ?? { status: "disconnected" });
    } catch {
      // keep current status on error
    }
  }

  async function handleRefresh() {
    setLoading(true);
    try {
      const s = await mcpServers.testServer(serverId);
      setStatus(s);
    } catch (e) {
      console.error("Test failed", e);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    loadStatus();
    const interval = setInterval(loadStatus, 30000);
    return () => clearInterval(interval);
  }, [serverId]);

  const tools = status.status === "connected" ? status.tools : [];
  const showTools = tools.length > 0;

  return (
    <div className="rounded-lg border p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">连接状态</h3>
        <button
          onClick={() => void handleRefresh()}
          disabled={loading}
          className="text-xs px-2 py-1 rounded border hover:bg-muted disabled:opacity-50"
        >
          {loading ? "测试中..." : "刷新"}
        </button>
      </div>

      <span
        className={`inline-block text-xs px-2 py-0.5 rounded-full font-medium ${STATUS_COLORS[status.status]}`}
      >
        {status.status === "connected" ? "已连接" :
         status.status === "connecting" ? "连接中" :
         status.status === "disconnected" ? "未连接" :
         "连接失败"}
      </span>

      {status.status === "failed" && (
        <p className="text-xs text-red-600 mt-1">{status.error}</p>
      )}

      {showTools && (
        <div className="space-y-2">
          <p className="text-xs text-muted-foreground">
            可用工具 ({tools.length})
          </p>
          <div className="flex flex-wrap gap-1 max-h-40 overflow-y-auto">
            {tools.slice(0, 10).map((tool) => (
              <span
                key={tool}
                className="bg-muted text-muted-foreground text-xs px-2 py-0.5 rounded-full"
              >
                {tool}
              </span>
            ))}
            {tools.length > 10 && (
              <span className="text-xs text-muted-foreground px-1 py-0.5">
                +{tools.length - 10} 更多
              </span>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 4: Export from index.ts**

In `crates/desktop/components/settings/index.ts`, add:
```typescript
export { McpStatusPanel } from "./mcp-status-panel"
```

- [ ] **Step 5: Update edit page to pass status panel**

In `crates/desktop/app/settings/mcp/[id]/page.tsx`, update to import and pass `McpStatusPanel`:

```typescript
import { McpServerEditor, McpStatusPanel } from "@/components/settings";

export default async function EditMcpServerPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  return <McpServerEditor serverId={parseInt(id)} rightPanel={<McpStatusPanel serverId={parseInt(id)} />} />;
}
```

- [ ] **Step 6: Verify the edit page builds**

Run: `cd crates/desktop && pnpm build 2>&1 | tail -20`

Expected: No TypeScript errors.

- [ ] **Step 7: Commit**

```bash
git add \
  crates/desktop/components/settings/mcp-server-editor.tsx \
  crates/desktop/components/settings/mcp-status-panel.tsx \
  crates/desktop/components/settings/index.ts \
  crates/desktop/app/settings/mcp/[id]/page.tsx
git commit -m "$(cat <<'EOF'
feat(frontend): add status panel to MCP edit page

- Modify McpServerEditor to accept optional rightPanel prop
- Add McpStatusPanel with status badge, error, and tools
- Auto-tests on mount with 30s polling + manual refresh button

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```
