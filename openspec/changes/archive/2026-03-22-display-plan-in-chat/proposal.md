## Why

The `update_plan` tool exists in the backend, but users have no way to see the LLM's task plan during chat. Displaying plan progress in the chat UI enables users to track the LLM's autonomous task execution in real-time, building trust and providing visibility into agentic behavior.

## What Changes

- **FilePlanStore**：将 plan 持久化到线程 trace 目录下的 `plan.json`，解决 Thread 重启后 plan 丢失的问题
- **get_plan Tauri command**：前端按需获取当前 plan 状态
- **前端 plan 状态**：从 `update_plan` 的 `tool_completed` 结果中提取 plan，存储到 `ChatSessionState.plan`
- **Plan 折叠面板 UI**：位于 ThreadViewport 顶部，有 plan 时显示，无 plan 时完全隐藏
- **plan_item_count** 从快照刷新时获取：通过 `get_thread_snapshot` 返回 plan_item_count

## Capabilities

### New Capabilities

- `plan-persistence`: Plan 持久化到文件系统（`{trace_dir}/{thread_id}/plan.json`），支持跨 Thread 重启持久化
- `plan-display`: 前端 chat 页面显示 LLM 的 plan 进度，包含步骤描述和状态（pending/in_progress/completed）

### Modified Capabilities

- `update-plan-tool`: 现有 update-plan-tool 的存储后端从内存改为 FilePlanStore（API 不变）

## Impact

- **新增文件**：
  - `crates/argus-thread/src/plan_store.rs` — FilePlanStore 实现
  - `crates/desktop/src-tauri/src/plan_commands.rs` — `get_plan` 命令
  - `crates/desktop/components/chat/plan-panel.tsx` — Plan 折叠面板组件
  - `crates/desktop/lib/types/plan.ts` — Plan 类型定义
- **修改文件**：
  - `crates/argus-thread/src/thread.rs` — Thread 持有 FilePlanStore 而非 `Arc<RwLock<Vec<Value>>>`
  - `crates/argus-thread/src/lib.rs` — 导出 FilePlanStore
  - `crates/desktop/src-tauri/src/commands.rs` — 注册 get_plan 命令
  - `crates/desktop/lib/chat-store.ts` — plan 状态管理
  - `crates/desktop/lib/types/chat.ts` — ThreadSnapshotPayload 增加 plan_item_count
  - `crates/desktop/components/assistant-ui/thread.tsx` — 嵌入 PlanPanel
  - `crates/desktop/lib/chat-runtime.ts` — plan 传递给 AssistantUiMessage
- **无新增前端依赖**：使用现有 UI 组件库
- **无新增 Rust 依赖**：仅使用 `tokio::fs`（已有）和标准库
