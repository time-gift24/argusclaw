# 历史会话恢复功能设计

**日期**: 2026-03-24
**状态**: 审核中
**负责人**: @claude

## 概述

在桌面端侧边栏实现历史会话管理功能，支持查看、恢复、重命名、删除会话与线程。

## 背景

当前应用后端（SQLite）已持久化存储会话和线程数据，前端只能创建新会话，无法浏览和恢复历史对话。本功能补全这一缺失能力。

## 需求

- 侧边栏会话列表（可折叠树形：会话 → 线程）
- 会话列表项显示：会话名 · 线程数 · 更新时间
- 线程列表项显示：线程标题（或"无标题"）· 消息条数 · token 估计数
- 点击线程行加载历史消息到聊天区
- 恢复时优先使用原 provider，不存在则提示用户选择
- 支持重命名和删除会话/线程
- **线程重命名不在本版本范围内**（如需可在后续迭代中添加 `rename_thread` API）

---

## UI 设计

### 侧边栏布局

```
┌─────────────────────────────────────┐
│ [Agent 选择器]  [Provider 选择器]      │
├─────────────────────────────────────┤
│ 历史会话                               │
│ ▼ 会话 A (3 个线程) · 3 分钟前        │
│   ├ 线程 1 · 5 条消息 · ~1k tokens   │
│   └ 线程 2 · 12 条消息 · ~3k tokens   │
│ ▶ 会话 B (1 个线程) · 1 小时前        │
│ ▶ 会话 C (2 个线程) · 昨天            │
├─────────────────────────────────────┤
│ [+ 新建会话]                          │
└─────────────────────────────────────┘
```

- 侧边栏宽度：240-280px
- 会话行高度：36px
- 线程行左侧缩进：20px
- 折叠/展开图标：ChevronRight / ChevronDown
- 右键上下文菜单：重命名、删除

### 交互流程

1. 侧边栏加载时调用 `list_sessions()`
2. 点击会话行 → 展开/折叠线程列表
3. 点击线程行 → 加载该线程到右侧聊天区，同时设置当前 session/thread 上下文
4. 右键会话行 → 重命名会话 / 删除会话
5. 右键线程行 → 删除线程
6. 恢复时检查 provider 存在性：
   - 存在 → 直接加载历史消息
   - 不存在 → toast 提示"该会话使用的 Provider 已不存在，请选择一个新的"

### 加载状态

- 列表加载中：骨架屏或 spinner
- 空状态：显示"暂无历史会话，开始一个新对话吧"
- 加载历史线程：线程行内联 spinner

---

## API 设计

### 后端（Tauri Commands）

| 命令 | 输入 | 输出 | 状态 |
|------|------|------|------|
| `list_sessions` | - | `Vec<SessionSummary>` | **需新增 Tauri command** |
| `list_threads` | `session_id: i64` | `Vec<ThreadSummary>` | **需新增 Tauri command** |
| `rename_session` | `session_id: i64, name: String` | `()` | **需新增（完整实现）** |
| `delete_session` | `session_id: i64` | `()` | **需新增 Tauri command** |
| `delete_thread` | `session_id: i64, thread_id: String` | `()` | **需新增 Tauri command**（后端已有 `SessionManager::delete_thread(session_id, thread_id)`，需同时传 session_id）|
| `get_thread_snapshot` | `session_id, thread_id` | 完整快照 | 已有 |
| `get_provider` | `provider_id: i64` | Provider 或 None | 已有 |

#### SessionSummary（已有）

```rust
pub struct SessionSummary {
    pub id: SessionId,        // i64
    pub name: String,
    pub thread_count: i64,
    pub updated_at: DateTime<Utc>,
}
```

#### ThreadSummary（需修改）

`provider_id` 字段需新增到 `ThreadSummary` 中，用于恢复时检查 provider 是否仍然有效。

