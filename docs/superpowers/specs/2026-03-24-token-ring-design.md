# Token 占比环设计规格

## 目标

在 chat 对话框右下角（发送按钮左侧）增加一个 Ring 类型的 token 占比环，直观显示当前 thread 的 token 消耗占模型上下文窗口的比例。

## 数据来源

| 数据 | 来源 | 方式 |
|------|------|------|
| `token_count` | Rust `ThreadSnapshotPayload` | 通过 Tauri IPC 拉取，持久化到 session state |
| `context_window` | Rust `LlmProvider.context_window()` | 新增 Tauri 命令 `get_provider_context_window` |

## 实现方案

### 1. Rust 层 — 新增 Tauri 命令

**文件**: `crates/desktop/src-tauri/src/commands.rs`

新增命令，获取指定 provider 和模型的上下文窗口大小：

```rust
#[tauri::command]
async fn get_provider_context_window(
    provider_id: i32,
    model: String,
) -> Result<u32, String>
```

内部调用 `LlmProvider.context_window()`，provider 不存在或模型查询失败时返回默认 `128000`。

### 2. Frontend — 持久化 token_count

**文件**: `crates/desktop/lib/chat-store.ts`

`ChatSessionState` 新增字段：

```typescript
interface ChatSessionState {
  // ...existing fields
  tokenCount: number;
  contextWindow: number | null;  // null = 未获取
}
```

`setSession` / `addSession` 时同时设置 `tokenCount`，初始为空闲时设为 0。

在 `fetchThreadSnapshot` 返回后更新 `tokenCount`：

```typescript
store.setState((s) => ({
  sessionsByKey: {
    ...s.sessionsByKey,
    [sessionKey]: {
      ...s.sessionsByKey[sessionKey],
      tokenCount: snapshot.token_count,
    },
  },
}));
```

### 3. Frontend — 获取 context_window

**文件**: `crates/desktop/lib/tauri.ts`

新增 TypeScript 封装：

```typescript
export async function getProviderContextWindow(
  providerId: number,
  model: string
): Promise<number>
```

`ComposerAction` 组件内，通过 `useActiveChatSession` 获取当前 session，从 session state 读取 `contextWindow`；若为 null，则调用上述命令获取并回填 session state。

### 4. UI — Ring 组件

**文件**: `crates/desktop/components/assistant-ui/thread.tsx`

在 `ComposerAction` 的右侧区域（send 按钮左侧）插入：

```tsx
{session.tokenCount > 0 && session.contextWindow && (
  <ContextDisplay.Ring
    modelContextWindow={session.contextWindow}
    className="size-8"
    side="top"
  />
)}
```

样式：`size-8`（32px），与 send 按钮视觉对齐。

**依赖**: 确保 `@/components/assistant-ui/context-display` 存在（已由 assistant-ui 包提供，参见 [Context Display](https://www.assistant-ui.com/docs/ui/context-display)）。

### 5. TurnCompleted 事件更新 token_count

**文件**: `crates/desktop/src-tauri/src/events/thread.rs`

`TurnCompleted` 事件已包含 `token_usage`。前端在处理 `TurnCompleted` 事件时，更新 session state 的 `tokenCount`：

```typescript
// chat-store.ts 中的事件处理
case "TurnCompleted": {
  const { token_usage } = payload as ThreadEventPayload["TurnCompleted"];
  store.setState((s) => ({
    sessionsByKey: {
      ...s.sessionsByKey,
      [sessionKey]: {
        ...s.sessionsByKey[sessionKey],
        tokenCount: token_usage.total_tokens,
      },
    },
  }));
}
```

## 文件变更清单

| 文件 | 改动 |
|------|------|
| `crates/desktop/src-tauri/src/commands.rs` | 新增 `get_provider_context_window` 命令 |
| `crates/desktop/src-tauri/src/lib.rs` | 注册新命令 |
| `crates/desktop/lib/tauri.ts` | 新增 `getProviderContextWindow` 封装 |
| `crates/desktop/lib/chat-store.ts` | `ChatSessionState` 新增 `tokenCount`、`contextWindow`；事件处理更新 tokenCount |
| `crates/desktop/components/assistant-ui/thread.tsx` | `ComposerAction` 新增 `<ContextDisplay.Ring>` |
| `crates/desktop/components/assistant-ui/context-display.tsx` | 可能需要确认存在（assistant-ui 包自带） |

## 设计原则

- **渐进增强**: 若 `tokenCount` 或 `contextWindow` 为 0/null，环不渲染，保持 UI 干净
- **零额外请求**: context_window 仅在 session 激活且值为 null 时请求一次，之后缓存
- **无状态破坏**: tokenCount 为 0 表示空闲状态，不影响现有逻辑
