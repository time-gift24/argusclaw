## Context

当前 assistant 消息的渲染结构为：

```
AssistantMessage
├── ReasoningBlock (正文上方)
│   ├── InProgress → 展开显示思考内容
│   └── Completed → 折叠为 "思考完成" 可展开标签
└── MarkdownText (正文)
```

Plan 面板目前作为独立的顶层组件，渲染在消息列表最上方（`{session?.plan && <PlanPanel plan={session.plan} />}`），仅在 `tool_completed` 事件且 `tool_name === "update_plan"` 时更新。

**问题**：

1. 思考内容在正文上方，用户阅读顺序不自然
2. 思考内容高度无限制，可能挤压正文空间
3. Plan 面板延迟展示，用户感知不到实时进度
4. Plan 面板与思考内容分离，认知负担高

## Goals / Non-Goals

**Goals:**

- 思考区块置于正文下方，布局层次更清晰
- 思考区块高度固定（约 150px），内容溢出时自动滚动到最新内容
- `update_plan` 工具调用时立即展示 Plan 面板（不等待完成）
- Plan 面板随任务状态变化实时更新

**Non-Goals:**

- 不修改 Plan 数据结构（保持 `PlanItem[]` 不变）
- 不修改 assistant-ui 的消息渲染协议（仅调整渲染顺序和样式）
- 不修改后端逻辑（纯前端展示调整）

## Decisions

### Decision 1: 思考区块移到正文下方

**选择**：将 `ReasoningBlock` 渲染在 `MarkdownText` 之后，而非之前。

**原因**：
- 用户阅读顺序为「正文 → 思考」，更符合从结论到推理的认知习惯
- assistant-ui 的 `MessagePartPrimitive` 支持按内容数组顺序渲染，只需调整 `content` 数组中的 `reasoning` 和 `text` 顺序

**替代方案**：
- 方案 B：保持原顺序但用 CSS `order` 调整视觉顺序 — 增加复杂度，不必要

### Decision 2: 思考区块固定高度 + 自动滚动

**选择**：使用固定 `max-height: 150px` + `overflow-y: auto`，并在 reasoning 内容更新时用 `scrollTop = scrollHeight` 自动滚动。

**原因**：
- 固定高度保证正文始终有足够空间展示
- 自动滚动让用户在长思考过程中持续看到最新内容
- Tailwind CSS 可简洁实现：`max-h-[150px] overflow-y-auto`

**替代方案**：
- 方案 B：根据内容高度动态调整 — 可能导致页面抖动
- 方案 C：折叠为可展开 — 用户需求明确为「默认展开，固定较小高度」

### Decision 3: Plan 面板提前展示 + 监听 `tool_started` 事件

**选择**：在 `chat-store.ts` 中，新增 `tool_started` 事件处理，当 `tool_name === "update_plan"` 时立即设置 `plan` 状态（初始化为传入参数）。

**原因**：
- 当前仅在 `tool_completed` 时更新 plan，用户需等待工具完成才能看到计划
- `update_plan` 工具的首次调用通常在 turn 初期，实时展示可显著提升用户信任感
- `update_plan` 工具的 `arguments` 中已包含 plan 结构（`{ plan: PlanItem[] }`），可直接从中提取初始状态

**数据流**：
```
tool_started (update_plan) → 从 arguments 提取 plan → 设置 plan 状态
tool_completed (update_plan) → 更新 plan 状态（最终状态，覆盖 tool_started 的初始值）
```

**替代方案**：
- 方案 B：仅监听 `tool_started` 不监听 `tool_completed` — 工具可能失败，需要最终状态
- 方案 C：使用 `tool_call_delta` 拼接 arguments — 复杂且 streaming 参数拼接容易出错

### Decision 4: Plan 面板置于思考区块下方

**选择**：Plan 面板作为 assistant 消息的一部分，紧接在 ReasoningBlock 下方渲染，而非顶层消息列表元素。

**原因**：
- Plan 与当前 assistant 消息强关联，归属到具体消息下更合理
- 避免 Plan 面板在多条消息间跳位
- assistant-ui 支持在 `AssistantMessage` 内自由组合子组件

**实现**：在 `AssistantMessage` 组件中，`ReasoningBlock` 之后、`ActionBar` 之前，插入条件渲染的 `PlanPanel`（仅当前 assistant 消息为 pending 时显示）。

**替代方案**：
- 方案 B：保持在消息列表顶层 — 需要处理多消息场景下 plan 归属问题
- 方案 C：在 `ReasoningBlock` 内部组合 — Plan 与思考内容性质不同，分开更清晰

## Risks / Trade-offs

- **[风险]** Plan 面板在消息内的归属问题：若用户快速连续发送多条消息，plan 可能显示在错误的消息下
  - **缓解**：Plan 仅在当前 pending assistant 消息时显示，一旦 turn 完成（`idle` 事件），plan 清空
- **[风险]** 思考内容过长导致滚动时用户错过早期内容
  - **缓解**：固定较小高度 + 自动滚动，用户可通过滚动条回看历史
- **[风险]** `update_plan` 工具参数结构未知，可能导致解析失败
  - **缓解**：使用类型安全的解析 + fallback 到 `null`，不影响主流程
