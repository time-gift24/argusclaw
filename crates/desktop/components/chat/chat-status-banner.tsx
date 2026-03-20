"use client";

import { useActiveChatSession } from "@/hooks/use-active-chat-session";
import { useChatStore } from "@/lib/chat-store";

const DEFAULT_PROVIDER_ERROR = "No default provider configured";

export function ChatStatusBanner() {
  const sessionError = useActiveChatSession()?.error;
  const storeError = useChatStore((state) => state.errorMessage);
  const message = sessionError ?? storeError;

  if (!message) return null;

  const isDefaultProviderError = message.includes(DEFAULT_PROVIDER_ERROR);

  if (isDefaultProviderError) {
    return (
      <div className="rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-sm text-amber-700">
        智能体未配置 Provider，将使用全局配置的 Provider
      </div>
    );
  }

  return (
    <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
      {message}
    </div>
  );
}
