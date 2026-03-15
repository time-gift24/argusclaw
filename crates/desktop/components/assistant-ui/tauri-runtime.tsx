"use client";

import { ReactNode, useCallback } from "react";
import {
  useExternalStoreRuntime,
  AssistantRuntimeProvider,
  ThreadMessageLike,
  AppendMessage,
} from "@assistant-ui/react";
import { useThread } from "@/app/hooks/useThread";
import {
  FALLBACK_THREAD_ID,
  useResolvedThreadId,
} from "@/app/hooks/useResolvedThreadId";

interface TauriRuntimeProviderProps {
  children: ReactNode;
  threadId?: string;
}

export function TauriRuntimeProvider({
  children,
  threadId,
}: TauriRuntimeProviderProps) {
  const {
    threadId: resolvedThreadId,
    isReady,
    error: threadResolutionError,
  } = useResolvedThreadId(threadId);

  const { messages, isRunning, sendMessage, error } = useThread({
    threadId: resolvedThreadId ?? FALLBACK_THREAD_ID,
    autoSubscribe: isReady,
  });

  console.log("[TauriRuntime] useThread state:", {
    messagesCount: messages.length,
    isRunning,
    error,
    messages: messages.map((m) => ({
      role: m.role,
      content: m.content.substring(0, 50) + "...",
    })),
  });

  // Convert backend messages to assistant-ui ThreadMessageLike format
  const threadMessages: readonly ThreadMessageLike[] = messages.map(
    (message, index) => ({
      id: `msg-${message.role}-${index}`,
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
          (part): part is { type: "text"; text: string } =>
            part.type === "text",
        )
        .map((part) => part.text)
        .join("\n");

      console.log("[TauriRuntime] Extracted textContent:", textContent);

      if (!isReady) {
        return;
      }

      if (textContent.trim()) {
        console.log("[TauriRuntime] Calling sendMessage...");
        await sendMessage(textContent);
        console.log("[TauriRuntime] sendMessage completed");
      } else {
        console.log("[TauriRuntime] textContent is empty, not sending");
      }
    },
    [isReady, sendMessage],
  );

  // Handler for message editing
  const onEdit = useCallback(
    async (message: AppendMessage) => {
      console.log("[TauriRuntime] onEdit called with message:", message);
      const textContent = message.content
        .filter(
          (part): part is { type: "text"; text: string } =>
            part.type === "text",
        )
        .map((part) => part.text)
        .join("\n");

      console.log("[TauriRuntime] onEdit extracted textContent:", textContent);

      if (!isReady) {
        return;
      }

      if (textContent.trim()) {
        console.log("[TauriRuntime] onEdit calling sendMessage...");
        await sendMessage(textContent);
        console.log("[TauriRuntime] onEdit sendMessage completed");
      }
    },
    [isReady, sendMessage],
  );

  // Handler for cancellation
  const onCancel = useCallback(async () => {
    // TODO: Implement cancellation via Tauri command
    console.log("[TauriRuntime] Cancel requested");
  }, []);

  console.log("[TauriRuntime] Creating runtime with:", {
    messagesCount: threadMessages.length,
    isRunning,
    isReady,
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

  if (!isReady && !threadResolutionError) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        Connecting to ArgusAgent...
      </div>
    );
  }

  if (threadResolutionError) {
    return (
      <div className="flex h-full items-center justify-center px-4 text-sm text-destructive">
        {threadResolutionError}
      </div>
    );
  }

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      {children}
    </AssistantRuntimeProvider>
  );
}
