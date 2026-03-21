"use client";

import * as React from "react";
import { AssistantRuntimeProvider } from "@assistant-ui/react";

import { Thread } from "@/components/assistant-ui/thread";
import { useChatRuntime } from "@/lib/chat-runtime";
import { useChatStore } from "@/lib/chat-store";
import { useThreadListStore } from "@/lib/thread-list-store";

export function ChatScreen() {
  const runtime = useChatRuntime();
  const initialize = useChatStore((state) => state.initialize);
  const activateSession = useChatStore((state) => state.activateSession);
  const setSelectedTemplateId = useChatStore((state) => state.selectTemplateId);
  const sessions = useThreadListStore((s) => s.sessions);
  const activeSessionId = useThreadListStore((s) => s.activeSessionId);

  const prevActiveIdRef = React.useRef<number | null>(null);

  React.useEffect(() => {
    void initialize();
  }, [initialize]);

  // React to active session changes from the thread sidebar
  React.useEffect(() => {
    if (activeSessionId === null || activeSessionId === prevActiveIdRef.current) return;
    prevActiveIdRef.current = activeSessionId;

    const session = sessions.find((s) => s.id === activeSessionId);
    if (!session) return;

    void activateSession(session.id, session.template_id ?? 0, session.provider_id ?? null);
  }, [activeSessionId, sessions, activateSession]);

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <Thread />
    </AssistantRuntimeProvider>
  );
}
