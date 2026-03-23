# Subagent 独立创建入口实现计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在智能体设置页面提供独立入口，允许用户创建新的 Subagent。

**Architecture:** 扩展 `AgentEditor` 组件，增加 `parentId` prop 和"父智能体"下拉框；在详情页增加新建子智能体入口按钮；将 `/new` 页面改为 client component 读取 URL 参数。

**Tech Stack:** React 19, TypeScript, Next.js, Tailwind CSS, shadcn/ui

**Spec:** `docs/superpowers/specs/2026-03-23-subagent-create-design.md`

---

## Chunk 1: 扩展 AgentEditor

**Files:**
- Modify: `crates/desktop/components/settings/agent-editor.tsx`

- [ ] **Step 1: 添加 `parentId` prop 和 state**

在 `AgentEditorProps` 接口添加 `parentId?: number`，并添加以下 state：

```tsx
// 在现有 state 声明后添加
const [parentAgentList, setParentAgentList] = React.useState<AgentRecord[]>([])
```

- [ ] **Step 2: 在 useEffect 中加载父智能体列表**

在 `loadData` async 函数中，`tools.list()` 之后添加：

```tsx
const allAgents = await agents.list()
const candidates = allAgents.filter(
  (a) => !a.parent_agent_id && a.agent_type !== "subagent" && a.id !== agentId
)
setParentAgentList(candidates)
```

这样在编辑模式下会排除自身，在新建模式下排除所有 subagent。

- [ ] **Step 3: 添加循环层级校验辅助函数**

在文件底部 `cn` 函数之后添加：

```tsx
function getExcludedAgentIds(agentId: number | undefined, allAgents: AgentRecord[]): Set<number> {
  if (agentId === undefined) return new Set()

  const excluded = new Set<number>()
  const queue = [agentId]

  while (queue.length > 0) {
    const current = queue.shift()!
    const children = allAgents.filter((a) => a.parent_agent_id === current)
    for (const child of children) {
      if (!excluded.has(child.id)) {
        excluded.add(child.id)
        queue.push(child.id)
      }
    }
  }

  return excluded
}
```

- [ ] **Step 4: 动态标题逻辑**

将标题 JSX 从：
```tsx
<h1 className="text-base font-semibold">
  {isEditing ? "编辑智能体" : "新建智能体"}
</h1>
```

改为：
```tsx
<h1 className="text-base font-semibold">
  {parentId !== undefined ? "新建子智能体" : isEditing && formData.parent_agent_id ? "编辑子智能体" : isEditing ? "编辑智能体" : "新建智能体"}
</h1>
```

- [ ] **Step 5: parentId 预填逻辑**

在 `loadData` 的编辑分支中（`agents.get()` 返回后），添加：

```tsx
if (agent) {
  setFormData(agent)
} else if (parentId !== undefined) {
  // 新建模式，parentId 由 URL 传入，预填 parent_agent_id
  setFormData({ ...createDefaultFormData(preferredProviderId), parent_agent_id: parentId })
} else {
  setFormData(createDefaultFormData(preferredProviderId))
}
```

注意：需要将 `parentId` 也加入 `useEffect` 的依赖中。将 `[agentId]` 改为 `[agentId, parentId]`。

- [ ] **Step 6: 添加"父智能体"下拉框**

在"基本信息"区块底部（`</div>` 闭合前），`</div>` 之前添加：

```tsx
<div className="space-y-1.5">
  <Label htmlFor="parent_agent_id" className="text-xs">父智能体</Label>
  <select
    id="parent_agent_id"
    value={formData.parent_agent_id?.toString() ?? ""}
    onChange={(e) =>
      setFormData({
        ...formData,
        parent_agent_id: e.target.value ? parseInt(e.target.value) : undefined,
      })
    }
    className="flex h-9 w-full rounded-md border border-input bg-input/20 px-3 py-1.5 text-sm outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30"
  >
    <option value="">无（独立智能体）</option>
    {(() => {
      const excluded = getExcludedAgentIds(agentId, parentAgentList)
      return parentAgentList
        .filter((a) => !excluded.has(a.id))
        .map((a) => (
          <option key={a.id} value={a.id}>
            {a.display_name} (v{a.version})
          </option>
        ))
    })()}
  </select>
</div>
```

将下拉框放在"描述"输入框下方（`</div>` 之前）。

- [ ] **Step 7: 验证 TypeScript 类型**

确保 `AgentRecord` 类型包含 `parent_agent_id` 字段（已有，backend 传来）。如 TypeScript 报错，需要确认 `lib/tauri.ts` 中 `AgentRecord` 类型定义包含该字段。

