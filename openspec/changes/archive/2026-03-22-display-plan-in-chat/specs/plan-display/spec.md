# plan-display — 前端 Plan 显示

## 职责

在 chat 页面显示 LLM 的 plan 进度，提供实时可见性。

## 实现细节

### 数据流

```
update_plan tool execute
    ↓
tool_completed event (result: { plan, updated, total })
    ↓
ChatSessionState._handleThreadEvent() 识别 tool_name === "update_plan"
    ↓
从 result.plan 提取 plan 内容
    ↓
写入 ChatSessionState.plan (Zustand state)
    ↓
PlanPanel 组件响应式渲染
```

### 组件结构

```
ThreadViewport
  └── PlanPanel (有 plan 时存在，无 plan 时 DOM 不渲染)
        ├── Header: "Plan (completed/total)" + 折叠按钮
        └── List: 步骤项 (pending/in_progress/completed 状态)
```

### ChatSessionState

```typescript
interface ChatSessionState {
  // ... 现有字段
  plan: PlanItem[] | null;  // null = 无 plan
}
```

## ADDED Requirements

### Requirement: 前端从 tool_completed 解析 plan

ChatSessionState SHALL 在收到 `tool_completed` 事件且 `tool_name === "update_plan"` 时，从 `result.plan` 解析 plan 并存储到 `ChatSessionState.plan`。

#### Scenario: tool_completed 触发 plan 更新

- **WHEN** `_handleThreadEvent` 收到 `tool_completed` 事件且 `tool_name === "update_plan"`
- **THEN** 从 `result.plan` 提取 plan 数组
- **AND** 写入 `ChatSessionState.plan`

#### Scenario: 初始状态无 plan

- **WHEN** Thread 刚加载且 LLM 未调用 `update_plan`
- **THEN** `ChatSessionState.plan` 为 `null`

#### Scenario: plan 为空时隐藏

- **WHEN** `update_plan` 被调用但 plan 数组为空
- **THEN** `ChatSessionState.plan` 设为 `null`
- **AND** PlanPanel 不渲染

### Requirement: PlanPanel 折叠面板 UI

当 `ChatSessionState.plan` 非 null 时，PlanPanel SHALL 渲染在 ThreadViewport 顶部。Panel 包含展开/折叠状态。

#### Scenario: 有 plan 时显示面板

- **WHEN** `ChatSessionState.plan` 非 null 且有内容
- **THEN** PlanPanel 渲染在 ThreadViewport 顶部

#### Scenario: 无 plan 时不渲染面板

- **WHEN** `ChatSessionState.plan` 为 null
- **THEN** PlanPanel 不存在于 DOM 中

#### Scenario: 用户可折叠面板

- **WHEN** 用户点击 PlanPanel 折叠按钮
- **THEN** 面板内容隐藏，仅显示 Header

#### Scenario: 用户可展开面板

- **WHEN** 用户在面板折叠状态下点击展开
- **THEN** 面板内容重新显示

### Requirement: PlanPanel 状态指示器

PlanPanel SHALL 为每个步骤显示正确的状态指示符：pending（圆圈）、in_progress（进行中标记）、completed（勾选）。

#### Scenario: pending 步骤显示空圆圈

- **WHEN** 步骤的 status 为 "pending"
- **THEN** 显示空心圆圈图标

#### Scenario: in_progress 步骤显示进行中状态

- **WHEN** 步骤的 status 为 "in_progress"
- **THEN** 显示进行中图标和文字标记

#### Scenario: completed 步骤显示勾选

- **WHEN** 步骤的 status 为 "completed"
- **THEN** 显示勾选图标

### Requirement: plan_item_count 从快照获取

前端 SHALL 在调用 `get_thread_snapshot` 时从返回的 `ThreadInfo.plan_item_count` 获取 plan 步骤数，用于 Thread 列表等场景。

#### Scenario: 快照刷新时携带 plan_item_count

- **WHEN** `get_thread_snapshot` 返回 ThreadInfo
- **THEN** payload 包含 `plan_item_count` 字段
