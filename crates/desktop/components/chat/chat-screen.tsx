"use client";

import * as React from "react";
import { AssistantRuntimeProvider } from "@assistant-ui/react";

import { Thread } from "@/components/assistant-ui/thread";
import { useChatRuntime } from "@/lib/chat-runtime";
import { useChatStore } from "@/lib/chat-store";

export function ChatScreen() {
  const runtime = useChatRuntime();
  const initialize = useChatStore((state) => state.initialize);

  React.useEffect(() => {
    void initialize();
  }, [initialize]);

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <Thread />
    </AssistantRuntimeProvider>
  );
}
