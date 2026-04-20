# ArgusWing Desktop

> 特性：React 19 + Vite 8 + Tauri 2 桌面前端，承载设置、聊天与 thread monitor 界面。

## 作用域

- 本文件适用于 `crates/desktop/` 及其子目录。
- 如果需要 Rust bridge 细节，继续看 `src-tauri/AGENTS.md`。

## 目录结构

- `app/`：route-level 页面与布局
- `components/assistant-ui`、`chat`、`settings`、`thread-monitor`、`ui`：界面组件
- `lib/`：runtime、store、Tauri 绑定与共享类型
- `hooks/`：前端状态钩子
- `tests/`：UI 回归与绑定测试
- `src-tauri/`：Rust bridge，见其本地 `AGENTS.md`

## UI 约束

- 无 SSR，全部按客户端渲染思路设计
- 用户可见文案默认中文
- 保持紧凑桌面密度，常见导航 / 表单 / 按钮文本以 `text-sm` 为主
- `settings`、`chat`、`thread-monitor` 页面延续现有 navbar + breadcrumb + `max-w-7xl` 内容宽度

## 开发命令

```bash
pnpm dev
pnpm tauri dev
pnpm build
pnpm tauri build
```

## 修改守则

- 共享状态优先放在 `lib/*` 或 `hooks/*`，不要在页面组件里偷偷造全局单例
- Tauri contract 变更时，要同步检查 `src-tauri`、`lib/tauri.ts` 与相关测试
- `dist/`、`node_modules/` 不是事实来源，不在里面做手改

## Chat 协作约束

处理 desktop chat 时，默认同时核对这 3 层是否一致：

- `lib/chat-store.ts`：会话、pending 态、事件收口
- `lib/chat-runtime.ts`：assistant-ui message 映射与 turn 聚合
- `components/assistant-ui/thread.tsx`：最终可见的 transcript / artifacts UI

## Chat 数据流

- `lib/chat-store.ts` 维护 desktop chat 的权威前端状态；聊天链路变更优先落在 store，而不是散落到页面组件里临时修
- `lib/chat-runtime.ts` 只负责把 store/session 状态转换成 assistant-ui runtime message；不要在组件里重复拼装 transcript
- 后端快照 `session.messages` 是已落库 transcript；前端临时态只用于“快照尚未刷新前”的可见性补偿
- 同一轮 assistant/tool 循环要在 runtime 中聚合成一个 assistant turn，并通过 `metadata.custom.turnArtifacts` 传给 UI

## Chat 交互不变量

- 用户消息发送后必须先在前端可见，不能等后端 snapshot 刷新
- 同一 turn 的 reasoning / tool artifacts 统一成一组渲染，不拆成多段“思考完成 + 工具调用”
- `pendingAssistant`、`pendingUserMessage` 这类前端临时态必须在成功 refresh 或失败收口时被清理，避免跨 thread/session 泄漏
- 修改 chat 行为时，至少同时检查 `lib/chat-store.ts`、`lib/chat-runtime.ts`、`components/assistant-ui/thread.tsx` 和对应 `tests/`
- 改动 chat 行为时同步补 `tests/`，优先覆盖 store/runtime 的根因，而不是只测样式
