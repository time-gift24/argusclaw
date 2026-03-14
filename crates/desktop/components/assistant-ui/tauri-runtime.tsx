"use client";

import { ReactNode, useCallback } from "react";
import {
  useExternalStoreRuntime,
  AssistantRuntimeProvider,
  ThreadMessageLike,
  AppendMessage,
} from "@assistant-ui/react";
import { useThread } from "@/app/hooks/useThread";

interface TauriRuntimeProviderProps {
  children: ReactNode;
  threadId?: string;
}

export function TauriRuntimeProvider({
  children,
  threadId = "default",
}: TauriRuntimeProviderProps) {
  const { messages, isRunning, sendMessage, error } = useThread({
    threadId,
    autoSubscribe: true,
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
      // Extract text content from the message
      const textContent = message.content
        .filter(
          (part): part is { type: "text"; text: string } => part.type === "text",
        )
        .map((part) => part.text)
        .join("\n");

      if (textContent.trim()) {
        await sendMessage(textContent);
      }
    },
    [sendMessage],
  );

  // Handler for message editing
  const onEdit = useCallback(
    async (message: AppendMessage) => {
      const textContent = message.content
        .filter(
          (part): part is { type: "text"; text: string } => part.type === "text",
        )
        .map((part) => part.text)
        .join("\n");

      if (textContent.trim()) {
        await sendMessage(textContent);
      }
    },
    [sendMessage],
  );

  // Handler for cancellation
  const onCancel = useCallback(async () => {
    // TODO: Implement cancellation via Tauri command
    console.log("Cancel requested");
  }, []);

  const runtime = useExternalStoreRuntime({
    messages: threadMessages,
    isRunning,
    onNew,
    onEdit,
    onCancel,
    // Show error in UI if present
    isDisabled: !!error,
    // Identity conversion since we already use ThreadMessageLike format
    convertMessage: (msg) => msg,
  });

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      {children}
    </AssistantRuntimeProvider>
  );
}
