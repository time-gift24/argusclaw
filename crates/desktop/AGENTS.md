先读同目录的 `CLAUDE.md`。

处理 desktop chat 时，默认同时核对这 3 层是否一致：
- `lib/chat-store.ts`：会话、pending 态、事件收口
- `lib/chat-runtime.ts`：assistant-ui message 映射与 turn 聚合
- `components/assistant-ui/thread.tsx`：最终可见的 transcript / artifacts UI

desktop chat 的 UX 底线：
- 用户消息发送后必须先在前端可见，不能等后端 snapshot 刷新
- 同一 turn 的 reasoning / tool artifacts 统一成一组渲染，不拆成多段“思考完成 + 工具调用”
- 改动 chat 行为时同步补 `tests/`，优先覆盖 store/runtime 的根因，而不是只测样式
