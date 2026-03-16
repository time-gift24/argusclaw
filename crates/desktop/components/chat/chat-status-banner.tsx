"use client";

import { useActiveChatSession } from "@/hooks/use-active-chat-session";
import { useChatStore } from "@/lib/chat-store";

export function ChatStatusBanner() {
  const sessionError = useActiveChatSession()?.error;
  const storeError = useChatStore((state) => state.errorMessage);
  const message = sessionError ?? storeError;

  if (!message) return null;

  return (
    <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
      {message}
    </div>
  );
}