- [ ] **Step 8: 运行 TypeScript 检查**

Run: `cd crates/desktop && npx tsc --noEmit 2>&1 | head -50`
Expected: 无 `parent_agent_id` 相关错误

- [ ] **Step 9: Commit**

```bash
git add crates/desktop/components/settings/agent-editor.tsx
git commit -m "feat(desktop): add parentId prop and parent dropdown to AgentEditor"
```

---

## Chunk 2: 更新 /new 页面

**Files:**
- Modify: `crates/desktop/app/settings/agents/new/page.tsx`

- [ ] **Step 1: 替换为 client component + Suspense**

将整个文件内容替换为：

```tsx
"use client"

import * as React from "react"
import { useSearchParams } from "next/navigation"
import { AgentEditor } from "@/components/settings"

function NewAgentContent() {
  const searchParams = useSearchParams()
  const parentId = searchParams.get("parent")
  return <AgentEditor parentId={parentId ? parseInt(parentId) : undefined} />
}

export default function NewAgentPage() {
  return (
    <React.Suspense fallback={
      <div className="flex items-center justify-center h-64">
        <div className="text-muted-foreground">加载中...</div>
      </div>
    }>
      <NewAgentContent />
    </React.Suspense>
  )
}
```

**注意**：`useSearchParams` 必须从 `next/navigation` 导入，且该组件必须被 `Suspense` 包裹。

- [ ] **Step 2: 验证构建**

Run: `cd crates/desktop && pnpm build 2>&1 | tail -20`
Expected: 无编译错误

- [ ] **Step 3: Commit**

```bash
git add crates/desktop/app/settings/agents/new/page.tsx
git commit -m "feat(desktop): change /new agent page to client component with Suspense for useSearchParams"
```

---

## Chunk 3: 更新 /[id] 详情页

**Files:**
- Modify: `crates/desktop/app/settings/agents/[id]/page.tsx`

- [ ] **Step 1: 添加按钮导入**

当前文件是 server component。需要改为 client component 以使用 `Link` 和按钮。先确认 server component 是否能直接使用 `Link`（可以）。添加 import：

```tsx
import Link from "next/link"
import { Plus } from "lucide-react"
import { Button } from "@/components/ui/button"
```

- [ ] **Step 2: 添加"新建子智能体"按钮**

在 `AgentEditor` 组件上方添加按钮，放在保存按钮右侧：

```tsx
export default async function EditAgentPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params

  return (
    <div className="w-full space-y-4">
      <div className="flex items-center justify-end">
        <Link href={`/settings/agents/new?parent=${id}`}>
          <Button size="sm">
            <Plus className="h-4 w-4 mr-1" />
            新建子智能体
          </Button>
        </Link>
      </div>
      <AgentEditor agentId={parseInt(id)} />
    </div>
  )
}
```

**注意**：将整个页面包裹在一个带 `space-y-4` 的 div 中，让按钮和编辑器纵向排列。

- [ ] **Step 3: 验证构建**

Run: `cd crates/desktop && pnpm build 2>&1 | tail -20`
Expected: 无编译错误

- [ ] **Step 4: Commit**

```bash
git add crates/desktop/app/settings/agents/[id]/page.tsx
git commit -m "feat(desktop): add '新建子智能体' button to agent detail page"
```

---

## Chunk 4: 验证

**Files:**
- All modified files

- [ ] **Step 1: 运行 TypeScript 类型检查**

Run: `cd crates/desktop && npx tsc --noEmit 2>&1`
Expected: 无错误

- [ ] **Step 2: 运行构建**

Run: `cd crates/desktop && pnpm build 2>&1 | tail -30`
Expected: 构建成功

- [ ] **Step 3: 验证验收标准**

手动测试以下场景：
1. 访问 `/settings/agents/new?parent=5`，确认"父智能体"下拉框预选 ID=5
2. 创建子智能体，保存后确认跳转到详情页
3. 编辑已有 subagent，确认父下拉框不包含自身及其 subagent
4. 访问 `/settings/agents/new?parent=abc`，确认不报错
5. 确认详情页有"新建子智能体"按钮

- [ ] **Step 4: Commit 所有剩余变更**

```bash
git add -A && git commit -m "feat(desktop): implement subagent independent creation entry"
```

---

## 依赖关系

```
Chunk 1 (AgentEditor) → Chunk 2 (/new page) → Chunk 3 (/[id] page) → Chunk 4 (验证)
```

Chunk 1 是核心改动，Chunk 2 和 Chunk 3 依赖 Chunk 1 的 `parentId` prop。Chunk 4 依赖所有改动完成。
