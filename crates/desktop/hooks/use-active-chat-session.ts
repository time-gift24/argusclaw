import { useChatStore } from "@/lib/chat-store";

export function useActiveChatSession() {
  return useChatStore((state) =>
    state.activeSessionKey ? state.sessionsByKey[state.activeSessionKey] ?? null : null,
  );
}