```rust
pub struct ThreadSummary {
    pub id: ThreadId,              // UUID
    pub provider_id: Option<i64>,   // ★ 新增：恢复时需检查此 provider 是否仍存在
    pub title: Option<String>,
    pub turn_count: i64,
    pub token_count: i64,
    pub updated_at: DateTime<Utc>,
}
```

### 前端（chat-store.ts）

**与现有 `sessionsByKey` 的共存策略**：

现有 `chat-store.ts` 使用 `sessionKey = "templateId::providerPreferenceId"` 管理活跃聊天会话。历史会话功能引入新的状态结构 `historySessions`，两个数据域完全独立：

- **`sessionsByKey`**：活跃会话（当前对话中），按模板+provider 索引
- **`historySessions`**：历史会话浏览（只读列表），按 `session_id` 索引

恢复历史线程时，将历史数据加载到 `sessionsByKey` 中，同时更新 `historySessions` 中的统计信息（线程数等）。

新增状态：

```typescript
interface HistorySessionState {
  // 历史会话列表
  sessions: SessionSummary[];
  expandedSessions: Set<number>;   // 已展开的 session id 集合
  threads: Map<number, ThreadSummary[]>;  // sessionId -> threads

  // 加载状态
  historyLoading: boolean;
  threadsLoading: Set<number>;  // 正在加载线程的 session id

  // 操作方法
  loadSessions(): Promise<void>;
  toggleSession(sessionId: number): Promise<void>;
  loadThread(sessionId: number, threadId: string): Promise<void>;
  renameSession(sessionId: number, name: string): Promise<void>;
  deleteSession(sessionId: number): Promise<void>;
  deleteThread(sessionId: number, threadId: string): Promise<void>;
}
```

---

## 组件设计

### 1. `<HistorySidebar>`

侧边栏容器，mount 时调用 `loadSessions()`。

### 2. `<SessionList>`

渲染 `sessions` 数组，每个 `<SessionItem>` 可展开。

### 3. `<SessionItem>`

- 展开状态：显示 `<ThreadList>`
- 折叠状态：只显示会话信息行
- 右键菜单：重命名、删除

### 4. `<ThreadItem>`

- 显示：标题 + 消息数 + token 数
- 右键菜单：删除
- 点击：加载线程

### 5. `<RenameDialog>`

模态对话框，用于重命名会话。输入框 + 确认/取消按钮。

### 6. `<DeleteConfirmDialog>`

确认对话框，删除时弹出。显示即将删除的内容摘要。

---

## 技术实现要点

### 后端改动

1. **`crates/argus-session/src/session.rs`**：`ThreadSummary` 新增 `provider_id: Option<i64>` 字段，类型映射自 `Option<ProviderId>`
2. **`crates/argus-session/src/manager.rs`**：
   - `SessionManager::list_threads()` 的 SQL 查询需增加 `provider_id` 列：`SELECT id, provider_id, title, token_count, turn_count, updated_at FROM threads WHERE session_id = ?`
   - `Session::list_threads()` 内存路径需从 `t.agent_record().provider_id` 提取并映射为 `Option<i64>`
   - 新增 `rename_session(session_id, new_name)` 方法（SQL UPDATE `sessions SET name = ?, updated_at = ? WHERE id = ?`）
3. **`crates/argus-wing/src/lib.rs`**：`ArgusWing` facade 新增 `rename_session` 代理方法
4. **`crates/desktop/src-tauri/src/commands.rs`**：新增以下 Tauri command：
   - `list_sessions` → 调用 `SessionManager::list_sessions()`
   - `list_threads` → 调用 `SessionManager::list_threads(session_id)`
   - `delete_session` → 调用 `ArgusWing::delete_session(session_id)`
   - `delete_thread` → 调用 `ArgusWing::delete_thread(session_id, thread_id)`（需同时传 session_id）
   - `rename_session` → 调用 `ArgusWing::rename_session(session_id, name)`

### 前端改动

