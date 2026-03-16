import { useExternalStoreRuntime, type ExternalStoreRuntime } from "@assistant-ui/react";

import { useActiveChatSession } from "@/hooks/use-active-chat-session";
import { useChatStore } from "@/lib/chat-store";
import type { ChatMessagePayload } from "@/lib/types/chat";

function buildAssistantUiMessages(session: ReturnType<typeof useActiveChatSession>) {
  if (!session) return [];

  return session.messages.map((msg: ChatMessagePayload, index: number) => ({
    id: `msg-${index}`,
    role: msg.role as "system" | "user" | "assistant",
    content: [{ type: "text" as const, text: msg.content }],
  }));
}

export function useChatRuntime(): ExternalStoreRuntime {
  const sendMessage = useChatStore((state) => state.sendMessage);
  const session = useActiveChatSession();

  return useExternalStoreRuntime({
    isRunning: session?.status === "running",
    messages: buildAssistantUiMessages(session),
    sendMessage: async (content) => {
      await sendMessage(typeof content === "string" ? content : content.text);
    },
    cancel: undefined,
  });
}
