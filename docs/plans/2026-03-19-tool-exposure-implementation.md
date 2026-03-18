# Tool Exposure Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Expose tool list from argus-wing to desktop frontend, display in tools page, and enable tool selection in agent editor.

**Architecture:** Add `list_tools()` method to `ArgusWing`, create Tauri command to expose it, add TypeScript API, create tools settings page, and add tool selection checkboxes to agent editor.

**Tech Stack:** Rust (argus-wing), Tauri commands, TypeScript/React (desktop frontend)

---

## Task 1: Add ToolInfo struct and list_tools() to ArgusWing

**Files:**
- Modify: `crates/argus-wing/src/lib.rs:185-189` (add method)

**Step 1: Add ToolInfo struct and list_tools method**

```rust
/// Tool information for frontend display.
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub risk_level: argus_protocol::RiskLevel,
    pub parameters: serde_json::Value,
}

impl ArgusWing {
    // ... existing methods ...

    /// List all available tools with their metadata.
    pub async fn list_tools(&self) -> Vec<ToolInfo> {
        let definitions = self.tool_manager.list_definitions();
        definitions
            .into_iter()
            .map(|def| ToolInfo {
                name: def.name.clone(),
                description: def.description.clone(),
                risk_level: self.tool_manager.get_risk_level(&def.name),
                parameters: def.parameters,
            })
            .collect()
    }
}
```

**Step 2: Verify compilation**

Run: `cargo check -p argus-wing`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/argus-wing/src/lib.rs
git commit -m "feat(argus-wing): add list_tools method and ToolInfo struct"
```

---

## Task 2: Add list_tools Tauri command

**Files:**
- Modify: `crates/desktop/src-tauri/src/commands.rs` (add command)
- Modify: `crates/desktop/src-tauri/src/lib.rs` (register command)

**Step 1: Add command to commands.rs**

Add after Agent Template Commands section (~line 175):

```rust
// ============================================================================
// Tool Commands
// ============================================================================

#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolInfoPayload {
    pub name: String,
    pub description: String,
    pub risk_level: String,
    pub parameters: serde_json::Value,
}

#[tauri::command]
pub async fn list_tools(wing: State<'_, Arc<ArgusWing>>) -> Result<Vec<ToolInfoPayload>, String> {
    let tools = wing.list_tools().await.map_err(|e| e.to_string())?;
    Ok(tools
        .into_iter()
        .map(|t| ToolInfoPayload {
            name: t.name,
            description: t.description,
            risk_level: format!("{:?}", t.risk_level).to_lowercase(),
            parameters: t.parameters,
        })
        .collect())
}
```

**Step 2: Register command in lib.rs**

Check `crates/desktop/src-tauri/src/lib.rs` and add `list_tools` to the command module registration.

**Step 3: Verify compilation**

Run: `cargo check -p desktop`
Expected: PASS

**Step 4: Commit**

```bash
git add crates/desktop/src-tauri/src/commands.rs crates/desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): add list_tools Tauri command"
```

---

## Task 3: Add TypeScript API for tools

**Files:**
- Modify: `crates/desktop/lib/tauri.ts`

**Step 1: Add ToolInfo interface and tools API**

Add after `ProviderTestResult` interface (~line 63):

```typescript
export interface ToolInfo {
  name: string;
  description: string;
  risk_level: "low" | "medium" | "high" | "critical";
  parameters: Record<string, unknown>;
}
```

Add after the `agents` export (~line 112):

```typescript
// Tools API
export const tools = {
  list: () => invoke<ToolInfo[]>("list_tools"),
};
```

**Step 2: Verify TypeScript compilation**

Run: `cd crates/desktop && pnpm tsc --noEmit`
Expected: PASS (or type errors if any)

**Step 3: Commit**

```bash
git add crates/desktop/lib/tauri.ts
git commit -m "feat(desktop): add ToolInfo type and tools.list API"
```

---

## Task 4: Create ToolCard component

**Files:**
- Create: `crates/desktop/components/settings/tool-card.tsx`

**Step 1: Create tool-card.tsx**

```tsx
"use client"

import * as React from "react"
import { type ToolInfo } from "@/lib/tauri"
import { Card, CardContent, CardHeader } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"

const riskColors: Record<ToolInfo["risk_level"], string> = {
  low: "bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-300",
  medium: "bg-yellow-100 text-yellow-800 dark:bg-yellow-900 dark:text-yellow-300",
  high: "bg-orange-100 text-orange-800 dark:bg-orange-900 dark:text-orange-300",
  critical: "bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-300",
}

interface ToolCardProps {
  tool: ToolInfo
}

export function ToolCard({ tool }: ToolCardProps) {
  const [showParams, setShowParams] = React.useState(false)

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-start justify-between">
          <div>
            <h3 className="text-sm font-semibold">{tool.name}</h3>
            <p className="text-xs text-muted-foreground mt-1">{tool.description}</p>
          </div>
          <Badge className={riskColors[tool.risk_level]} variant="secondary">
            {tool.risk_level}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="pt-0">
        <button
          type="button"
          onClick={() => setShowParams(!showParams)}
          className="text-xs text-muted-foreground hover:text-foreground transition-colors"
        >
          {showParams ? "隐藏" : "显示"}参数 schema
        </button>
        {showParams && (
          <pre className="mt-2 text-xs bg-muted p-2 rounded-md overflow-x-auto">
            {JSON.stringify(tool.parameters, null, 2)}
          </pre>
        )}
      </CardContent>
    </Card>
  )
}
```

**Step 2: Verify compilation**

Run: `cd crates/desktop && pnpm tsc --noEmit`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/desktop/components/settings/tool-card.tsx
git commit -m "feat(desktop): add ToolCard component"
```

