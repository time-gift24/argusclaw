## Why

当前 AI Chat 对话中，思考过程（reasoning）和任务计划（plan）混杂在消息正文中，布局和交互体验不够清晰。用户无法快速分辨正文和思考内容的层次关系，且 plan 的触发时机（仅在 `tool_completed` 事件后显示为折叠面板）与思考内容的自然布局不匹配。

## What Changes

1. **思考区块位置调整**：将思考（Reasoning）从正文上方移至正文下方，作为独立区块
2. **思考区块交互优化**：默认展开，高度固定（约 150-200px），内容溢出时滚动，并自动滚动到最新内容
3. **Plan 面板提前展示**：当 `update_plan` 工具被调用时，立即在思考区块下方显示 Plan 面板（不等待工具完成）
4. **Plan 实时更新**：Plan 面板随每个任务状态变更实时更新，无需刷新

## Capabilities

### New Capabilities

- `chat-reasoning-panel`: 将 assistant 消息的思考内容独立渲染为固定高度、可滚动的面板，位于正文下方
- `chat-plan-display`: 实现 Plan 面板的提前展示和实时更新逻辑，包括与推理面板的布局协调

### Modified Capabilities

- (无 — 仅涉及前端展示层调整，不改变现有功能契约)

## Impact

- **前端 UI 组件**：`crates/desktop/components/assistant-ui/thread.tsx` 中的 `ReasoningBlock` 组件重构
- **前端状态管理**：`crates/desktop/lib/chat-store.ts` 中 Plan 更新的事件处理逻辑扩展（监听 `tool_started` 事件触发 Plan 展示）
- **前端样式**：`crates/desktop/components/chat/plan-panel.tsx` 调整样式以适配新布局位置
