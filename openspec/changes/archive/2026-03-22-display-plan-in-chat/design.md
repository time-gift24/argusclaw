## Context

当前 `update_plan` tool 已实现，plan 状态存储在 `Thread.plan: Arc<RwLock<Vec<Value>>>` 中（内存），LLM 在 `tool_completed` 事件中返回 plan 结果。但前端无法看到 plan，且 Thread 重启后 plan 丢失。

**当前架构（关键片段）：**

```
SessionManager (trace_dir: PathBuf)
  └── ThreadBuilder
        ├── TurnConfig.trace_config (per-thread: {trace_dir}/{thread_id}/)
        └── Thread
              ├── plan: Arc<RwLock<Vec<Value>>>  ← 仅内存，无持久化
              └── execute_turn_streaming()
                    └── UpdatePlanTool (写入 Thread.plan)

Tauri desktop
  └── chat-store.ts (ChatSessionState)
        └── _handleThreadEvent() ← 监听 thread:event
```

**利益相关方：** LLM agent 用户，需要在 chat UI 中实时查看 plan 进度。

## Goals / Non-Goals

**Goals:**
- Plan 持久化到文件系统，支持 Thread 重启后恢复
- 前端 chat 页面显示 LLM 的 plan 进度（步骤描述 + 状态）
- Plan 有内容时显示折叠面板，无内容时完全隐藏
- `get_thread_snapshot` 返回 `plan_item_count`

**Non-Goals:**
- 不修改 `update_plan` tool 的 API（参数格式不变）
- 不支持跨 Thread 共享 plan
- 不提供 plan 的手动编辑/删除能力
- 不修改 `argus-thread` 对外的公共 API（`Thread.plan()` 的返回类型暂时保持不变）

## Decisions

### D1: FilePlanStore 作为 Thread 内部存储

**选项 A：Thread 直接持有 `Arc<RwLock<Vec<Value>>>`（现状）**
- 优点：无额外抽象，最小改动
- 缺点：重启后丢失

**选项 B：Thread 持有 FilePlanStore（实现 plan 持久化）**
- FilePlanStore 内部持有 `Arc<RwLock<Vec<Value>>>` + 持久化路径
- 优点：内存操作不变，支持持久化，API 兼容
- 缺点：多了一个内部 struct

**选项 C：引入 `PlanStore` trait + `FilePlanStore` 实现**
- 优点：可插拔
- 缺点：过度工程化，后续不需要替换存储后端

**Decision: B** — FilePlanStore 作为 Thread 内部实现细节，不暴露 trait。Thread 持有 `FilePlanStore` 而非直接持有 `Arc<RwLock<Vec<Value>>>`。UpdatePlanTool 从 Thread 获取 store 时，拿到的是同样的引用。

**文件路径：** `{trace_dir}/{thread_id}/plan.json`

trace_dir 由 SessionManager 管理，在 `create_thread` 时已可用。FilePlanStore 在 Thread build 时创建，路径为 `thread_trace_dir / "plan.json"`。

### D2: plan_item_count 从快照刷新时获取

**Decision: 通过 `get_thread_snapshot` 命令返回 plan_item_count。**

不新增独立的 `get_plan` 命令，而是将 plan 信息整合进现有快照流程：
- Thread 的 `info()` 方法已包含 `plan_item_count: self.plan.len()`
- `get_thread_snapshot` 返回值中已包含 `ThreadInfo` → 自动携带 `plan_item_count`
- 前端通过 `get_thread_snapshot` 刷新时自然拿到最新 count

### D3: 前端 plan 数据来源

**Decision: 从 `tool_completed` 事件的 `result` 中解析 plan。**

```
tool_completed { tool_name: "update_plan", result: { plan: [...], updated: N, total: N } }
```

当 `tool_name === "update_plan"` 时，从 result 中提取 plan 数组，写入 `ChatSessionState.plan`。Snapshot 刷新时不覆盖前端 plan 状态（避免服务端覆盖客户端实时更新）。

### D4: Plan 面板 UI 位置

**Decision: 位于 ThreadViewport 顶部，折叠面板形式。**

```
┌──────────────────────────────────────────────┐
│ ▼ Plan (3/5)                            [−] │  ← 可折叠
│   ○ Step 1: Research requirements          │
│   ● Step 2: Implement core logic   (进行中)  │
│   ✓ Step 3: Write tests                    │
│   ○ Step 4: Update documentation            │
│   ○ Step 5: Create PR                      │
└──────────────────────────────────────────────┘
```

- 有 plan 时显示，无 plan 时 DOM 完全不存在（不是 `display: none`）
- 默认展开，用户可折叠
- 展开/折叠状态不持久化

### D5: UpdatePlanTool 不感知 FilePlanStore

**Decision: UpdatePlanTool 仍通过 `Arc<RwLock<Vec<Value>>>` 操作，FilePlanStore 负责读写文件。**

FilePlanStore 在内部监听 RwLock 的写事件（通过 `notify` 或定期同步），或提供 `sync()` 方法供 Thread 在适当时机调用。

**实际方案：** UpdatePlanTool 调用 `FilePlanStore::write()` 写入内存 + 文件，而非仅写内存。

具体：FilePlanStore 提供 `store: Arc<RwLock<Vec<Value>>>` 字段，外部通过 `.store()` 获取引用。UpdatePlanTool 构造时从 Thread 获取此引用，execute 时同时更新内存和文件。

## Risks / Trade-offs

| Risk | Mitigation |
|------|-----------|
| 频繁写文件影响性能（每次 tool 调用都写 plan.json） | plan.json 通常很小（<10KB），tokio::fs 为 async，不会阻塞 turn 执行 |
| Thread 重启后文件存在但 plan 已过期（LLM 后续调用覆盖） | 每次 `update_plan` 调用都全量覆盖文件，文件内容始终与内存一致 |
| 前端 plan 状态与后端不一致（前端从 tool_completed 更新，后端从文件恢复） | 初始化时从文件恢复后，前端数据源统一为 tool_completed，不依赖服务端推送 |
| FilePlanStore 创建时 trace_dir 可能不存在 | SessionManager.create_thread 中 trace_dir 已确保存在（join 操作） |

## Open Questions

- **Q1:** plan.json 在 Thread 销毁时是否删除？→ **暂不删除**，trace_dir 保留所有历史文件，便于调试。
- **Q2:** plan 折叠状态是否持久化？→ **不持久化**，每次进入 thread 默认展开。
