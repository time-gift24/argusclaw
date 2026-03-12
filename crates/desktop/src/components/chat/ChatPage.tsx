// crates/desktop/src/components/chat/ChatPage.tsx

import { AssistantRuntimeProvider, Thread } from "@assistant-ui/react";
import { useMockRuntime } from "@/hooks/useMockRuntime";

/**
 * 聊天页面组件
 * 使用 assistant-ui 的 Thread 组件和自定义 Markdown 渲染
 */
export function ChatPage() {
  const runtime = useMockRuntime();

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <div className="flex flex-col h-full">
        <Thread />
      </div>
    </AssistantRuntimeProvider>
  );
}
