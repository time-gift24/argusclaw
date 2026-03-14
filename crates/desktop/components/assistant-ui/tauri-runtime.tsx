"use client";

import { ReactNode, useCallback } from "react";
import {
  useExternalStoreRuntime,
  AssistantRuntimeProvider,
  ThreadMessageLike,
  AppendMessage,
} from "@assistant-ui/react";
import { useThread } from "@/app/hooks/useThread";

// Default thread ID as a valid UUID (deterministic for "default" thread)
const DEFAULT_THREAD_ID = "00000000-0000-0000-0000-000000000001";

console.log("[TauriRuntime] Module loaded, DEFAULT_THREAD_ID:", DEFAULT_THREAD_ID);

interface TauriRuntimeProviderProps {
  children: ReactNode;
  threadId?: string;
}

export function TauriRuntimeProvider({
  children,
  threadId = DEFAULT_THREAD_ID,
}: TauriRuntimeProviderProps) {
  console.log("[TauriRuntime] Provider rendered with threadId:", threadId);

  const { messages, isRunning, sendMessage, error } = useThread({
    threadId,
    autoSubscribe: true,
  });

  console.log("[TauriRuntime] useThread state:", {
    messagesCount: messages.length,
    isRunning,
    error,
    messages: messages.map(m => ({ role: m.role, content: m.content.substring(0, 50) + "..." }))
  });

  // Convert backend messages to assistant-ui ThreadMessageLike format
  const threadMessages: readonly ThreadMessageLike[] = messages.map(
    (message, index) => ({
      id: `msg-${index}-${Date.now()}`,
      role: message.role as "user" | "assistant" | "system",
      content: message.content,
    }),
  );

  // Handler for new messages from the user
  const onNew = useCallback(
    async (message: AppendMessage) => {
      console.log("[TauriRuntime] onNew called with message:", message);
      // Extract text content from the message
      const textContent = message.content
        .filter(
          (part): part is { type: "text"; text: string } => part.type === "text",
        )
        .map((part) => part.text)
        .join("\n");

      console.log("[TauriRuntime] Extracted textContent:", textContent);

      if (textContent.trim()) {
        console.log("[TauriRuntime] Calling sendMessage...");
        await sendMessage(textContent);
        console.log("[TauriRuntime] sendMessage completed");
      } else {
        console.log("[TauriRuntime] textContent is empty, not sending");
      }
    },
    [sendMessage],
  );

  // Handler for message editing
  const onEdit = useCallback(
    async (message: AppendMessage) => {
      console.log("[TauriRuntime] onEdit called with message:", message);
      const textContent = message.content
        .filter(
          (part): part is { type: "text"; text: string } => part.type === "text",
        )
        .map((part) => part.text)
        .join("\n");

      console.log("[TauriRuntime] onEdit extracted textContent:", textContent);

      if (textContent.trim()) {
        console.log("[TauriRuntime] onEdit calling sendMessage...");
        await sendMessage(textContent);
        console.log("[TauriRuntime] onEdit sendMessage completed");
      }
    },
    [sendMessage],
  );

  // Handler for cancellation
  const onCancel = useCallback(async () => {
    // TODO: Implement cancellation via Tauri command
    console.log("[TauriRuntime] Cancel requested");
  }, []);

  console.log("[TauriRuntime] Creating runtime with:", {
    messagesCount: threadMessages.length,
    isRunning,
  });

  const runtime = useExternalStoreRuntime({
    messages: threadMessages,
    isRunning,
    onNew,
    onEdit,
    onCancel,
    // Identity conversion since we already use ThreadMessageLike format
    convertMessage: (msg) => msg,
  });

  console.log("[TauriRuntime] Runtime created, rendering provider");

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      {children}
    </AssistantRuntimeProvider>
  );
}
