# Token 占比环设计规格

## 目标

在 chat 对话框右下角（发送按钮左侧）增加一个 Ring 类型的 token 占比环，直观显示当前 thread 的 token 消耗占模型上下文窗口的比例。

## 数据来源

| 数据 | 来源 | 方式 |
|------|------|------|
| `token_count` | Rust `ThreadSnapshotPayload` | 通过 Tauri IPC 拉取，持久化到 session state |
| `context_window` | Rust `LlmProvider.context_window()` | 新增 Tauri 命令 `get_provider_context_window` |

## 已知限制

- **`Compacted` 事件是死事件**：Rust `Thread` 的 compactor 在 `send_message` 内部运行，但从未 emit `ThreadEvent::Compacted`。因此前端无法感知上下文压缩，环在压缩后不会更新至压缩后的估算值。这超出本 spec 范围，需单独处理。
- **token_count 估算不精确**：发送消息后（turn 执行前）和压缩后，`token_count` 使用 `estimate_tokens()`（`len/4`）重算，与 LLM 返回的精确值不一致。环的显示会有跳变（例：准确值 40k → 估算 60k → 压缩后估算 35k → turn 完成后准确值 45k）。这是当前架构的固有 trade-off，不在本 spec 范围内修复。

## 实现方案

### 1. Rust 层 — 新增 Tauri 命令

**文件**: `crates/desktop/src-tauri/src/commands.rs`

新增命令，通过 `provider_id` 获取其 `LlmProvider.context_window()`：

```rust
#[tauri::command]
async fn get_provider_context_window(
    provider_id: i64,
) -> Result<u32, String>
```

`LlmProvider::context_window()` 不接受 model 参数，统一返回 provider 的上下文窗口大小。provider 不存在时返回默认 `128000`。

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

`newSessionState`（`addSession` / `activateSession` 对应处）新增 `tokenCount: 0`。

在 `fetchThreadSnapshot` 返回后更新 `tokenCount`（在现有更新 `messages` 的逻辑旁）：

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
  providerId: number
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

**依赖**: `@/components/assistant-ui/context-display`（assistant-ui 包自带，参见 [Context Display](https://www.assistant-ui.com/docs/ui/context-display)）。

### 5. TurnCompleted 事件更新 token_count

**文件**: `crates/desktop/src-tauri/src/events/thread.rs`

`TurnCompleted` 事件已包含 `token_usage`。现有 `chat-store.ts` 中 `case "turn_completed":` 为 no-op（直接 `break`），需替换为更新逻辑：

```typescript
case "turn_completed": {
  const { token_usage } = payload as ThreadEventPayload["turn_completed"];
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

**注意**: `Idle` 事件触发 `refreshSnapshot` 时，`snapshot.token_count` 已是 `apply_turn_output` 后的准确值，与上述 `TurnCompleted` 逻辑一致，无需额外处理。

## 文件变更清单

| 文件 | 改动 |
|------|------|
| `crates/desktop/src-tauri/src/commands.rs` | 新增 `get_provider_context_window` 命令（参数为 `provider_id: i64`） |
| `crates/desktop/src-tauri/src/lib.rs` | 注册新命令 |
| `crates/desktop/lib/tauri.ts` | 新增 `getProviderContextWindow` 封装 |
| `crates/desktop/lib/chat-store.ts` | `ChatSessionState` 新增 `tokenCount`、`contextWindow`；`newSessionState` 初始化 `tokenCount: 0`；`fetchThreadSnapshot` 后更新 `tokenCount`；`turn_completed` 事件处理更新 `tokenCount` |
| `crates/desktop/components/assistant-ui/thread.tsx` | `ComposerAction` 新增 `<ContextDisplay.Ring>` |
| `crates/desktop/components/assistant-ui/context-display.tsx` | 确认存在（assistant-ui 包自带） |

## 设计原则

- **渐进增强**: 若 `tokenCount` 或 `contextWindow` 为 0/null，环不渲染，保持 UI 干净
- **零额外请求**: context_window 仅在 session 激活且值为 null 时请求一次，之后缓存
- **无状态破坏**: tokenCount 为 0 表示空闲状态，不影响现有逻辑
