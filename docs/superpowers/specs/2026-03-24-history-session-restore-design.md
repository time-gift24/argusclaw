# 历史会话恢复功能设计

**日期**: 2026-03-24
**状态**: 设计中
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
| `list_sessions` | - | `Vec<SessionSummary>` | 已有 |
| `list_threads` | `session_id: i64` | `Vec<ThreadSummary>` | 已有 |
| `rename_session` | `session_id: i64, name: String` | `()` | **需新增** |
| `delete_session` | `session_id: i64` | `()` | 已有 |
| `delete_thread` | `thread_id: String` | `()` | **需新增** |
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

#### ThreadSummary（已有）

```rust
pub struct ThreadSummary {
    pub id: ThreadId,        // UUID
    pub title: Option<String>,
    pub turn_count: i64,
    pub token_count: i64,
    pub updated_at: DateTime<Utc>,
}
```

### 前端（chat-store.ts）

新增状态：

```typescript
interface SessionStore {
  // 历史会话列表
  sessions: SessionSummary[];
  expandedSessions: Set<number>;   // session id 集合
  threads: Map<number, ThreadSummary[]>;  // sessionId -> threads

  // 加载状态
  historyLoading: boolean;

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

1. **`crates/desktop/src-tauri/src/commands.rs`**：新增 `rename_session` 和 `delete_thread` 命令
2. **`crates/argus-session/src/manager.rs`**：确认/实现 `rename_session` 和 `delete_thread` 方法
3. **`crates/argus-repository`**：确认 `delete_thread` 在 `ThreadRepository` trait 中存在

### 前端改动

1. **`crates/desktop/lib/chat-store.ts`**：新增 `sessions`、`threads`、`expandedSessions` 状态和相关方法
2. **`crates/desktop/components/`**：新增组件（参考上方组件设计）
3. **`crates/desktop/lib/tauri.ts`**：新增 Tauri command wrapper
4. **`crates/desktop/app/`**：新增历史页面路由或整合到现有侧边栏

### Provider 恢复检查

在 `loadThread()` 流程中：

1. 获取 `ThreadSummary`（含 `provider_id`）
2. 调用 `get_provider(provider_id)`
3. 若返回 null → toast 提示"Provider 不存在，请选择新的"
4. 若返回有效 → 调用 `get_thread_snapshot` 并加载消息

---

## 错误处理

| 场景 | 处理 |
|------|------|
| `list_sessions` 失败 | toast "加载历史会话失败" |
| `rename_session` 失败 | toast "重命名失败" |
| `delete_session` 失败 | toast "删除会话失败" |
| `delete_thread` 失败 | toast "删除线程失败" |
| `get_thread_snapshot` 失败 | toast "加载历史消息失败" |
| Provider 不存在 | toast "该会话使用的 Provider 已不存在，请选择一个新的" |

---

## 测试计划

1. **会话列表加载**：无会话 / 有多个会话 / 大量会话（100+）
2. **展开/折叠**：快速连续点击、展开多个
3. **恢复历史线程**：消息数 1 / 100 / 1000 条
4. **重命名**：空名称 / 正常名称 / 超长名称
5. **删除**：取消确认、确认删除
6. **Provider 不存在场景**：模拟 provider 已删除的情况

---

## 工作量估算

| 模块 | 任务 | 优先级 |
|------|------|--------|
| 后端 | 确认/实现 `rename_session` | P1 |
| 后端 | 确认/实现 `delete_thread` | P1 |
| 前端 | Tauri command wrappers | P1 |
| 前端 | chat-store 状态管理 | P1 |
| 前端 | SessionList / SessionItem 组件 | P1 |
| 前端 | ThreadItem 组件 | P1 |
| 前端 | 右键菜单（重命名/删除对话框） | P2 |
| 前端 | Provider 恢复检查逻辑 | P1 |
| 前端 | 空状态 / 加载状态 UI | P2 |
