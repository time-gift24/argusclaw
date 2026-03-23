# Subagent 独立创建入口设计

## 目标

在智能体设置页面提供一个独立入口，允许用户创建新的 Subagent。

## 现状

- `AgentEditor`（智能体编辑器）没有 `parent_agent_id` 字段，只能创建独立的智能体
- 现有的"添加子智能体"按钮（agents 列表页）只能将已有 agent 关联为 subagent，不能新建
- 数据库和后端 API 已支持 `parent_agent_id` 和 `agent_type`

## 方案

### 1. 扩展 AgentEditor

在 `AgentEditor` 组件中增加可选的 `parentId` prop：

```tsx
interface AgentEditorProps {
  agentId?: number
  parentId?: number  // 新增
}
```

- **传入 `parentId`**：标题显示"新建子智能体"，parent 自动预填，可改
- **不传 `parentId`**：标题显示"新建智能体"，parent 为空
- 在"基本信息"区块底部增加"父智能体"下拉框（Select，可选，空值表示独立智能体）

### 2. 新建子智能体入口

在父智能体详情页 `/settings/agents/[id]` 增加"**+ 新建子智能体**"按钮，链接到 `/settings/agents/new?parent=[id]`。

**注意区分按钮命名：**
- 列表页已有"**添加子智能体**"按钮 = 关联已有 agent
- 详情页新增"**+ 新建子智能体**"按钮 = 创建新 agent 并关联

### 3. 读取 URL 参数

将 `/settings/agents/new` 页面改为 client component，从 URL 查询参数 `?parent=` 读取 parent_id 并传给 `AgentEditor`。

### 4. 数据流

```
用户点击父智能体详情页「新建子智能体」
  → /settings/agents/new?parent=5
  → AgentEditor 读取 parent=5，预填 parent dropdown
  → 用户填写表单（可改 parent）
  → agents.upsert() 提交时带上 parent_agent_id
  → 保存后跳转到 /settings/agents/{new_id}
```

## 改动文件

| 文件 | 改动 |
|------|------|
| `crates/desktop/components/settings/agent-editor.tsx` | 增加 `parentId` prop；增加"父智能体"下拉框 |
| `crates/desktop/app/settings/agents/new/page.tsx` | 改为 client component，读取 `?parent=` 参数 |
| `crates/desktop/app/settings/agents/[id]/page.tsx` | 增加"新建子智能体"按钮 |

## 改动详情

### AgentEditor

- 新增 `parentId?: number` prop
- 新增 `parentAgentList` state（用于下拉选项）
- "基本信息"区块底部增加"父智能体"下拉框（Select），选项为"无"（空值，表示独立智能体）加上其他可用作父智能体的候选（过滤条件：`!agent.parent_agent_id && agent.agent_type !== "subagent"`，并排除自身）
- **循环层级校验**：编辑已有 subagent 时，父智能体下拉框需排除自身和自身的所有 subagent（递归），防止循环引用。具体算法：
  1. 从当前编辑的 agent 开始，递归收集所有 `parent_agent_id === 当前agent.id` 的 subagent ID
  2. 对每个 subagent 递归执行同样逻辑
  3. 最终得到一个"不可选" ID 集合，将这些从父智能体下拉框中排除
- 标题动态显示：
  - `parentId` prop 传入时：显示"新建子智能体"
  - 编辑模式（`agentId` 存在）且 `formData.parent_agent_id` 非空：显示"编辑子智能体"
  - 其他情况：显示"编辑智能体"（已有逻辑）或"新建智能体"（已有逻辑）
- 初始化时：如果 `parentId` 存在，在数据加载完成后（`agents.get()` 返回 agent 后），将 `parent_agent_id` 设为 `parentId`
- **agent_type 自动推断**：`agents.upsert()` 提交时，后端根据 `parent_agent_id` 是否存在自动设置 `agent_type`（有则为 `'subagent'`，否则 `'standard'`），前端无需显式设置
- 父智能体下拉框加载时显示 disabled 状态，数据加载完成后渲染选项

### /settings/agents/new 页面

将 `/settings/agents/new` 页面改为 client component，用 `Suspense` 包裹以支持 `useSearchParams()`：

```tsx
import { Suspense } from "react"
"use client"

function NewAgentContent() {
  const searchParams = useSearchParams()
  const parentId = searchParams.get("parent")
  return <AgentEditor parentId={parentId ? parseInt(parentId) : undefined} />
}

export default function NewAgentPage() {
  return (
    <Suspense fallback={<div className="flex items-center justify-center h-64">加载中...</div>}>
      <NewAgentContent />
    </Suspense>
  )
}
```

**注意**：`useSearchParams()` 在 Next.js 中需要 Suspense boundary 包裹，否则会触发 build warning 并影响部分预渲染（Partial Prerendering）。面包屑"设置 / 智能体 / 新建"不受 `?parent=` 参数影响，保持不变。

### /settings/agents/[id] 页面

在页面顶部按钮区（保存按钮旁）增加"新建子智能体"按钮，链接到 `/settings/agents/new?parent=[id]`。

## 验收标准

- [ ] 访问 `/settings/agents/new?parent=5`，表单中"父智能体"下拉框预选为 ID=5 的智能体
- [ ] 创建子智能体后，数据库中该记录 `parent_agent_id=5` 且 `agent_type='subagent'`
- [ ] 编辑已有 subagent 时，父智能体下拉框不包含自身及其所有 subagent
- [ ] 父智能体下拉框在数据加载完成前显示 disabled 状态
- [ ] `?parent=abc`（无效值）被安全忽略，不报错