---

## Task 5: Create Tools settings page

**Files:**
- Create: `crates/desktop/app/settings/tools/page.tsx`

**Step 1: Create tools page**

```tsx
"use client"

import * as React from "react"
import { tools, type ToolInfo } from "@/lib/tauri"
import { ToolCard } from "@/components/settings/tool-card"

export default function ToolsPage() {
  const [toolList, setToolList] = React.useState<ToolInfo[]>([])
  const [loading, setLoading] = React.useState(true)

  React.useEffect(() => {
    const loadTools = async () => {
      try {
        const data = await tools.list()
        setToolList(data)
      } catch (error) {
        console.error("Failed to load tools:", error)
      } finally {
        setLoading(false)
      }
    }
    loadTools()
  }, [])

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-muted-foreground">加载中...</div>
      </div>
    )
  }

  return (
    <div className="w-full space-y-4">
      <div>
        <h1 className="text-sm font-semibold">工具</h1>
        <p className="text-muted-foreground text-xs">
          系统中的所有可用工具
        </p>
      </div>

      {toolList.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-64 border rounded-lg border-dashed">
          <p className="text-muted-foreground">暂无可用工具</p>
        </div>
      ) : (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {toolList.map((tool) => (
            <ToolCard key={tool.name} tool={tool} />
          ))}
        </div>
      )}
    </div>
  )
}
```

**Step 2: Verify compilation**

Run: `cd crates/desktop && pnpm tsc --noEmit`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/desktop/app/settings/tools/page.tsx
git commit -m "feat(desktop): add tools settings page"
```

---

## Task 6: Add tool selection to AgentEditor

**Files:**
- Modify: `crates/desktop/components/settings/agent-editor.tsx`

**Step 1: Add tool loading state and handler**

Add to state section (~line 46):

```tsx
const [toolList, setToolList] = React.useState<ToolInfo[]>([])
```

Add to useEffect (~line 79):

```tsx
const toolsData = await tools.list()
setToolList(toolsData)
```

Add import at top (~line 7):

```tsx
import { agents, providers, tools, type AgentRecord, type LlmProviderSummary, type ToolInfo } from "@/lib/tauri"
```

**Step 2: Add Checkbox import**

Check shadcn checkbox component exists at `@/components/ui/checkbox`. If not, it may need to be created.

**Step 3: Add tool selection UI section**

Add after the temperature field (~line 246), before closing the left form div:

```tsx
<div className="space-y-2">
  <Label htmlFor="tool_names">可用工具</Label>
  <div className="space-y-2 max-h-48 overflow-y-auto border rounded-md p-3">
    {toolList.length === 0 ? (
      <p className="text-xs text-muted-foreground">暂无可用工具</p>
    ) : (
      toolList.map((tool) => (
        <div key={tool.name} className="flex items-start gap-2">
          <Checkbox
            id={`tool-${tool.name}`}
            checked={formData.tool_names.includes(tool.name)}
            onCheckedChange={(checked) => {
              if (checked) {
                setFormData({
                  ...formData,
                  tool_names: [...formData.tool_names, tool.name],
                })
              } else {
                setFormData({
                  ...formData,
                  tool_names: formData.tool_names.filter((n) => n !== tool.name),
                })
              }
            }}
          />
          <div className="flex-1">
            <Label
              htmlFor={`tool-${tool.name}`}
              className="text-sm font-normal cursor-pointer"
            >
              {tool.name}
            </Label>
            <p className="text-xs text-muted-foreground">{tool.description}</p>
          </div>
        </div>
      ))
    )}
  </div>
</div>
```

**Step 4: Verify compilation**

Run: `cd crates/desktop && pnpm tsc --noEmit`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/desktop/components/settings/agent-editor.tsx
git commit -m "feat(desktop): add tool selection to agent editor"
```

---

## Task 7: Verify end-to-end

**Step 1: Build desktop**

Run: `cd crates/desktop && pnpm tauri build 2>&1 | tail -20`
Expected: Build completes without errors

**Step 2: Run prek check**

Run: `prek`
Expected: All checks pass (fmt, clippy)

**Step 3: Commit final changes**

```bash
git add -A
git commit -m "feat: complete tool exposure feature"
```

---

## Summary

| Task | Description | Files |
|------|-------------|-------|
| 1 | Add ToolInfo and list_tools to ArgusWing | argus-wing/src/lib.rs |
| 2 | Add Tauri command | commands.rs, lib.rs |
| 3 | Add TypeScript API | lib/tauri.ts |
| 4 | Create ToolCard component | tool-card.tsx |
| 5 | Create tools page | tools/page.tsx |
| 6 | Add tool selection to AgentEditor | agent-editor.tsx |
| 7 | Verify end-to-end | Build and prek |
