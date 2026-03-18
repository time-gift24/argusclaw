# Tool Exposure Design

**Date:** 2026-03-19
**Status:** Approved

## Overview

Expose available tools from argus-wing so the desktop can display tool information and allow agents to select which tools they can use.

## Architecture

```
argus-wing (Rust)
  └── ToolManager.list_definitions() → Vec<ToolDefinition>
  └── ToolManager.get_risk_level(name) → RiskLevel
  └── ArgusWing::list_tools() → Vec<ToolInfo>

desktop Tauri (Rust)
  └── list_tools command → ToolInfo[]

desktop React (TypeScript)
  └── lib/tauri.ts → tools.list()
  └── /settings/tools (page)
  └── agent-editor.tsx (checkbox selection)
```

## Data Types

### Rust: ToolInfo

```rust
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub risk_level: RiskLevel,
    pub parameters: serde_json::Value, // JSON Schema
}
```

### TypeScript: ToolInfo

```typescript
interface ToolInfo {
  name: string;
  description: string;
  risk_level: "low" | "medium" | "high" | "critical";
  parameters: Record<string, unknown>; // JSON Schema object
}
```

## Implementation Phases

### Phase 1: Backend API (argus-wing)

**File:** `crates/argus-wing/src/lib.rs`

Add to `ArgusWing`:

```rust
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub risk_level: argus_protocol::RiskLevel,
    pub parameters: serde_json::Value,
}

impl ArgusWing {
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

### Phase 2: Tauri Commands (desktop/src-tauri)

**File:** `crates/desktop/src-tauri/src/commands.rs`

Add command:

```rust
#[tauri::command]
pub async fn list_tools(wing: State<'_, Arc<ArgusWing>>) -> Result<Vec<ToolInfo>, String> {
    wing.list_tools().await.map_err(|e| e.to_string())
}
```

Register in `lib.rs` commands module.

### Phase 3: Frontend API (lib/tauri.ts)

**File:** `crates/desktop/lib/tauri.ts`

```typescript
export interface ToolInfo {
  name: string;
  description: string;
  risk_level: "low" | "medium" | "high" | "critical";
  parameters: Record<string, unknown>;
}

export const tools = {
  list: () => invoke<ToolInfo[]>("list_tools"),
};
```

### Phase 4: Tools Page

**File:** `crates/desktop/app/settings/tools/page.tsx`

Layout: Card grid (3 columns on large screens)
Card content:
- Tool name (title)
- Description (text-xs, muted)
- Risk level badge (colored)
- Parameters schema (collapsible, text-xs)

Reuse `Card` component from shadcn-studio.

**File:** `crates/desktop/components/settings/tool-card.tsx` (new)

### Phase 5: Agent Editor Tool Selection

**File:** `crates/desktop/components/settings/agent-editor.tsx`

Add section after provider selection:

```tsx
<div className="space-y-2">
  <Label>可用工具</Label>
  <div className="space-y-2 max-h-48 overflow-y-auto border rounded-md p-3">
    {tools.map((tool) => (
      <div key={tool.name} className="flex items-start gap-2">
        <Checkbox
          id={tool.name}
          checked={formData.tool_names.includes(tool.name)}
          onCheckedChange={(checked) => {
            if (checked) {
              setFormData({
                ...formData,
                tool_names: [...formData.tool_names, tool.name],
              });
            } else {
              setFormData({
                ...formData,
                tool_names: formData.tool_names.filter((n) => n !== tool.name),
              });
            }
          }}
        />
        <div className="flex-1">
          <Label htmlFor={tool.name} className="text-sm font-normal cursor-pointer">
            {tool.name}
          </Label>
          <p className="text-xs text-muted-foreground">{tool.description}</p>
        </div>
      </div>
    ))}
  </div>
</div>
```

## UI Mockup

```
┌─────────────────────────────────────────────────────────┐
│  工具                                                   │
│  系统中的所有可用工具                                     │
├─────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │
│  │ shell       │  │ read        │  │ glob        │  │
│  │ 执行 Shell  │  │ 读取文件    │  │ 文件匹配    │  │
│  │ [Critical]  │  │ [Low]       │  │ [Low]       │  │
│  └─────────────┘  └─────────────┘  └─────────────┘  │
└─────────────────────────────────────────────────────────┘
```

## Risk Badge Colors

| Risk Level | Color (Tailwind) |
|------------|------------------|
| Low | `bg-green-100 text-green-800` |
| Medium | `bg-yellow-100 text-yellow-800` |
| High | `bg-orange-100 text-orange-800` |
| Critical | `bg-red-100 text-red-800` |

## Files to Modify

1. `crates/argus-wing/src/lib.rs` - Add `list_tools()` method and `ToolInfo` struct
2. `crates/desktop/src-tauri/src/commands.rs` - Add `list_tools` command
3. `crates/desktop/src-tauri/src/lib.rs` - Register command
4. `crates/desktop/lib/tauri.ts` - Add `ToolInfo` type and `tools` API
5. `crates/desktop/app/settings/tools/page.tsx` - New tools listing page
6. `crates/desktop/components/settings/tool-card.tsx` - New tool card component
7. `crates/desktop/components/settings/agent-editor.tsx` - Add tool selection UI