1. **`crates/desktop/lib/chat-store.ts`**：新增 `sessions`、`threads`、`expandedSessions` 状态和相关方法
2. **`crates/desktop/components/`**：新增组件（参考上方组件设计）
3. **`crates/desktop/lib/tauri.ts`**：新增 Tauri command wrapper
4. **`crates/desktop/app/`**：新增历史页面路由或整合到现有侧边栏

### Provider 恢复检查 & 线程加载流程

在 `loadThread()` 流程中：

1. 获取 `ThreadSummary`（含 `provider_id`）
2. 调用 `get_provider(provider_id)` 检查是否存在
3. **若 provider 不存在** → toast 提示"该会话使用的 Provider 已不存在，请选择一个新的"，并触发 provider 选择器高亮
4. **若 provider 存在** → 继续步骤 5
5. 调用 `get_thread_snapshot(session_id, thread_id)` 获取完整消息历史
6. 启动该线程的事件转发订阅（参考 `create_chat_session` 中 `ThreadSubscriptions::start` 的模式），确保恢复后实时事件能推送到前端
7. 将消息数据加载到 `sessionsByKey`，设置当前活跃会话上下文

> 恢复后的线程被视为"活跃会话"，用户可以继续发送新消息、接收实时事件流。

---

## 错误处理

所有 toast 消息应**包含底层错误详情**，而非静态字符串：

| 场景 | 处理 |
|------|------|
| `list_sessions` 失败 | toast `"加载历史会话失败: ${error}"` |
| `list_threads` 失败 | toast `"加载线程列表失败: ${error}"` |
| `rename_session` 失败 | toast `"重命名失败: ${error}"` |
| `delete_session` 失败 | toast `"删除会话失败: ${error}"` |
| `delete_thread` 失败 | toast `"删除线程失败: ${error}"` |
| `get_thread_snapshot` 失败 | toast `"加载历史消息失败: ${error}"` |
| Provider 不存在 | toast "该会话使用的 Provider 已不存在，请选择一个新的"（前端业务校验，无后端错误） |
| 并发删除（已删除的会话/线程） | 静默忽略（列表自动刷新），无需 toast |

---

## 测试计划

1. **会话列表加载**：无会话 / 有多个会话 / 大量会话（100+）
2. **展开/折叠**：快速连续点击、展开多个
3. **恢复历史线程**：消息数 1 / 100 / 1000 条
4. **重命名**：空名称 / 正常名称 / 超长名称
5. **删除**：取消确认、确认删除、确认后验证数据完整性（列表刷新、计数更新）
6. **Provider 不存在场景**：模拟 provider 已删除，验证 toast 和 provider 选择器高亮
7. **并发安全**：同时打开多个标签页，一方删除会话/线程，另一方列表自动同步
8. **事件转发订阅**：恢复历史线程后，发送新消息验证实时事件流正常接收
9. **数据一致性**：删除线程后验证会话的 `thread_count` 正确递减

---

## 工作量估算

| 模块 | 任务 | 优先级 |
|------|------|--------|
| 后端 | 完整实现 `rename_session`（DB UPDATE + facade + Tauri command） | P1 |
| 后端 | 新增 Tauri command：`list_sessions`、`list_threads`、`delete_session` | P1 |
| 后端 | 新增 Tauri command：`delete_thread`（后端已有） | P1 |
| 后端 | `ThreadSummary` 新增 `provider_id` 字段 | P1 |
| 前端 | Tauri command wrappers | P1 |
| 前端 | chat-store 状态管理（含与 `sessionsByKey` 共存设计） | P1 |
| 前端 | SessionList / SessionItem 组件 | P1 |
| 前端 | ThreadItem 组件 | P1 |
| 前端 | 右键菜单（重命名/删除对话框） | P2 |
| 前端 | Provider 恢复检查逻辑 + 事件转发订阅 | P1 |
| 前端 | 空状态 / 加载状态 UI | P2 |
