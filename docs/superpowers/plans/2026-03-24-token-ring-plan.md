# Token Ring Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 chat composer 右下角（发送按钮左侧）增加 Token 占比环，显示 `token_count / context_window`。

**Architecture:** 复用 assistant-ui 内置 `ContextDisplay.Ring` 组件；`tokenCount` 通过 `turn_completed` 事件和 `refreshSnapshot` 持久化到 Zustand session state；`contextWindow` 通过新增 Tauri 命令获取并缓存。

**Tech Stack:** Rust (Tauri), TypeScript (Zustand), Tailwind CSS v4, assistant-ui

**Spec:** `docs/superpowers/specs/2026-03-24-token-ring-design.md`

---

## Chunk 1: Rust — Tauri 命令

### Context

`ArgusWing` 已有 `provider_manager.get_provider_record()` 但缺少 `get_provider()`（返回 provider 实例）。`LlmProvider::context_window()` 需要 provider 实例才能调用。

**Files:**
- Modify: `crates/argus-wing/src/lib.rs` — 新增 `get_provider` 方法
- Modify: `crates/desktop/src-tauri/src/commands.rs` — 新增 `get_provider_context_window` 命令
- Modify: `crates/desktop/src-tauri/src/lib.rs` — 注册新命令

---

- [ ] **Step 1: Add `get_provider` to `ArgusWing`**

文件: `crates/argus-wing/src/lib.rs`

在 Provider CRUD API 区块末尾（`get_default_provider_record` 之后）添加：

```rust
/// Get a provider instance by ID (for calling methods like context_window).
pub async fn get_provider(&self, id: LlmProviderId) -> Result<Arc<dyn LlmProvider>> {
    self.provider_manager.get_provider(&id).await
}
```

> `Arc<dyn LlmProvider>` 已在 import 中（`use argus_protocol::LlmProviderId;`），确认 `LlmProvider` 在 scope 中。

---

- [ ] **Step 2: Add `get_provider_context_window` Tauri command**

文件: `crates/desktop/src-tauri/src/commands.rs`

在文件末尾（在最后一个 command 之后）添加：

```rust
#[tauri::command]
pub async fn get_provider_context_window(
    wing: State<'_, Arc<ArgusWing>>,
    provider_id: i64,
) -> Result<u32, String> {
    let id = LlmProviderId::new(provider_id);
    match wing.get_provider(id).await {
        Ok(provider) => Ok(provider.context_window()),
        Err(_) => Ok(128_000), // provider not found or build failed, use default
    }
}
```

---

- [ ] **Step 3: Register new command in lib.rs**

文件: `crates/desktop/src-tauri/src/lib.rs`

在 commands 导入列表中添加 `commands::get_provider_context_window`（找到现有的 commands 导入行，在其中添加）：

```rust
commands::get_provider_context_window,
```

---

- [ ] **Step 4: Verify Rust builds**

Run: `cargo build -p argus-wing -p desktop 2>&1`
Expected: Clean build, no errors

---

- [ ] **Step 5: Commit**

```bash
git add crates/argus-wing/src/lib.rs crates/desktop/src-tauri/src/commands.rs crates/desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): add get_provider_context_window Tauri command"
```

---

## Chunk 2: TypeScript — Tauri Wrapper

### Context

`tauri.ts` 中的 `providers` 对象已封装了 provider 相关的 invoke 调用，遵循相同模式添加 `getProviderContextWindow`。

**Files:**
- Modify: `crates/desktop/lib/tauri.ts`

---

- [ ] **Step 1: Add `getProviderContextWindow` to providers namespace**

文件: `crates/desktop/lib/tauri.ts`

在 `providers` 对象末尾（`testInput` 之后）添加：

```typescript
getContextWindow: (providerId: number) =>
  invoke<number>("get_provider_context_window", { providerId }),
```

---

- [ ] **Step 2: Verify TypeScript**

Run: `cd crates/desktop && pnpm tsc --noEmit 2>&1`
Expected: No errors (or only pre-existing errors unrelated to this change)

---

- [ ] **Step 3: Commit**

```bash
git add crates/desktop/lib/tauri.ts
git commit -m "feat(desktop): add getProviderContextWindow wrapper"
```

---

## Chunk 3: Chat Store — State Management

### Context

`ChatSessionState` 需要新增 `tokenCount` 和 `contextWindow` 字段。`newSessionState` 初始化 `tokenCount: 0`，`refreshSnapshot` 更新 `tokenCount`，`turn_completed` 事件处理从 no-op 改为更新 `tokenCount`。

**Files:**
- Modify: `crates/desktop/lib/chat-store.ts`
- Modify: `crates/desktop/lib/types/chat.ts`

---

- [ ] **Step 1: Add new fields to `ChatSessionState` interface**

文件: `crates/desktop/lib/chat-store.ts`，在 `ChatSessionState` 接口定义中（`error: string | null` 之后）添加：

```typescript
  tokenCount: number;
  contextWindow: number | null;
```

---

- [ ] **Step 2: Initialize fields in `newSessionState`**

文件: `crates/desktop/lib/chat-store.ts`，`newSessionState` 对象中（`error: null` 之后）添加：

```typescript
        tokenCount: 0,
        contextWindow: null,
```

---

- [ ] **Step 3: Update `refreshSnapshot` to sync `tokenCount`**

文件: `crates/desktop/lib/chat-store.ts`，`refreshSnapshot` 函数中，在 `sessionsByKey` 更新对象里添加 `tokenCount`：

