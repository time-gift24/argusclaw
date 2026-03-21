import { useChatStore } from "@/lib/chat-store";

export function useActiveChatSession() {
  return useChatStore((state) => {
    const activeSessionId = state.activeSessionId;
    if (!activeSessionId) return null;
    return state.sessionsByKey[activeSessionId.toString()] ?? null;
  });
}