```typescript
            tokenCount: snapshot.token_count,
```

在 `status: "idle"` 之后添加，确保与 `messages` 同一批次更新。

---

- [ ] **Step 4: Replace no-op `turn_completed` handler with token update**

文件: `crates/desktop/lib/chat-store.ts`，找到 `case "turn_completed":` 块（当前只有 `break;`），替换为：

```typescript
      case "turn_completed": {
        const payload = rawPayload as ThreadEventPayload;
        if (payload.type === "turn_completed") {
          const { total_tokens } = payload;
          set((state) => ({
            sessionsByKey: {
              ...state.sessionsByKey,
              [sessionKey]: {
                ...state.sessionsByKey[sessionKey],
                tokenCount: total_tokens,
              },
            },
          }));
        }
        break;
      }
```

> 注意：`_handleThreadEvent` 中的 `rawPayload` 是 `unknown` 类型，需要先断言为 `ThreadEventPayload` 再访问 `type` 字段以获得类型安全。现有其他 case 已有类似模式（如 `case "idle"`）。

---

- [ ] **Step 5: Verify TypeScript**

Run: `cd crates/desktop && pnpm tsc --noEmit 2>&1`
Expected: No errors

---

- [ ] **Step 6: Commit**

```bash
git add crates/desktop/lib/chat-store.ts
git commit -m "feat(desktop): persist tokenCount and contextWindow in chat store"
```

---

## Chunk 4: UI — Ring Component

### Context

在 `ComposerAction` 的右侧区域（send 按钮左侧）插入 `<ContextDisplay.Ring>`。`ContextDisplay` 是 assistant-ui 包内置组件（`@assistant-ui/react` 版本 0.12.17 应已包含）。通过 `useActiveChatSession` 获取 session，从中读取 `tokenCount` 和 `contextWindow`。

**Files:**
- Modify: `crates/desktop/components/assistant-ui/thread.tsx`

---

- [ ] **Step 1: Verify `ContextDisplay` exists in assistant-ui**

Run: `cd crates/desktop && grep -r "ContextDisplay" node_modules/@assistant-ui/react/dist/*.d.ts 2>/dev/null | head -5`
Expected: 输出包含 `ContextDisplay` 类型定义

若不存在，执行：
```bash
cd crates/desktop && pnpm add @assistant-ui/react@latest
```
然后重试。若包无 `ContextDisplay`，需手动实现 SVG ring（见下方 fallback）。

---

- [ ] **Step 2: Import `ContextDisplay`**

文件: `crates/desktop/components/assistant-ui/thread.tsx`

在 import 区块顶部（第一个 import 之后）添加：

```typescript
import { ContextDisplay } from "@assistant-ui/react";
```

---

- [ ] **Step 3: Update `ComposerAction` to render the Ring**

文件: `crates/desktop/components/assistant-ui/thread.tsx`

将现有的 `ComposerAction` 组件：

```tsx
const ComposerAction: FC = () => {
  return (
    <div className="aui-composer-action-wrapper relative mx-2 mb-2 flex items-center justify-between gap-2">
      <div className="flex items-center gap-2">
        <AgentSelector />
        <ProviderSelector />
      </div>
      <ComposerPrimitive.Send asChild>
        <TooltipIconButton tooltip="Send message" side="bottom" type="button" variant="default" size="icon" className="aui-composer-send size-8 rounded-full" aria-label="Send message">
          <ArrowUpIcon className="aui-composer-send-icon size-4" />
        </TooltipIconButton>
      </ComposerPrimitive.Send>
    </div>
  );
};
```

替换为：

```tsx
const ComposerAction: FC = () => {
  const session = useActiveChatSession();

  return (
    <div className="aui-composer-action-wrapper relative mx-2 mb-2 flex items-center justify-between gap-2">
      <div className="flex items-center gap-2">
        <AgentSelector />
        <ProviderSelector />
      </div>
      <div className="flex items-center gap-2">
        {session && session.tokenCount > 0 && session.contextWindow && (
          <ContextDisplay.Ring
            modelContextWindow={session.contextWindow}
            className="size-8"
            side="top"
          />
        )}
        <ComposerPrimitive.Send asChild>
          <TooltipIconButton tooltip="Send message" side="bottom" type="button" variant="default" size="icon" className="aui-composer-send size-8 rounded-full" aria-label="Send message">
            <ArrowUpIcon className="aui-composer-send-icon size-4" />
          </TooltipIconButton>
        </ComposerPrimitive.Send>
      </div>
    </div>
  );
};
```

---

- [ ] **Step 4: Verify TypeScript**

Run: `cd crates/desktop && pnpm tsc --noEmit 2>&1`
Expected: No errors

---

- [ ] **Step 5: Commit**

```bash
git add crates/desktop/components/assistant-ui/thread.tsx
git commit -m "feat(desktop): add ContextDisplay.Ring to ComposerAction"
```

---

## Summary

| Chunk | Scope | Files |
|-------|-------|-------|
| 1 | Rust Tauri command | `argus-wing/src/lib.rs`, `desktop/src-tauri/src/commands.rs`, `desktop/src-tauri/src/lib.rs` |
| 2 | TypeScript wrapper | `desktop/lib/tauri.ts` |
| 3 | Zustand state | `desktop/lib/chat-store.ts` |
| 4 | UI component | `desktop/components/assistant-ui/thread.tsx` |

Total: **4 chunks, ~20 steps**. 每个 chunk 独立可测试。
